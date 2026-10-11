//! Integration tests for the `arch_lint::check!()` macro.
//!
//! These tests verify that the macro correctly generates test functions
//! and that the runner integrates with the analyzer.

// Runs minimal preset with examples excluded.
// This verifies the full pipeline: macro expansion → config load → analysis → pass.
arch_lint::check!(
    preset = "minimal",
    config = "crates/arch-lint/tests/test-config.toml",
);

/// Macro-expansion coverage for `check!(baseline = "...")`: a `mod` of its
/// own so the generated `arch_lint_check` test function does not collide
/// with the top-level one above. Runs against this same (already-clean)
/// repository tree with a committed, intentionally empty baseline — this is
/// expansion coverage only (does `baseline = "..."` parse and route to
/// `run_check_with_baseline` without error), not a suppression test; see
/// `crates/arch-lint-cli/tests/baseline.rs` and the unit tests in
/// `crates/arch-lint/src/baseline.rs` for that.
mod with_baseline {
    arch_lint::check!(
        preset = "minimal",
        config = "crates/arch-lint/tests/test-config.toml",
        baseline = "crates/arch-lint/tests/empty.baseline.toml",
    );
}
