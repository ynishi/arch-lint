//! Tree-sitter engine check command.
//!
//! Runs architecture layer enforcement using Tree-sitter
//! when `[[layers]]` is present in config.

use anyhow::{Context, Result};
use arch_lint_core::LintResult;
use arch_lint_ts::{ArchConfig, ArchRuleEngine, KotlinExtractor, LanguageExtractor};
use std::path::{Path, PathBuf};

use crate::OutputFormat;

/// Runs the tree-sitter check command.
pub fn run(
    path: &Path,
    format: OutputFormat,
    source: &crate::config_resolver::ConfigSource,
) -> Result<()> {
    let config = load_ts_config(source)?;
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

        let Some(extractor) = extractors
            .iter()
            .find(|e| e.extensions().contains(&ext.as_str()))
        else {
            continue;
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

    super::output::print(&result, format)?;

    if result.has_errors() {
        std::process::exit(1);
    }

    Ok(())
}

fn load_ts_config(source: &crate::config_resolver::ConfigSource) -> Result<ArchConfig> {
    match source {
        crate::config_resolver::ConfigSource::Default => {
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
