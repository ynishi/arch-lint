//! arch-lint CLI tool.
//!
//! Usage:
//! ```bash
//! arch-lint check [OPTIONS] [PATH]
//! arch-lint list-rules
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod commands;

/// Architecture linter for Rust projects
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
    },

    /// List available rules
    ListRules,

    /// Initialize configuration file
    Init {
        /// Overwrite existing config
        #[arg(long)]
        force: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    Compact,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
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
        } => commands::check::run(&path, format, rules, exclude, cli.config),
        Commands::ListRules => {
            commands::list_rules::run();
            Ok(())
        }
        Commands::Init { force } => commands::init::run(force),
    }
}
