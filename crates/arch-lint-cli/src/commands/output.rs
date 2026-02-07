//! Shared output formatting for lint results.

use anyhow::Result;
use arch_lint_core::{LintResult, Severity};

use crate::OutputFormat;

/// Print lint results in the specified format.
pub fn print(result: &LintResult, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => print_text(result),
        OutputFormat::Json => return print_json(result),
        OutputFormat::Compact => print_compact(result),
    }
    Ok(())
}

fn print_text(result: &LintResult) {
    let (errors, warnings, infos) = result.count_by_severity();

    for violation in &result.violations {
        let severity_indicator = match violation.severity {
            Severity::Error => "\x1b[31merror\x1b[0m",
            Severity::Warning => "\x1b[33mwarning\x1b[0m",
            Severity::Info => "\x1b[34minfo\x1b[0m",
        };

        println!(
            "{} {} at {}:{}:{}",
            violation.code,
            violation.rule,
            violation.location.file.display(),
            violation.location.line,
            violation.location.column,
        );
        println!("  {}: {}", severity_indicator, violation.message);
        if let Some(suggestion) = &violation.suggestion {
            println!("  = help: {}", suggestion.message);
        }
        println!();
    }

    let summary_color = if errors > 0 {
        "\x1b[31m"
    } else if warnings > 0 {
        "\x1b[33m"
    } else {
        "\x1b[32m"
    };

    println!(
        "{}Found {} error(s), {} warning(s), {} info(s) in {} file(s)\x1b[0m",
        summary_color, errors, warnings, infos, result.files_checked
    );
}

fn print_json(result: &LintResult) -> Result<()> {
    let json = serde_json::to_string_pretty(result)?;
    println!("{json}");
    Ok(())
}

fn print_compact(result: &LintResult) {
    for violation in &result.violations {
        println!(
            "{}:{}:{}: {} [{}] {}",
            violation.location.file.display(),
            violation.location.line,
            violation.location.column,
            violation.severity,
            violation.code,
            violation.message,
        );
    }
}
