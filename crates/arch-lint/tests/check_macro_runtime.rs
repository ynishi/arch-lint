//! Runtime behaviour of the `check!(baseline = "...")` macro option,
//! exercised directly through `arch_lint::__internal::run_check_at` —
//! the `#[doc(hidden)]` function both `run_check` and
//! `run_check_with_baseline` (what the macro actually calls, see
//! `crates/arch-lint-macros/src/lib.rs`) delegate to, parameterized on the
//! project root instead of `find_project_root()`'s real
//! `CARGO_MANIFEST_DIR` walk.
//!
//! This replaces an earlier version of this test that spawned a real
//! `cargo test` subprocess on a throwaway crate (to work around
//! `check!()` resolving its root from the environment at runtime) — heavier
//! and, with each invocation building its own isolated dependency graph,
//! slower for the same coverage `run_check_at` gives directly.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use arch_lint::__internal::run_check_at;
use arch_lint::Session;
use tempfile::TempDir;

/// A project with one `.unwrap()` violation in each of `src/a.rs`,
/// `src/b.rs`, `src/c.rs` (mirrors `crates/arch-lint-cli/tests/baseline.rs`).
fn project_with_three_violations() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    for name in ["a", "b", "c"] {
        fs::write(
            dir.path().join(format!("src/{name}.rs")),
            "pub fn f(v: Option<i32>) -> i32 {\n    v.unwrap()\n}\n",
        )
        .expect("write src");
    }
    fs::write(
        dir.path().join("arch-lint.toml"),
        "preset = \"recommended\"\n",
    )
    .expect("write config");
    dir
}

/// Writes a baseline at `<root>/arch-lint.baseline.toml` capturing every
/// violation `root` currently has, and returns the baseline's relative name
/// (what a `baseline = "..."` argument would name).
fn write_baseline_for(root: &Path) -> &'static str {
    let result = Session::discover(root)
        .expect("session")
        .run()
        .expect("run");
    arch_lint::baseline::Baseline::from_result(&result)
        .save(root.join("arch-lint.baseline.toml"))
        .expect("save baseline");
    "arch-lint.baseline.toml"
}

/// Runs `run_check_at` and returns the panic payload as text, or `None` if
/// it did not panic.
fn run_catching(root: &Path, baseline: Option<&str>) -> Option<String> {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        run_check_at(root, None, None, None, baseline);
    }));
    match result {
        Ok(()) => None,
        Err(payload) => Some(
            payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "<non-string panic payload>".to_string()),
        ),
    }
}

#[test]
fn pass_case_three_baselined_violations_do_not_panic() {
    let dir = project_with_three_violations();
    let baseline = write_baseline_for(dir.path());

    let payload = run_catching(dir.path(), Some(baseline));
    assert!(payload.is_none(), "unexpected panic: {payload:?}");
}

#[test]
fn a_fourth_violation_panics_with_the_new_violation_and_the_suppressed_count() {
    let dir = project_with_three_violations();
    let baseline = write_baseline_for(dir.path());

    fs::write(
        dir.path().join("src/a.rs"),
        "pub fn f(v: Option<i32>) -> i32 {\n    v.unwrap()\n}\n\npub fn g(v: Option<i32>) -> i32 {\n    v.unwrap()\n}\n",
    )
    .expect("rewrite src/a.rs");

    let payload = run_catching(dir.path(), Some(baseline)).expect("must panic");
    assert!(payload.contains("AL001"), "{payload}");
    assert!(payload.contains("baseline: 3 suppressed"), "{payload}");
}

#[test]
fn a_missing_baseline_file_panics_pointing_at_update_baseline() {
    let dir = project_with_three_violations();

    let payload = run_catching(dir.path(), Some("nonexistent.toml")).expect("must panic");
    assert!(payload.contains("--update-baseline"), "{payload}");
}
