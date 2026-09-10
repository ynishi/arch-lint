//! Check command implementation.

use anyhow::{bail, Context, Result};
use arch_lint_core::{Analyzer, Config, RuleBox, Severity};
use arch_lint_rules::{rule_from_config, Preset};
use std::path::Path;

use crate::OutputFormat;

/// Runs the check command.
///
/// Rule selection mirrors `arch_lint::check!()`:
/// `--rules` > `preset` in config > `recommended`. Declarative rules
/// (`[[restrict-use]]`, `[[require-use]]`, `[[deny-scope-dep]]`) from the
/// config file are always added. The process exits with status 1 when any
/// violation at or above `fail_on` (config, default `error`) is found.
pub fn run(
    path: &Path,
    format: OutputFormat,
    rules_filter: Option<String>,
    exclude: Vec<String>,
    source: &crate::config_resolver::ConfigSource,
) -> Result<()> {
    let (config, declarative_rules) = load_config(source)?;

    let preset = resolve_preset(&config)?;
    let fail_on = resolve_fail_on(&config)?;

    // Build analyzer
    let mut builder = Analyzer::builder().root(path).config(config.clone());

    // Add exclude patterns
    for pattern in exclude {
        builder = builder.exclude(pattern);
    }

    // Add rules based on filter
    let rules_to_add = if let Some(filter) = rules_filter {
        let rule_names: Vec<&str> = filter.split(',').map(str::trim).collect();
        filter_rules(&rule_names, &config)?
    } else {
        preset.rules_with_config(&config)
    };

    for rule in rules_to_add {
        builder = builder.rule_box(rule);
    }
    for rule in declarative_rules {
        builder = builder.rule_box(rule);
    }

    let analyzer = builder.build().context("Failed to build analyzer")?;

    tracing::info!("Analyzing {:?} with {} rules", path, analyzer.rule_count());

    let result = analyzer.analyze().context("Analysis failed")?;

    // Output results
    super::output::print(&result, format)?;

    // Exit with error code if there are violations at or above fail_on
    if result.has_violations_at(fail_on) {
        std::process::exit(1);
    }

    Ok(())
}

/// Loads `Config` plus declarative rules from the resolved config source.
fn load_config(source: &crate::config_resolver::ConfigSource) -> Result<(Config, Vec<RuleBox>)> {
    match source {
        crate::config_resolver::ConfigSource::Default => Ok((Config::default(), Vec::new())),
        other => {
            // Invariant: non-Default variants always have a path
            let p = other.path().context("resolved config has no path")?;
            if source.is_global() {
                tracing::info!("Using global config: {}", p.display());
            }
            let content = std::fs::read_to_string(p)
                .with_context(|| format!("Failed to read config: {}", p.display()))?;
            let config = Config::parse(&content)
                .with_context(|| format!("Failed to load config: {}", p.display()))?;
            let declarative = arch_lint_core::declarative::load_rules_from_toml(&content)
                .with_context(|| format!("Invalid declarative rules in {}", p.display()))?;
            Ok((config, declarative))
        }
    }
}

fn resolve_preset(config: &Config) -> Result<Preset> {
    config
        .preset
        .as_deref()
        .unwrap_or("recommended")
        .parse()
        .map_err(anyhow::Error::from)
}

fn resolve_fail_on(config: &Config) -> Result<Severity> {
    config
        .fail_on
        .as_deref()
        .unwrap_or("error")
        .parse()
        .map_err(anyhow::Error::from)
}

fn filter_rules(names: &[&str], config: &Config) -> Result<Vec<RuleBox>> {
    let mut rules = Vec::with_capacity(names.len());
    for name in names {
        match rule_from_config(name, config) {
            Some(rule) => rules.push(rule),
            None => bail!("Unknown rule `{name}`. Run `arch-lint list-rules` to see valid names."),
        }
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_rules_accepts_names_and_codes() {
        let config = Config::default();
        let rules = filter_rules(&["AL001", "require-doc-comments"], &config).expect("valid");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].code(), "AL012");
    }

    #[test]
    fn filter_rules_rejects_unknown() {
        let config = Config::default();
        let err = match filter_rules(&["AL001", "bogus"], &config) {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn preset_and_fail_on_come_from_config() {
        let config = Config::parse("preset = \"strict\"\nfail_on = \"warning\"").expect("parse");
        assert_eq!(resolve_preset(&config).expect("preset"), Preset::Strict);
        assert_eq!(
            resolve_fail_on(&config).expect("fail_on"),
            Severity::Warning
        );

        let bad = Config::parse("preset = \"nope\"").expect("parse");
        assert!(resolve_preset(&bad).is_err());
    }
}
