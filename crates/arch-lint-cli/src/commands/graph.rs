//! Graph command implementation.

use std::io::Write;
use std::path::Path;

use anyhow::{bail, Result};
use arch_lint::graph::{self, GraphFormat};
use arch_lint::session::{ConfigSource, Session};

use super::Outcome;

/// Runs the `graph` command.
///
/// Renders the [`arch_lint::graph`] of the project as `format`, with edges
/// that a lint run flags (`ALD003`, `ALD004`, `ALD009`, `ALD010`, `ALD011`)
/// styled as violations (see
/// [`arch_lint::graph::violating_edges`]). Violations never fail this
/// command — only `check` does — so it always prints and returns
/// [`Outcome::Clean`] once it runs at all.
///
/// Supports the `syn` engine only: a configuration with a non-empty
/// `[[layers]]` table (the tree-sitter engine) is rejected before the
/// session is built, with a message naming the config file.
///
/// A project with no `[[scopes]]` prints an empty graph and a one-line hint
/// on stderr.
///
/// # Errors
///
/// Returns an error (`main` maps it to exit status 2) if the configuration
/// declares `[[layers]]`, cannot be loaded, or the analysis itself fails.
pub fn run(path: &Path, format: GraphFormat, source: &ConfigSource) -> Result<Outcome> {
    if let Some(config_path) = source.path() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if crate::has_layers_table(&content) {
                bail!(
                    "`arch-lint graph` supports the syn engine only; {} declares [[layers]] \
                     (tree-sitter engine)",
                    config_path.display()
                );
            }
        }
    }

    let session = Session::from_source(path, source.clone())?;

    // Both built from the same session configuration: `scope_graph` first
    // (it only borrows), then `run` (it consumes `session`).
    let scope_graph = session.scope_graph()?;
    let result = session.run()?;

    let violating = graph::violating_edges(&scope_graph, &result);
    let rendered = graph::render(&scope_graph, format, &violating);

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(rendered.as_bytes())?;
    stdout.flush()?;

    if scope_graph.is_empty() {
        let where_ = source.path().map_or_else(
            || "the configuration".to_string(),
            |p| p.display().to_string(),
        );
        eprintln!("no [[scopes]] declared in {where_}");
    }

    Ok(Outcome::Clean)
}
