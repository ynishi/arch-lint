//! arch-lint CLI tool.
//!
//! Usage:
//! ```bash
//! arch-lint check [OPTIONS] [PATH]
//! arch-lint list-rules [--json]
//! arch-lint schema [--out <PATH>]
//! arch-lint init
//! ```

use anyhow::Result;
use arch_lint::session::ConfigSource;
use clap::{Parser, Subcommand};
use std::io::IsTerminal;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod commands;

/// Architecture linter for Rust projects and cross-language layer enforcement
#[derive(Parser)]
#[command(name = "arch-lint")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Path to configuration file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run lint checks
    Check {
        /// Path to analyze (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output format
        #[arg(short, long, default_value = "text")]
        format: OutputFormat,

        /// Only run specific rules (comma-separated)
        #[arg(long)]
        rules: Option<String>,

        /// Exclude patterns (can be specified multiple times)
        #[arg(short, long)]
        exclude: Vec<String>,

        /// Engine hint: "syn" (Rust AST) or "ts" (Tree-sitter).
        /// Auto-detected from config if omitted.
        #[arg(long)]
        engine: Option<EngineHint>,
    },

    /// List available rules
    ListRules {
        /// Print the rule registry, option schemas included, as JSON
        #[arg(long)]
        json: bool,
    },

    /// Print the JSON Schema of arch-lint.toml
    Schema {
        /// Write the schema to this file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Initialize configuration file
    Init {
        /// Overwrite existing config
        #[arg(long)]
        force: bool,

        /// Generate tree-sitter config (with [[layers]] for Kotlin etc.)
        #[arg(long)]
        ts: bool,
    },
}

/// Output format for lint results.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output.
    #[default]
    Text,
    /// JSON output.
    Json,
    /// One-line-per-violation compact format.
    Compact,
}

/// Engine selection hint.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum EngineHint {
    /// syn-based Rust AST analysis (existing rules)
    Syn,
    /// Tree-sitter based cross-language analysis (layer enforcement)
    Ts,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(std::io::stdout().is_terminal())
        .init();

    match cli.command {
        Commands::Check {
            path,
            format,
            rules,
            exclude,
            engine,
        } => {
            let source = ConfigSource::resolve(&path, cli.config.as_deref());
            let engine = engine.unwrap_or_else(|| detect_engine(&source));
            match engine {
                EngineHint::Syn => commands::check::run(&path, format, rules, exclude, &source),
                EngineHint::Ts => commands::check_ts::run(&path, format, &source),
            }
        }
        Commands::ListRules { json } => commands::list_rules::run(json),
        Commands::Schema { out } => commands::schema::run(out.as_deref()),
        Commands::Init { force, ts } => {
            if ts {
                commands::init_ts::run(force)
            } else {
                commands::init::run(force)
            }
        }
    }
}

/// Auto-detect engine from config: if `[[layers]]` present → ts, else → syn.
fn detect_engine(source: &ConfigSource) -> EngineHint {
    if let Some(p) = source.path() {
        if let Ok(content) = std::fs::read_to_string(p) {
            if has_layers_table(&content) {
                tracing::info!(
                    "Detected [[layers]] in {}, using tree-sitter engine",
                    p.display()
                );
                return EngineHint::Ts;
            }
        }
    }

    EngineHint::Syn
}

/// Returns `true` if the TOML content defines a non-empty `[[layers]]` array.
///
/// Parses the document instead of substring-matching so that commented-out
/// or quoted `[[layers]]` text does not switch engines.
fn has_layers_table(content: &str) -> bool {
    content
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| {
            t.get("layers")
                .and_then(toml::Value::as_array)
                .map(|a| !a.is_empty())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::has_layers_table;

    #[test]
    fn detects_real_layers_table() {
        assert!(has_layers_table(
            "[[layers]]\nname = \"domain\"\npackages = [\"com.example.domain\"]\n"
        ));
    }

    #[test]
    fn ignores_commented_or_quoted_layers() {
        assert!(!has_layers_table("# [[layers]]\npreset = \"strict\"\n"));
        assert!(!has_layers_table("note = \"[[layers]]\"\n"));
        assert!(!has_layers_table("preset = \"strict\"\n"));
    }
}
