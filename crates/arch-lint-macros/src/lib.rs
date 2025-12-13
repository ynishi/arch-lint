//! # arch-lint-macros
//!
//! Procedural macros for simplifying arch-lint rule definitions.
//!
//! This crate provides derive macros and attribute macros to reduce
//! boilerplate when implementing lint rules.
//!
//! ## Future Features
//!
//! - `#[derive(Rule)]` - Auto-implement Rule trait boilerplate
//! - `#[rule_test]` - Generate test helpers for rules
//!
//! Currently a placeholder for future development.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

/// Placeholder for future Rule derive macro.
///
/// Will auto-generate `name()`, `code()`, and `description()` methods.
#[proc_macro_derive(LintRule, attributes(rule))]
pub fn derive_lint_rule(_input: TokenStream) -> TokenStream {
    // Placeholder - to be implemented
    TokenStream::new()
}
