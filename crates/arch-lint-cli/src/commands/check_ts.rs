//! Tree-sitter engine check command.
//!
//! Runs architecture layer enforcement using Tree-sitter
//! when `[[layers]]` is present in config.

use anyhow::{Context, Result};
use arch_lint_core::{LintResult, Severity};
use arch_lint_ts::{ArchConfig, ArchRuleEngine, KotlinExtractor, LanguageExtractor};
use std::path::{Path, PathBuf};

use crate::OutputFormat;

/// Runs the tree-sitter check command.
pub fn run(path: &Path, format: OutputFormat, config_path: Option<PathBuf>) -> Result<()> {
    let config = load_ts_config(path, config_path.as_deref())?;
    config.validate().context("Config validation failed")?;

    let engine = ArchRuleEngine::new(config.clone());
    let extractors: Vec<Box<dyn LanguageExtractor>> = vec![Box::new(KotlinExtractor::new())];

    let root = if config.root.is_absolute() {
        config.root.clone()
    } else {
        path.join(&config.root)
    };

    let files = discover_files(&root, &config.exclude, &extractors)?;

    tracing::info!("Analyzing {} files with tree-sitter engine", files.len());

    let mut result = LintResult::new();

    for file_path in &files {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();

        let extractor = match extractors
            .iter()
            .find(|e| e.extensions().contains(&ext.as_str()))
        {
            Some(e) => e,
            None => continue,
        };

        let source = std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let rel = file_path
            .strip_prefix(&root)
            .unwrap_or(file_path)
            .to_path_buf();

        let mut analysis = extractor.analyze(&source);
        analysis.file_path = rel;

        let violations = engine.check(&analysis);
        result.violations.extend(violations);
        result.files_checked += 1;
    }

    // Sort by file, then line
    result.violations.sort_by(|a, b| {
        a.location
            .file
            .cmp(&b.location.file)
            .then(a.location.line.cmp(&b.location.line))
    });

    match format {
        OutputFormat::Text => print_text(&result),
        OutputFormat::Json => print_json(&result)?,
        OutputFormat::Compact => print_compact(&result),
    }

    if result.has_errors() {
        std::process::exit(1);
    }

    Ok(())
}

fn load_ts_config(path: &Path, config_path: Option<&Path>) -> Result<ArchConfig> {
    let candidates = if let Some(cp) = config_path {
        vec![cp.to_path_buf()]
    } else {
        vec![path.join("arch-lint.toml"), path.join(".arch-lint.toml")]
    };

    for candidate in &candidates {
        if candidate.exists() {
            return ArchConfig::from_file(candidate)
                .with_context(|| format!("Failed to load {}", candidate.display()));
        }
    }

    anyhow::bail!("No arch-lint.toml found. Run `arch-lint init --ts` to create one.")
}

fn discover_files(
    root: &Path,
    exclude: &[String],
    extractors: &[Box<dyn LanguageExtractor>],
) -> Result<Vec<PathBuf>> {
    let supported_exts: Vec<&str> = extractors
        .iter()
        .flat_map(|e| e.extensions().iter().copied())
        .collect();

    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(false).git_ignore(true);

    let mut files = Vec::new();
    for entry in builder.build() {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();

        if !supported_exts.contains(&ext.as_str()) {
            continue;
        }

        let rel_str = path.strip_prefix(root).unwrap_or(path).to_string_lossy();

        let excluded = exclude.iter().any(|pattern| {
            let clean = pattern.replace("**/", "").replace("/**", "");
            !clean.is_empty() && rel_str.contains(&clean)
        });

        if !excluded {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

// --- Output (reuses same format as syn check) ---

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
