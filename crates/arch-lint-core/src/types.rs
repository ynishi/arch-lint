//! Core types for lint violations and results.

use miette::{Diagnostic, SourceSpan};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Severity level for lint violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational message, does not fail lint.
    Info,
    /// Warning that should be addressed.
    Warning,
    /// Error that must be fixed.
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Source code location.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    /// File path relative to project root.
    pub file: PathBuf,
    /// Line number (1-indexed).
    pub line: usize,
    /// Column number (1-indexed).
    pub column: usize,
    /// Byte offset in file (for miette integration).
    pub offset: usize,
    /// Length of the span in bytes.
    pub length: usize,
}

impl Location {
    /// Creates a new location from span information.
    #[must_use]
    pub fn from_span(file: PathBuf, span: proc_macro2::Span) -> Self {
        let start = span.start();
        Self {
            file,
            line: start.line,
            column: start.column + 1,
            offset: 0, // Will be calculated from content
            length: 0, // Will be calculated from span
        }
    }

    /// Creates a new location with explicit values.
    #[must_use]
    pub fn new(file: PathBuf, line: usize, column: usize) -> Self {
        Self {
            file,
            line,
            column,
            offset: 0,
            length: 0,
        }
    }

    /// Sets the byte offset and length for this location.
    #[must_use]
    pub fn with_span(mut self, offset: usize, length: usize) -> Self {
        self.offset = offset;
        self.length = length;
        self
    }
}

/// A labeled span for additional context in violations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    /// Location of the label.
    pub location: Location,
    /// Message for this label.
    pub message: String,
}

impl Label {
    /// Creates a new label.
    #[must_use]
    pub fn new(location: Location, message: impl Into<String>) -> Self {
        Self {
            location,
            message: message.into(),
        }
    }
}

/// A suggested fix for a violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Human-readable description of the fix.
    pub message: String,
    /// Optional automatic replacement.
    pub replacement: Option<Replacement>,
}

impl Suggestion {
    /// Creates a new suggestion without automatic fix.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            replacement: None,
        }
    }

    /// Creates a new suggestion with automatic fix.
    #[must_use]
    pub fn with_fix(message: impl Into<String>, replacement: Replacement) -> Self {
        Self {
            message: message.into(),
            replacement: Some(replacement),
        }
    }
}

/// An automatic code replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replacement {
    /// Location to replace.
    pub location: Location,
    /// New text to insert.
    pub new_text: String,
}

impl Replacement {
    /// Creates a new replacement.
    #[must_use]
    pub fn new(location: Location, new_text: impl Into<String>) -> Self {
        Self {
            location,
            new_text: new_text.into(),
        }
    }
}

/// A lint violation found during analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// Rule code (e.g., "AL001").
    pub code: String,
    /// Rule name (e.g., "no-unwrap-expect").
    pub rule: String,
    /// Severity of this violation.
    pub severity: Severity,
    /// Primary location of the violation.
    pub location: Location,
    /// Human-readable message.
    pub message: String,
    /// Optional suggestion for fixing.
    pub suggestion: Option<Suggestion>,
    /// Additional labels for context.
    pub labels: Vec<Label>,
    /// Reference to design document (e.g., "ARCHITECTURE.md L85").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_ref: Option<String>,
}

