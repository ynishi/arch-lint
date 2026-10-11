//! Rendering a [`LintResult`] into text a caller can print.
//!
//! ```no_run
//! use arch_lint::report::{render, Format};
//! use arch_lint::Session;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let result = Session::discover(".")?.run()?;
//! print!("{}", render(&result, Format::default())?);
//! # Ok(())
//! # }
//! ```
//!
//! Every renderer returns the exact bytes to write: the string ends with a
//! newline unless it is empty. ANSI colors are opt-in — the library default is
//! [`Format::Text`] with `color: false`, so piped and captured output stays
//! plain unless the caller asks for colors.

use arch_lint_core::{LintResult, Severity};

mod sarif;

/// Output format for [`render`].
///
/// `#[non_exhaustive]`: added when [`Format::Sarif`] was introduced (issue
/// #11). A caller matching on `Format` from outside this crate must now
/// carry a wildcard arm — see `CHANGELOG.md` `[Unreleased]` → `### Changed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// Multi-line human-readable report, one block per violation.
    Text {
        /// Emit ANSI color escapes for severities and the summary line.
        color: bool,
    },
    /// Pretty-printed JSON of the whole [`LintResult`].
    Json,
    /// One line per violation, no summary.
    Compact,
    /// SARIF 2.1.0 JSON, consumable by `github/codeql-action/upload-sarif`.
    Sarif,
}

impl Default for Format {
    /// [`Format::Text`] without colors.
    fn default() -> Self {
        Self::Text { color: false }
    }
}

/// Errors that can occur while rendering.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// The lint result could not be serialized as JSON.
    #[error("failed to serialize lint result as JSON")]
    Json(#[from] serde_json::Error),
}

/// Renders `result` in `format`.
///
/// # Errors
///
/// Returns [`RenderError::Json`] if [`Format::Json`] or [`Format::Sarif`] is
/// requested and serialization fails.
pub fn render(result: &LintResult, format: Format) -> Result<String, RenderError> {
    match format {
        Format::Text { color } => Ok(TextReport { result, color }.to_string()),
        Format::Json => Ok(format!("{}\n", serde_json::to_string_pretty(result)?)),
        Format::Compact => Ok(CompactReport { result }.to_string()),
        Format::Sarif => Ok(sarif::render(result)?),
    }
}

/// ANSI escape for a severity label, or the bare word when `color` is off.
fn severity_label(severity: Severity, color: bool) -> &'static str {
    match (severity, color) {
        (Severity::Error, true) => "\x1b[31merror\x1b[0m",
        (Severity::Error, false) => "error",
        (Severity::Warning, true) => "\x1b[33mwarning\x1b[0m",
        (Severity::Warning, false) => "warning",
        (Severity::Info, true) => "\x1b[34minfo\x1b[0m",
        (Severity::Info, false) => "info",
    }
}

/// Multi-line report; `to_string()` produces the [`Format::Text`] output.
struct TextReport<'a> {
    result: &'a LintResult,
    color: bool,
}

impl std::fmt::Display for TextReport<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (errors, warnings, infos) = self.result.count_by_severity();

        for violation in &self.result.violations {
            writeln!(
                f,
                "{} {} at {}:{}:{}",
                violation.code,
                violation.rule,
                violation.location.file.display(),
                violation.location.line,
                violation.location.column,
            )?;
            writeln!(
                f,
                "  {}: {}",
                severity_label(violation.severity, self.color),
                violation.message
            )?;
            if let Some(suggestion) = &violation.suggestion {
                writeln!(f, "  = help: {}", suggestion.message)?;
            }
            for label in &violation.labels {
                writeln!(
                    f,
                    "  = note: {} at {}:{}:{}",
                    label.message,
                    label.location.file.display(),
                    label.location.line,
                    label.location.column,
                )?;
            }
            writeln!(f)?;
        }

        let (open, close) = if self.color {
            let open = if errors > 0 {
                "\x1b[31m"
            } else if warnings > 0 {
                "\x1b[33m"
            } else {
                "\x1b[32m"
            };
            (open, "\x1b[0m")
        } else {
            ("", "")
        };

        writeln!(
            f,
            "{open}Found {errors} error(s), {warnings} warning(s), {infos} info(s) in {} file(s){close}",
            self.result.files_checked
        )
    }
}

/// One line per violation; `to_string()` produces the [`Format::Compact`] output.
struct CompactReport<'a> {
    result: &'a LintResult,
}

