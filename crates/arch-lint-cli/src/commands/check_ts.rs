//! Tree-sitter engine check command.
//!
//! Runs architecture layer enforcement using Tree-sitter
//! when `[[layers]]` is present in config.

use anyhow::{Context, Result};
use arch_lint::session::ConfigSource;
use arch_lint_ts::{ArchConfig, TsSession};
use std::path::Path;

use crate::OutputFormat;

/// Runs the tree-sitter check command.
///
/// # Errors
///
/// Returns an error if no config file was found, if it cannot be loaded or
/// validated, or if analysis fails.
pub fn run(path: &Path, format: OutputFormat, source: &ConfigSource) -> Result<()> {
    let config = load_ts_config(source)?;

    let result = TsSession::new(config, path).run()?;

    super::print_result(&result, format)?;

    if result.has_errors() {
        std::process::exit(1);
    }

    Ok(())
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
