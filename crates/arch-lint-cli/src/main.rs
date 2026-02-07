//! arch-lint CLI tool.
//!
//! Usage:
//! ```bash
//! arch-lint check [OPTIONS] [PATH]
//! arch-lint list-rules
//! arch-lint init
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod commands;
mod config_resolver;

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
    ListRules,

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
        .init();

    match cli.command {
        Commands::Check {
            path,
            format,
            rules,
            exclude,
            engine,
        } => {
            let engine = engine.unwrap_or_else(|| detect_engine(&path, cli.config.as_deref()));
            match engine {
                EngineHint::Syn => {
                    commands::check::run(&path, format, rules, exclude, cli.config.as_deref())
                }
                EngineHint::Ts => commands::check_ts::run(&path, format, cli.config.as_deref()),
            }
        }
        Commands::ListRules => {
            commands::list_rules::run();
            Ok(())
        }
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
fn detect_engine(path: &std::path::Path, config_path: Option<&std::path::Path>) -> EngineHint {
    let source = config_resolver::resolve(path, config_path);

    if let Some(p) = source.path() {
        if let Ok(content) = std::fs::read_to_string(p) {
            if content.contains("[[layers]]") {
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
