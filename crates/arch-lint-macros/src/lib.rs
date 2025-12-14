//! # arch-lint-macros
//!
//! Procedural macros for arch-lint.
//!
//! ## Suppression Attributes
//!
//! - `#[arch_lint::allow(...)]` - Suppress rules for a function, impl, or module
//! - `#![arch_lint::allow(...)]` - Suppress rules for an entire file
//!
//! ## Examples
//!
//! ```rust,ignore
//! // Block-level suppression
//! #[arch_lint::allow(no_unwrap_expect, reason = "validated input")]
//! fn parse_config() {
//!     value.unwrap();
//! }
//!
//! // File-level suppression (at top of file)
//! #![arch_lint::allow(no_sync_io, reason = "CLI startup")]
//! ```

#![forbid(unsafe_code)]

use proc_macro::TokenStream;

/// Suppresses specified arch-lint rules for the annotated item.
///
/// This is an identity macro - it returns the item unchanged.
/// arch-lint detects this attribute during AST analysis.
///
/// # Arguments
///
/// * `rules` - Comma-separated rule names to allow (e.g., `no_unwrap_expect`)
/// * `reason` - Required for error-severity rules; explains why suppression is acceptable
///
/// # Examples
///
/// ```rust,ignore
/// // Function-level
/// #[arch_lint::allow(no_unwrap_expect, reason = "Startup config, validated externally")]
/// fn load_config() -> Config {
///     CONFIG.get().unwrap().clone()
/// }
///
/// // Module-level
/// #[arch_lint::allow(no_sync_io, reason = "Synchronous CLI commands")]
/// mod cli {
///     // ...
/// }
///
/// // File-level (inner attribute)
/// #![arch_lint::allow(no_sync_io, reason = "Build script")]
/// ```
#[proc_macro_attribute]
pub fn allow(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Identity transform - arch-lint detects this attribute during AST analysis
    item
}

/// Placeholder for future Rule derive macro.
///
/// Will auto-generate `name()`, `code()`, and `description()` methods.
#[proc_macro_derive(LintRule, attributes(rule))]
pub fn derive_lint_rule(_input: TokenStream) -> TokenStream {
    // Placeholder - to be implemented
    TokenStream::new()
}