impl Violation {
    /// Creates a new violation.
    #[must_use]
    pub fn new(
        code: impl Into<String>,
        rule: impl Into<String>,
        severity: Severity,
        location: Location,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            rule: rule.into(),
            severity,
            location,
            message: message.into(),
            suggestion: None,
            labels: Vec::new(),
            doc_ref: None,
        }
    }

    /// Adds a design document reference to this violation.
    #[must_use]
    pub fn with_doc_ref(mut self, doc_ref: impl Into<String>) -> Self {
        self.doc_ref = Some(doc_ref.into());
        self
    }

    /// Adds a suggestion to this violation.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestion = Some(suggestion);
        self
    }

    /// Adds a label to this violation.
    #[must_use]
    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    /// Formats the violation for terminal output.
    #[must_use]
    pub fn format(&self) -> String {
        use std::fmt::Write;
        let mut output = format!(
            "{} {} at {}:{}:{}\n",
            self.code,
            self.rule,
            self.location.file.display(),
            self.location.line,
            self.location.column,
        );
        let _ = writeln!(output, "  {}: {}", self.severity, self.message);
        if let Some(suggestion) = &self.suggestion {
            let _ = writeln!(output, "  = help: {}", suggestion.message);
        }
        if let Some(doc_ref) = &self.doc_ref {
            let _ = writeln!(output, "  = see: {doc_ref}");
        }
        output
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}: {} [{}] {}",
            self.location.file.display(),
            self.location.line,
            self.location.column,
            self.severity,
            self.code,
            self.message
        )?;
        if let Some(doc_ref) = &self.doc_ref {
            write!(f, " (see: {doc_ref})")?;
        }
        Ok(())
    }
}

/// Converts a Violation to a miette Diagnostic for rich error display.
#[allow(dead_code)] // Public API for miette integration
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("{message}")]
pub struct ViolationDiagnostic {
    message: String,
    #[help]
    help: Option<String>,
    #[label("{label_message}")]
    span: SourceSpan,
    label_message: String,
}

impl From<&Violation> for ViolationDiagnostic {
    fn from(v: &Violation) -> Self {
        Self {
            message: format!("[{}] {}", v.code, v.message),
            help: v.suggestion.as_ref().map(|s| s.message.clone()),
            span: SourceSpan::from((v.location.offset, v.location.length)),
            label_message: v.rule.clone(),
        }
    }
}

/// Result of running lint analysis.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LintResult {
    /// All violations found.
    pub violations: Vec<Violation>,
    /// Number of files checked.
    pub files_checked: usize,
}

