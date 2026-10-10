//! Tree-sitter engine check command.
//!
//! Runs architecture layer enforcement using Tree-sitter
//! when `[[layers]]` is present in config.

use anyhow::{Context, Result};
use arch_lint::session::ConfigSource;
use arch_lint_ts::{ArchConfig, TsSession};
use std::path::Path;

use super::Outcome;
use crate::OutputFormat;

/// Runs the tree-sitter check command.
///
/// Returns [`Outcome::Violations`] when any error-severity violation is
/// found; `main` turns that into exit status 1.
///
/// # Errors
///
/// Returns an error if no config file was found, if it cannot be loaded or
/// validated, or if analysis fails. `main` maps that to exit status 2.
pub fn run(path: &Path, format: OutputFormat, source: &ConfigSource) -> Result<Outcome> {
    let config = load_ts_config(source)?;

    let result = TsSession::new(config, path).run()?;

    super::print_result(&result, format)?;

    if result.has_errors() {
        Ok(Outcome::Violations)
    } else {
        Ok(Outcome::Clean)
    }
}

fn load_ts_config(source: &ConfigSource) -> Result<ArchConfig> {
    match source {
        ConfigSource::Default => {
            anyhow::bail!("No arch-lint.toml found. Run `arch-lint init --ts` to create one.")
        }
        other => {
            let p = other.path().context("resolved config has no path")?;
            if source.is_global() {
                tracing::info!("Using global config: {}", p.display());
            }
            ArchConfig::from_file(p).with_context(|| format!("Failed to load {}", p.display()))
        }
    }
}
