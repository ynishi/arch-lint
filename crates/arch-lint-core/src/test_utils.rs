//! Helpers for unit-testing rules.
//!
//! Enabled by the `test-utils` feature (and always available inside this
//! crate's own tests). Rule crates add it as a dev-dependency feature:
//!
//! ```toml
//! [dev-dependencies]
//! arch-lint-core = { workspace = true, features = ["test-utils"] }
//! ```
//!
//! ```
//! # #[cfg(feature = "test-utils")] {
//! use arch_lint_core::test_utils::check_rule;
//! # }
//! ```

use crate::context::FileContext;
use crate::rule::Rule;
use crate::types::Violation;
use std::path::Path;

/// Builds a [`FileContext`] for `content` at `path`, taken as relative to the
/// project root (which is also the crate root), so `src/foo.rs` has the module
/// path `["crate", "foo"]`.
///
/// `is_test` is used as given, not detected from `path`.
#[must_use]
pub fn file_ctx<'a>(path: &'a str, content: &'a str, is_test: bool) -> FileContext<'a> {
    let mut ctx = FileContext::new(Path::new(path), content, Path::new(""));
    ctx.is_test = is_test;
    ctx
}

/// Parses `code` and runs `rule` over it as the non-test file `test.rs`.
///
/// # Panics
///
/// Panics if `code` is not valid Rust.
#[must_use]
pub fn check_rule(rule: &dyn Rule, code: &str) -> Vec<Violation> {
    let ast = parse(code);
    rule.check(&file_ctx("test.rs", code, false), &ast)
}

/// Parses `code` and runs `rule` over it as the test file `tests/test.rs`
/// (`FileContext::is_test` is true).
///
/// # Panics
///
/// Panics if `code` is not valid Rust.
#[must_use]
pub fn check_rule_in_test_file(rule: &dyn Rule, code: &str) -> Vec<Violation> {
    let ast = parse(code);
    rule.check(&file_ctx("tests/test.rs", code, true), &ast)
}

fn parse(code: &str) -> syn::File {
    match syn::parse_file(code) {
        Ok(ast) => ast,
        Err(err) => panic!("Failed to parse: {err}"),
    }
}
