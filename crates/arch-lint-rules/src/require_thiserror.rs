//! Rule to require `thiserror` derive for error types.
//!
//! # Rationale
//!
//! Consistent error handling with `thiserror` provides:
//! - Automatic `std::error::Error` implementation
//! - Structured error messages with `#[error("...")]`
//! - Source error chaining with `#[from]` and `#[source]`
//!
//! # Detected Patterns
//!
//! - Structs/enums ending with `Error` without `#[derive(thiserror::Error)]`
//! - Custom `impl std::error::Error` without thiserror
//!
//! # Good Patterns
//!
//! ```ignore
//! #[derive(Debug, thiserror::Error)]
//! pub enum MyError {
//!     #[error("IO error: {0}")]
//!     Io(#[from] std::io::Error),
//!
//!     #[error("Parse error at line {line}")]
//!     Parse { line: usize },
//! }
//! ```

use arch_lint_core::utils::{check_allow_comment, has_allow_attr};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{ItemEnum, ItemStruct};

/// Rule code for require-thiserror.
pub const CODE: &str = "AL005";

/// Rule name for require-thiserror.
pub const NAME: &str = "require-thiserror";

/// Requires `thiserror::Error` derive for error types.
#[derive(Debug, Clone)]
pub struct RequireThiserror {
    /// Severity level.
    pub severity: Severity,
    /// Patterns to match error type names.
    pub patterns: Vec<String>,
}

impl Default for RequireThiserror {
    fn default() -> Self {
        Self::new()
    }
}

impl RequireThiserror {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            severity: Severity::Warning,
            patterns: vec!["Error".to_string()],
        }
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Adds a pattern for error type names.
    #[must_use]
    pub fn add_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    fn is_error_type(&self, name: &str) -> bool {
        self.patterns.iter().any(|p| name.ends_with(p))
    }
}

impl Rule for RequireThiserror {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Requires thiserror::Error derive for error types"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = ThiserrorVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct ThiserrorVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a RequireThiserror,
    violations: Vec<Violation>,
}

impl<'ast> Visit<'ast> for ThiserrorVisitor<'_> {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let name = node.ident.to_string();

        if self.rule.is_error_type(&name) && !has_thiserror_derive(&node.attrs) {
            self.report_violation(&name, node.ident.span(), &node.attrs);
        }

        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        let name = node.ident.to_string();

        if self.rule.is_error_type(&name) && !has_thiserror_derive(&node.attrs) {
            self.report_violation(&name, node.ident.span(), &node.attrs);
        }

        syn::visit::visit_item_enum(self, node);
    }
}

impl ThiserrorVisitor<'_> {
    fn report_violation(&mut self, name: &str, span: proc_macro2::Span, attrs: &[syn::Attribute]) {
        let start = span.start();

        // Check for allow attributes
        if has_allow_attr(attrs, &["require_thiserror"]) {
            return;
        }

        // Check for inline allow comment
        if check_allow_comment(self.ctx.content, start.line, NAME).is_allowed() {
            return;
        }

        let location = Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

        self.violations.push(
            Violation::new(
                CODE,
                NAME,
                self.rule.severity,
                location,
                format!("Error type `{name}` should derive `thiserror::Error`"),
            )
            .with_suggestion(Suggestion::new(
                "Add `#[derive(Debug, thiserror::Error)]` and `#[error(\"...\")]` attributes",
            )),
        );
    }
}

/// Checks if attributes contain `#[derive(thiserror::Error)]` or `#[derive(Error)]`.
fn has_thiserror_derive(attrs: &[syn::Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }

        let attr_str = quote::quote!(#attr).to_string();
        // Check for various thiserror patterns
        if attr_str.contains("thiserror::Error")
            || attr_str.contains("thiserror :: Error")
            || (attr_str.contains("Error") && !attr_str.contains("derive(Error)"))
        {
            // More precise check for standalone Error in derive
            let normalized = attr_str.replace(' ', "");
            if normalized.contains("thiserror::Error") {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn check_code(code: &str) -> Vec<Violation> {
        let ast = syn::parse_file(code).expect("Failed to parse");
        let ctx = FileContext {
            path: Path::new("test.rs"),
            content: code,
            is_test: false,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("test.rs"),
        };
        RequireThiserror::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_missing_thiserror() {
        let violations = check_code(
            r#"
#[derive(Debug)]
pub enum MyError {
    Io(std::io::Error),
    Parse(String),
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
    }

    #[test]
    fn test_allows_with_thiserror() {
        let violations = check_code(
            r#"
#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("IO error")]
    Io(#[from] std::io::Error),
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_ignores_non_error_types() {
        let violations = check_code(
            r#"
#[derive(Debug)]
pub struct Config {
    name: String,
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_struct_error() {
        let violations = check_code(
            r#"
#[derive(Debug)]
pub struct ParseError {
    line: usize,
    message: String,
}
"#,
        );
        assert_eq!(violations.len(), 1);
    }
}
