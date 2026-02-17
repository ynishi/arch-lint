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
