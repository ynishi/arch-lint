//! # arch-lint-macros
//!
//! Procedural macros for arch-lint.
//!
//! ## Suppression Attributes
//!
//! - `#[arch_lint::allow(...)]` - Suppress rules for a function, impl, or module
//! - `#![arch_lint::allow(...)]` - Suppress rules for an entire file
//!
//! ## cargo test Integration
//!
//! - `arch_lint::check!()` - Generates a `#[test]` that runs arch-lint checks
//!
//! ## Examples
//!
//! ```rust,ignore
//! // tests/architecture.rs
//! arch_lint::check!();
//!
//! // With options
//! arch_lint::check! {
//!     preset = "strict",
//!     config = "arch-lint.toml",
//!     fail_on = "warning",
//! }
//! ```

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token};

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

/// Options for the `check!()` macro.
struct CheckArgs {
    preset: Option<String>,
    config: Option<String>,
    fail_on: Option<String>,
}

impl Parse for CheckArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut preset = None;
        let mut config = None;
        let mut fail_on = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            let _: Token![=] = input.parse()?;
            let value: LitStr = input.parse()?;

            match key.to_string().as_str() {
                "preset" => preset = Some(value.value()),
                "config" => config = Some(value.value()),
                "fail_on" => fail_on = Some(value.value()),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown option `{other}`, expected: preset, config, fail_on"),
                    ));
                }
            }

            // Consume trailing comma if present
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(Self {
            preset,
            config,
            fail_on,
        })
    }
}

/// Generates a `#[test]` function that runs arch-lint analysis.
///
/// Place this in `tests/architecture.rs` (or any integration test file).
/// It will automatically discover `arch-lint.toml` in the project root
/// and run all configured rules.
///
/// # Examples
///
/// ```rust,ignore
/// // Minimal — uses recommended preset, looks for arch-lint.toml
/// arch_lint::check!();
///
/// // With explicit preset
/// arch_lint::check!(preset = "strict");
///
/// // With custom config path
/// arch_lint::check!(config = "my-lint.toml");
///
/// // Fail on warnings too (default: fail on errors only)
/// arch_lint::check!(fail_on = "warning");
///
/// // Combined
/// arch_lint::check! {
///     preset = "strict",
///     config = "arch-lint.toml",
///     fail_on = "warning",
/// }
/// ```
#[proc_macro]
pub fn check(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as CheckArgs);

    let preset_expr = if let Some(p) = &args.preset {
        quote! { Some(#p) }
    } else {
        quote! { None }
    };
    let config_expr = if let Some(c) = &args.config {
        quote! { Some(#c) }
    } else {
        quote! { None }
    };
    let fail_on_expr = if let Some(f) = &args.fail_on {
        quote! { Some(#f) }
    } else {
        quote! { None }
    };

    let output = quote! {
        #[test]
        fn arch_lint_check() {
            ::arch_lint::__internal::run_check(
                #preset_expr,
                #config_expr,
                #fail_on_expr,
            );
        }
    };

    output.into()
}
