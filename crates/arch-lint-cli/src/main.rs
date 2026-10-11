//! arch-lint CLI tool.
//!
//! Installed as two binaries built from this same `main.rs`: `arch-lint`,
//! and `cargo-arch-lint` so the tool can also be run as the `cargo
//! arch-lint` subcommand. Cargo's external-subcommand convention invokes a
//! `cargo-<name>` binary found on `PATH` as `cargo <name> <args>` →
//! `cargo-<name> <name> <args>` — the subcommand name arrives as the first
//! argument, which [`parse_cli`] strips before handing the rest to clap.
//! Both binaries accept the same arguments and print the same output.
//!
//! Usage:
//! ```bash
//! cargo arch-lint check [OPTIONS] [PATH]   # or: arch-lint check [OPTIONS] [PATH]
//! arch-lint check --format sarif                         # SARIF 2.1.0 for code scanning
//! arch-lint check --baseline <PATH> --update-baseline   # (re)write a baseline
//! arch-lint check --baseline <PATH>                      # report only new violations
//! arch-lint list-rules [--json]
//! arch-lint schema [--out <PATH>]
//! arch-lint init
//! arch-lint graph [PATH] [--format dot|mermaid]
//! ```
//!
//! Exit status: `0` when the command ran and found nothing to fail on, `1`
//! when `check` found violations at or above `fail_on`, `2` when the command
//! could not run (configuration error, unknown rule, unreadable path, I/O
//! error). `check --exit-zero` reports violations but exits `0`.

use anyhow::Result;
use arch_lint::session::ConfigSource;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use commands::Outcome;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;

mod commands;

/// Exit-status table shown at the end of `arch-lint check --help`.
const CHECK_AFTER_HELP: &str = "\
Exit status:
  0  no violation at or above `fail_on` (or --exit-zero)
  1  violations at or above `fail_on` were found
  2  could not run: configuration error, unknown rule, unreadable path, I/O error";

/// Architecture linter for Rust projects and cross-language layer enforcement
#[derive(Parser)]
#[command(name = "arch-lint")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose (debug-level) logging on stderr.
    ///
    /// Without this flag only warnings and errors are logged. A non-empty
    /// `RUST_LOG` overrides `--verbose`; invalid directives are reported on
    /// stderr (`ignoring ...`) and skipped, and if none remain the default
    /// applies.
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
    #[command(after_help = CHECK_AFTER_HELP)]
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

        /// Exit 0 even when violations are found (errors still exit 2)
        #[arg(long)]
        exit_zero: bool,

        /// Baseline file of known violations (TOML, syn engine only).
        ///
        /// Without --update-baseline, the file must already exist: only
        /// violations beyond it are reported, and a
        /// `baseline: N suppressed, M stale` summary (plus one line per
        /// stale entry) is printed to stderr.
        #[arg(long)]
        baseline: Option<PathBuf>,

        /// (Requires --baseline) Authoring step: run the full analysis,
        /// print its full report, and (re)write --baseline from it. Always
        /// exits 0, regardless of violations found — this writes the
        /// baseline, it does not check it. Run it once when adopting
        /// arch-lint, and again whenever a baselined violation is fixed or
        /// a new one is intentionally added to the baseline.
        #[arg(long, requires = "baseline")]
        update_baseline: bool,
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

    /// Render the scope dependency graph (read-only, syn engine only)
    Graph {
        /// Path to analyze (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output format
        #[arg(long, default_value = "mermaid")]
        format: GraphFormatArg,
    },
}

/// Output format for the `graph` command.
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
pub enum GraphFormatArg {
    /// Mermaid flowchart text (`graph LR`), pasteable into a Markdown
    /// `mermaid` fenced code block.
    #[default]
    Mermaid,
    /// Graphviz DOT text, accepted by `dot -Tsvg`.
    Dot,
}

impl From<GraphFormatArg> for arch_lint::graph::GraphFormat {
    fn from(value: GraphFormatArg) -> Self {
        match value {
            GraphFormatArg::Mermaid => Self::Mermaid,
            GraphFormatArg::Dot => Self::Dot,
        }
    }
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
    /// SARIF 2.1.0 JSON, for `github/codeql-action/upload-sarif`.
    Sarif,
}

/// Engine selection hint.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum EngineHint {
    /// syn-based Rust AST analysis (existing rules)
    Syn,
    /// Tree-sitter based cross-language analysis (layer enforcement)
    Ts,
}