impl LintResult {
    /// Creates a new empty result.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if there are any errors.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.severity == Severity::Error)
    }

    /// Returns true if there are any warnings or errors.
    #[must_use]
    pub fn has_warnings(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.severity >= Severity::Warning)
    }

    /// Returns violations filtered by severity.
    #[must_use]
    pub fn by_severity(&self, severity: Severity) -> Vec<&Violation> {
        self.violations
            .iter()
            .filter(|v| v.severity == severity)
            .collect()
    }

    /// Counts violations by severity.
    #[must_use]
    pub fn count_by_severity(&self) -> (usize, usize, usize) {
        let errors = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();
        let warnings = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Warning)
            .count();
        let infos = self
            .violations
            .iter()
            .filter(|v| v.severity == Severity::Info)
            .count();
        (errors, warnings, infos)
    }

    /// Prints a summary report to stdout.
    pub fn print_report(&self) {
        let (errors, warnings, infos) = self.count_by_severity();

        for violation in &self.violations {
            println!("{}", violation.format());
        }

        println!(
            "\nFound {} error(s), {} warning(s), {} info(s) in {} file(s)",
            errors, warnings, infos, self.files_checked
        );
    }

    /// Formats violations as a test failure report.
    ///
    /// Produces a human-readable multi-line report suitable for `panic!()` messages
    /// in `cargo test` integration.
    #[must_use]
    pub fn format_test_report(&self, fail_on: Severity) -> String {
        use std::fmt::Write;

        let failing: Vec<&Violation> = self
            .violations
            .iter()
            .filter(|v| v.severity >= fail_on)
            .collect();

        let mut report = String::new();
        let _ = writeln!(
            report,
            "\n=== arch-lint: {} violation(s) ===\n",
            failing.len()
        );

        for v in &failing {
            let _ = writeln!(
                report,
                "{} [{}] at {}:{}:{}",
                v.rule,
                v.code,
                v.location.file.display(),
                v.location.line,
                v.location.column,
            );
            let _ = writeln!(report, "  {}: {}", v.severity, v.message);
            if let Some(suggestion) = &v.suggestion {
                let _ = writeln!(report, "  = help: {}", suggestion.message);
            }
            if let Some(doc_ref) = &v.doc_ref {
                let _ = writeln!(report, "  = see: {doc_ref}");
            }
            let _ = writeln!(report);
        }

        let (errors, warnings, infos) = self.count_by_severity();
        let _ = writeln!(
            report,
            "Total: {} error(s), {} warning(s), {} info(s) in {} file(s)",
            errors, warnings, infos, self.files_checked
        );

        report
    }

    /// Checks if any violations meet or exceed the given severity threshold.
    #[must_use]
    pub fn has_violations_at(&self, severity: Severity) -> bool {
        self.violations.iter().any(|v| v.severity >= severity)
    }

    /// Adds violations from another result.
    pub fn extend(&mut self, other: Self) {
        self.violations.extend(other.violations);
        self.files_checked += other.files_checked;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_violation(severity: Severity) -> Violation {
        Violation::new(
            "AL001",
            "no-unwrap-expect",
            severity,
            Location::new(PathBuf::from("src/lib.rs"), 42, 10),
            ".unwrap() detected",
        )
    }

    // --- Violation doc_ref tests ---

    #[test]
    fn violation_new_has_no_doc_ref() {
        let v = make_violation(Severity::Error);
        assert!(v.doc_ref.is_none());
    }

    #[test]
    fn violation_with_doc_ref_sets_value() {
        let v = make_violation(Severity::Error).with_doc_ref("ARCHITECTURE.md L85");
        assert_eq!(v.doc_ref.as_deref(), Some("ARCHITECTURE.md L85"));
    }

    #[test]
    fn violation_format_includes_doc_ref() {
        let v = make_violation(Severity::Error).with_doc_ref("ARCHITECTURE.md L85");
        let formatted = v.format();
        assert!(formatted.contains("= see: ARCHITECTURE.md L85"));
    }

    #[test]
    fn violation_format_omits_doc_ref_when_none() {
        let v = make_violation(Severity::Error);
        let formatted = v.format();
        assert!(!formatted.contains("see:"));
    }

    #[test]
    fn violation_display_includes_doc_ref() {
        let v = make_violation(Severity::Error).with_doc_ref("DDD.md L33");
        let display = format!("{v}");
        assert!(display.contains("(see: DDD.md L33)"));
    }

    #[test]
    fn violation_display_omits_doc_ref_when_none() {
        let v = make_violation(Severity::Error);
        let display = format!("{v}");
        assert!(!display.contains("see:"));
    }

    // --- LintResult tests ---

    #[test]
    fn has_violations_at_error_only() {
        let mut result = LintResult::new();
        result.violations.push(make_violation(Severity::Warning));
        assert!(!result.has_violations_at(Severity::Error));
        assert!(result.has_violations_at(Severity::Warning));
    }

    #[test]
    fn format_test_report_filters_by_severity() {
        let mut result = LintResult::new();
        result.files_checked = 5;
        result.violations.push(make_violation(Severity::Warning));
        result.violations.push(make_violation(Severity::Error));

        let report = result.format_test_report(Severity::Error);
        // Only 1 error-level violation should appear in report header
        assert!(report.contains("1 violation(s)"));
        assert!(report.contains("1 error(s)"));
        assert!(report.contains("1 warning(s)"));
    }

    #[test]
    fn format_test_report_includes_doc_ref() {
        let mut result = LintResult::new();
        result.files_checked = 1;
        result
            .violations
            .push(make_violation(Severity::Error).with_doc_ref("ARCH.md L10"));

        let report = result.format_test_report(Severity::Error);
        assert!(report.contains("= see: ARCH.md L10"));
    }

    #[test]
    fn format_test_report_includes_suggestion() {
        let mut result = LintResult::new();
        result.files_checked = 1;
        result.violations.push(
            make_violation(Severity::Error).with_suggestion(Suggestion::new("Use ? operator")),
        );

        let report = result.format_test_report(Severity::Error);
        assert!(report.contains("= help: Use ? operator"));
    }
}
