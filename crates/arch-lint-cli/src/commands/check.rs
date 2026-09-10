//! Check command implementation.

use anyhow::Result;
use arch_lint::session::{ConfigSource, Session};
use std::path::Path;

use crate::OutputFormat;

/// Runs the check command.
///
/// Rule selection mirrors `arch_lint::check!()`:
/// `--rules` > `preset` in config > `recommended`. Declarative rules
/// (`[[restrict-use]]`, `[[require-use]]`, `[[deny-scope-dep]]`) from the
/// config file are always added. The process exits with status 1 when any
/// violation at or above `fail_on` (config, default `error`) is found.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded, a `--rules` name is
/// unknown, or the analysis fails.
pub fn run(
    path: &Path,
    format: OutputFormat,
    rules_filter: Option<String>,
    exclude: Vec<String>,
    source: &ConfigSource,
) -> Result<()> {
    let mut session = Session::from_source(path, source.clone())?;

    for pattern in exclude {
        session = session.with_exclude(pattern);
    }

    if let Some(filter) = rules_filter {
        let names: Vec<&str> = filter.split(',').map(str::trim).collect();
        session = session.with_rule_filter(&names)?;
    }

    let fail_on = session.fail_on();
    let result = session.run()?;

    super::print_result(&result, format)?;

    if result.has_violations_at(fail_on) {
        std::process::exit(1);
    }

    Ok(())
}
