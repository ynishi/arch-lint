//! Check command implementation.

use anyhow::{Context, Result};
use arch_lint_core::{Analyzer, Config, LintResult, Severity};
use arch_lint_rules::{
    recommended_rules, HandlerComplexity, NoErrorSwallowing, NoSyncIo, NoUnwrapExpect,
    RequireThiserror, RequireTracing, TracingEnvInit,
};
use std::path::Path;

use crate::OutputFormat;

/// Runs the check command.
pub fn run(
    path: &Path,
    format: OutputFormat,
    rules_filter: Option<String>,
    exclude: Vec<String>,
    source: &crate::config_resolver::ConfigSource,
) -> Result<()> {
    let config = match source {
        crate::config_resolver::ConfigSource::Default => Config::default(),
        other => {
            // Invariant: non-Default variants always have a path
            let p = other.path().context("resolved config has no path")?;
            if source.is_global() {
                tracing::info!("Using global config: {}", p.display());
            }
            Config::from_file(p)
                .with_context(|| format!("Failed to load config: {}", p.display()))?
        }
    };

    // Build analyzer
    let mut builder = Analyzer::builder().root(path).config(config);

    // Add exclude patterns
    for pattern in exclude {
        builder = builder.exclude(pattern);
    }

    // Add rules based on filter
    let rules_to_add = if let Some(filter) = rules_filter {
        let rule_names: Vec<&str> = filter.split(',').map(str::trim).collect();
        filter_rules(&rule_names)
    } else {
        recommended_rules()
    };

    for rule in rules_to_add {
        builder = builder.rule_box(rule);
    }

    let analyzer = builder.build().context("Failed to build analyzer")?;

    tracing::info!("Analyzing {:?} with {} rules", path, analyzer.rule_count());

    let result = analyzer.analyze().context("Analysis failed")?;

    // Output results
    match format {
        OutputFormat::Text => print_text(&result),
        OutputFormat::Json => print_json(&result)?,
        OutputFormat::Compact => print_compact(&result),
    }

    // Exit with error code if there are errors
    if result.has_errors() {
        std::process::exit(1);
    }

    Ok(())
}

fn filter_rules(names: &[&str]) -> Vec<arch_lint_core::RuleBox> {
    let mut rules: Vec<arch_lint_core::RuleBox> = Vec::new();

    for name in names {
        match *name {
            "no-unwrap-expect" | "AL001" => rules.push(Box::new(NoUnwrapExpect::new())),
            "no-sync-io" | "AL002" => rules.push(Box::new(NoSyncIo::new())),
            "no-error-swallowing" | "AL003" => rules.push(Box::new(NoErrorSwallowing::new())),
            "handler-complexity" | "AL004" => rules.push(Box::new(HandlerComplexity::new())),
            "require-thiserror" | "AL005" => rules.push(Box::new(RequireThiserror::new())),
            "require-tracing" | "AL006" => rules.push(Box::new(RequireTracing::new())),
            "tracing-env-init" | "AL007" => rules.push(Box::new(TracingEnvInit::new())),
            _ => tracing::warn!("Unknown rule: {}", name),
        }
    }

    rules
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
