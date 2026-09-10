//! CLI command implementations.

use anyhow::Result;
use arch_lint::report::{render, Format};
use arch_lint_core::LintResult;
use std::io::{IsTerminal, Write};

use crate::OutputFormat;

pub mod check;
pub mod check_ts;
pub mod init;
pub mod init_ts;
pub mod list_rules;

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
    };

    let rendered = render(result, format)?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(rendered.as_bytes())?;
    stdout.flush()?;
    Ok(())
}
