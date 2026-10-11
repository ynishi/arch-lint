//! Check command implementation.

use anyhow::{bail, Context, Result};
use arch_lint::baseline::Baseline;
use arch_lint::session::{ConfigSource, Session};
use std::path::Path;

use super::Outcome;
use crate::OutputFormat;

/// Runs the check command.
///
/// Rule selection mirrors `arch_lint::check!()`:
/// `--rules` > `preset` in config > `recommended`. Declarative rules
/// (`[[restrict-use]]`, `[[require-use]]`, `[[deny-scope-dep]]`) from the
/// config file are always added. Returns [`Outcome::Violations`] when any
/// violation at or above `fail_on` (config, default `error`) is found;
/// `main` turns that into exit status 1.
///
/// `baseline` and `update_baseline` implement the `--baseline` /
/// `--update-baseline` flags:
///
/// - Neither: ordinary behaviour, as above.
/// - `baseline` only: the file must exist (a missing one is an error, mapped
///   to exit `2`); only violations beyond it are reported, a
///   `baseline: N suppressed, M stale` line (plus one `stale entry` line per
///   stale entry) is printed to stderr, and the exit status comes from the
///   new violations only.
/// - `baseline` + `update_baseline`: runs the full analysis, prints the full
///   report, (re)writes the baseline file from it, and always returns
///   [`Outcome::Clean`] — this is an authoring step, not a check.
///
/// # Errors
///
/// Returns an error if the configuration cannot be loaded, a `--rules` name is
/// unknown, the analysis fails, the baseline file cannot be read (without
/// `--update-baseline`) or written (with it).
#[allow(clippy::too_many_arguments)]
pub fn run(
    path: &Path,
    format: OutputFormat,
    rules_filter: Option<String>,
    exclude: Vec<String>,
    source: &ConfigSource,
    baseline: Option<&Path>,
    update_baseline: bool,
) -> Result<Outcome> {
    let mut session = Session::from_source(path, source.clone())?;

    for pattern in exclude {
        session = session.with_exclude(pattern);
    }

    if let Some(filter) = rules_filter {
        let names: Vec<&str> = filter.split(',').map(str::trim).collect();
        session = session.with_rule_filter(&names)?;
    }

    let fail_on = session.fail_on();

    let Some(baseline_path) = baseline else {
        let result = session.run()?;
        super::print_result(&result, format)?;
        return Ok(if result.has_violations_at(fail_on) {
            Outcome::Violations
        } else {
            Outcome::Clean
        });
    };

    if update_baseline {
        let result = session.run()?;
        super::print_result(&result, format)?;
        let written = Baseline::from_result(&result);
        written
            .save(baseline_path)
            .with_context(|| format!("Failed to write baseline {}", baseline_path.display()))?;
        let count = written.entries().len();
        eprintln!(
            "baseline: wrote {count} {} to {}",
            if count == 1 { "entry" } else { "entries" },
            baseline_path.display()
        );
        return Ok(Outcome::Clean);
    }

    if !baseline_path.exists() {
        bail!(
            "Baseline file {} does not exist. Run `arch-lint check --baseline {} \
             --update-baseline` to create it.",
            baseline_path.display(),
            baseline_path.display(),
        );
    }
    let baseline_data = Baseline::load(baseline_path)
        .with_context(|| format!("Failed to load baseline {}", baseline_path.display()))?;

    let applied = session.with_baseline(baseline_data).run_with_baseline()?;
    super::print_result(&applied.result, format)?;

    eprintln!(
        "baseline: {} suppressed, {} stale",
        applied.suppressed,
        applied.stale.len()
    );
    for entry in &applied.stale {
        eprintln!(
            "baseline: stale entry {} {} ({}) x{}",
            entry.code,
            entry.file.display(),
            entry.message,
            entry.count,
        );
    }

    if applied.result.has_violations_at(fail_on) {
        Ok(Outcome::Violations)
    } else {
        Ok(Outcome::Clean)
    }
}
