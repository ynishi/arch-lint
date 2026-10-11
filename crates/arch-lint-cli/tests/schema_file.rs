//! Drift check for the committed JSON Schema: issue #16.
//!
//! `schema/arch-lint.schema.json` is tracked so SchemaStore and editors have
//! a stable URL to point at (`crates/arch-lint/src/schema.rs`'s `SCHEMA_ID`
//! is that URL). Nothing enforces that the committed bytes still match what
//! the code generates, short of this test: it runs the built binary's
//! `schema` subcommand the same way a contributor running `make schema`
//! would, and compares its stdout to the committed file.
//!
//! This replaces a separate CI job (`make schema-check`) — this test already
//! runs in the `test (arch-lint-cli)` CI job, with no extra workflow step
//! needed. `make schema` / `make schema-check` stay in the Makefile for
//! local regeneration and a quick manual check.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Repository root, derived from this crate's manifest directory — the same
/// pattern `tests/cargo_subcommand.rs` uses.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repository root resolves")
}

fn arch_lint_schema() -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("schema")
        .current_dir(repo_root())
        .output()
        .expect("run arch-lint schema")
}

#[test]
fn schema_file_matches_generated() {
    let output = arch_lint_schema();
    assert!(
        output.status.success(),
        "arch-lint schema exited with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let committed_path = repo_root().join("schema/arch-lint.schema.json");
    let committed = std::fs::read(&committed_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", committed_path.display()));

    // Both are produced by the exact same code path (`serde_json::to_
    // string_pretty` plus one trailing newline), so they are byte-identical
    // today — compare strictly, with no normalisation, so any future drift
    // (trailing newline included) fails loudly.
    assert_eq!(
        output.stdout, committed,
        "schema/arch-lint.schema.json is stale; regenerate it with `make schema`"
    );
}