impl std::fmt::Display for CompactReport<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for violation in &self.result.violations {
            writeln!(
                f,
                "{}:{}:{}: {} [{}] {}",
                violation.location.file.display(),
                violation.location.line,
                violation.location.column,
                violation.severity,
                violation.code,
                violation.message,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arch_lint_core::{Label, Location, Suggestion, Violation};
    use std::path::PathBuf;

    fn sample() -> LintResult {
        let mut result = LintResult::new();
        result.files_checked = 3;
        result.violations.push(
            Violation::new(
                "AL001",
                "no-unwrap-expect",
                Severity::Error,
                Location::new(PathBuf::from("src/lib.rs"), 10, 5),
                "Avoid unwrap()",
            )
            .with_suggestion(Suggestion::new("Use `?` instead")),
        );
        result.violations.push(Violation::new(
            "AL002",
            "no-sync-io",
            Severity::Warning,
            Location::new(PathBuf::from("src/io.rs"), 4, 1),
            "Blocking IO in async fn",
        ));
        result
    }

    #[test]
    fn default_format_is_text_without_color() {
        assert_eq!(Format::default(), Format::Text { color: false });
    }

    #[test]
    fn text_without_color_has_no_escapes() {
        let out = render(&sample(), Format::Text { color: false }).expect("render");
        assert!(!out.contains('\x1b'), "{out}");
        assert!(out.contains("  error: Avoid unwrap()"));
        assert!(out.contains("  = help: Use `?` instead"));
        assert!(out.ends_with("Found 1 error(s), 1 warning(s), 0 info(s) in 3 file(s)\n"));
    }

    #[test]
    fn text_shows_labels_so_a_cycle_shows_its_whole_path() {
        let mut result = LintResult::new();
        result.files_checked = 2;
        result.violations.push(
            Violation::new(
                "ALD009",
                "acyclic",
                Severity::Error,
                Location::new(PathBuf::from("src/domain/a.rs"), 1, 23),
                "Cycle between scopes: domain \u{2192} infra \u{2192} domain",
            )
            .with_label(Label::new(
                Location::new(PathBuf::from("src/infra/b.rs"), 2, 29),
                "infra \u{2192} domain",
            )),
        );

        let out = render(&result, Format::Text { color: false }).expect("render");
        assert!(
            out.contains("Cycle between scopes: domain \u{2192} infra \u{2192} domain"),
            "{out}"
        );
        assert!(
            out.contains("  = note: infra \u{2192} domain at src/infra/b.rs:2:29"),
            "{out}"
        );
    }

    #[test]
    fn text_with_color_has_escapes() {
        let out = render(&sample(), Format::Text { color: true }).expect("render");
        assert!(out.contains("\x1b[31merror\x1b[0m"));
        assert!(out.contains("\x1b[33mwarning\x1b[0m"));
        assert!(out.contains("\x1b[31mFound 1 error(s)"));
        assert!(out.ends_with("in 3 file(s)\x1b[0m\n"));
    }

    #[test]
    fn text_summary_color_reflects_worst_severity() {
        let mut result = LintResult::new();
        result.files_checked = 1;
        let green = render(&result, Format::Text { color: true }).expect("render");
        assert!(green.contains("\x1b[32mFound 0 error(s)"));

        result.violations.push(Violation::new(
            "AL002",
            "no-sync-io",
            Severity::Warning,
            Location::new(PathBuf::from("a.rs"), 1, 1),
            "m",
        ));
        let yellow = render(&result, Format::Text { color: true }).expect("render");
        assert!(yellow.contains("\x1b[33mFound 0 error(s)"));
    }

    #[test]
    fn compact_is_one_line_per_violation() {
        let out = render(&sample(), Format::Compact).expect("render");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "src/lib.rs:10:5: error [AL001] Avoid unwrap()");
        assert_eq!(
            lines[1],
            "src/io.rs:4:1: warning [AL002] Blocking IO in async fn"
        );
    }

    #[test]
    fn compact_is_empty_when_clean() {
        let out = render(&LintResult::new(), Format::Compact).expect("render");
        assert_eq!(out, "");
    }

    #[test]
    fn json_matches_serde_pretty_output() {
        let result = sample();
        let out = render(&result, Format::Json).expect("render");
        let expected = format!(
            "{}\n",
            serde_json::to_string_pretty(&result).expect("serialize")
        );
        assert_eq!(out, expected);
    }
}
