//! CLI command implementations.

use anyhow::Result;
use arch_lint::report::{render, Format};
use arch_lint_core::LintResult;
use std::io::{IsTerminal, Write};

use crate::OutputFormat;

pub mod check;
pub mod check_ts;
pub mod graph;
pub mod init;
pub mod init_ts;
pub mod list_rules;
pub mod schema;

/// What a command found, mapped to the process exit code in `main`.
///
/// A command never calls `std::process::exit` itself: it returns this value
/// (or an `Err`, for "could not run"), so the mapping to `0` / `1` / `2`
/// lives in one place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The command ran and found nothing to fail on (exit `0`).
    Clean,
    /// The command ran and found violations at or above the fail threshold
    /// (exit `1`).
    Violations,
}

/// Writes a lint result to stdout in the requested format.
///
/// ANSI colors are enabled only when stdout is a terminal, so piped and
/// redirected output stays plain.
fn print_result(result: &LintResult, format: OutputFormat) -> Result<()> {
    let format = match format {
        OutputFormat::Text => Format::Text {
            color: std::io::stdout().is_terminal(),
        },
        OutputFormat::Json => Format::Json,
        OutputFormat::Compact => Format::Compact,
        OutputFormat::Sarif => Format::Sarif,
    };

    let rendered = render(result, format)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(rendered.as_bytes())?;
    stdout.flush()?;
    Ok(())
}