fn main() -> ExitCode {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        // Mirrors what `Cli::parse()` does internally: a `--help` / `--version`
        // request or a usage error is printed by clap and the process exits
        // (0 for the former, 2 for the latter) — `clap::Error::exit` never
        // returns.
        Err(err) => err.exit(),
    };
    match run(cli) {
        Ok(Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Violations) => ExitCode::from(1),
        Err(err) => {
            eprintln!("Error: {err:?}");
            ExitCode::from(2)
        }
    }
}

/// Parses `std::env::args_os()`, adapting usage/help text and the
/// cargo-subcommand argument-stripping rule to how the binary was invoked.
///
/// Detected from the executable's own file name (`argv[0]`), not from an
/// env var or a cargo-set marker: `cargo-arch-lint` strips exactly one
/// leading `arch-lint` argument (the subcommand name cargo passes), while
/// the plain `arch-lint` binary never does — so typing the word twice
/// (`cargo arch-lint arch-lint check`, or the plain `arch-lint arch-lint
/// check`) always leaves one copy for clap to reject as an unexpected
/// argument. Running `cargo-arch-lint` directly, without the `arch-lint`
/// word (not through cargo), is unaffected: nothing is stripped unless the
/// first argument is exactly `arch-lint`.
///
/// `--version` prints the same `arch-lint <version>` text either way: the
/// [`Cli`] command's `name` is pinned to `arch-lint`
/// (`#[command(name = "arch-lint")]`), so `--version` prints the same line
/// under both invocations; only `bin_name` — used in `Usage:` lines and
/// "unexpected argument" messages — is switched to `cargo arch-lint` so
/// those read naturally under the cargo-subcommand invocation.
fn parse_cli() -> std::result::Result<Cli, clap::Error> {
    let mut args: Vec<OsString> = std::env::args_os().collect();

    let is_cargo_subcommand = args
        .first()
        .map(Path::new)
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
        == Some("cargo-arch-lint");

    if is_cargo_subcommand && args.get(1).and_then(|a| a.to_str()) == Some("arch-lint") {
        args.remove(1);
    }

    let bin_name = if is_cargo_subcommand {
        "cargo arch-lint"
    } else {
        "arch-lint"
    };
    let command = Cli::command().bin_name(bin_name);
    let matches = command.try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

/// Dispatches the parsed command line.
///
/// `Ok` means the command ran; the [`Outcome`] says what it found. `Err`
/// means it could not run, and `main` reports that as exit status 2.
fn run(cli: Cli) -> Result<Outcome> {
    // Logs are diagnostics, not the report: they always go to stderr so
    // stdout stays machine-readable (`--format json`, `--format compact`,
    // `--format sarif`).
    // A non-empty `RUST_LOG` overrides `--verbose`; invalid directives are
    // reported on stderr (`ignoring ...`) and skipped, and if none remain
    // the default applies: `--verbose` selects `debug`, otherwise `warn`.
    let default_level = if cli.verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::WARN
    };
    let filter = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    match cli.command {
        Commands::Check {
            path,
            format,
            rules,
            exclude,
            engine,
            exit_zero,
            baseline,
            update_baseline,
        } => {
            let source = ConfigSource::resolve(&path, cli.config.as_deref());
            let engine = engine.unwrap_or_else(|| detect_engine(&source));
            let outcome = match engine {
                EngineHint::Syn => commands::check::run(
                    &path,
                    format,
                    rules,
                    exclude,
                    &source,
                    baseline.as_deref(),
                    update_baseline,
                )?,
                EngineHint::Ts => {
                    if baseline.is_some() {
                        anyhow::bail!("--baseline is only supported by the syn engine");
                    }
                    commands::check_ts::run(&path, format, &source)?
                }
            };
            Ok(if exit_zero { Outcome::Clean } else { outcome })
        }
        Commands::ListRules { json } => commands::list_rules::run(json).map(|()| Outcome::Clean),
        Commands::Schema { out } => commands::schema::run(out.as_deref()).map(|()| Outcome::Clean),
        Commands::Init { force, ts } => if ts {
            commands::init_ts::run(force)
        } else {
            commands::init::run(force)
        }
        .map(|()| Outcome::Clean),
        Commands::Graph { path, format } => {
            let source = ConfigSource::resolve(&path, cli.config.as_deref());
            commands::graph::run(&path, format.into(), &source)
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
/// or quoted `[[layers]]` text does not switch engines. Also used by
/// `commands::graph` to reject a tree-sitter configuration before building a
/// `syn`-engine session.
pub(crate) fn has_layers_table(content: &str) -> bool {
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
