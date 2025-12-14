//! Rule to prefer `From` trait implementation over `Into`.
//!
//! # Rationale
//!
//! Implementing `From` automatically provides `Into` implementation for free
//! due to Rust's blanket implementation. Implementing `Into` directly is
//! redundant and goes against Rust conventions.
//!
//! # Detected Patterns
//!
//! - `impl Into<T> for U { ... }`
//!
//! # Good Patterns
//!
//! ```ignore
//! // Good - Implement From, get Into for free
//! impl From<MyType> for String {
//!     fn from(value: MyType) -> String {
//!         value.0
//!     }
//! }
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::check_arch_lint_allow;
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
#[allow(unused_imports)]
use syn::spanned::Spanned;
use syn::{ItemImpl, ItemMod};

/// Rule code for prefer-from-over-into.
pub const CODE: &str = "AL010";

/// Rule name for prefer-from-over-into.
pub const NAME: &str = "prefer-from-over-into";

/// Prefers `From` trait implementation over `Into`.
#[derive(Debug, Clone)]
pub struct PreferFromOverInto {
    /// Severity level.
    pub severity: Severity,
}

impl Default for PreferFromOverInto {
    fn default() -> Self {
        Self::new()
    }
}

impl PreferFromOverInto {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            severity: Severity::Warning,
        }
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl Rule for PreferFromOverInto {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Prefers From trait implementation over Into"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = FromIntoVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct FromIntoVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a PreferFromOverInto,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for FromIntoVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        // Check if this impl is explicitly allowed
        if self.in_allowed_context || check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            syn::visit::visit_item_impl(self, node);
            return;
        }

        // Check if this is an `impl Into<T> for U` pattern
        if let Some((_, path, _)) = &node.trait_ {
            // Get the last segment of the path
            if let Some(segment) = path.segments.last() {
                if segment.ident == "Into" {
                    let span = segment.ident.span();
                    let start = span.start();

                    // Check for inline allow comment
                    let allow_check = check_allow_with_reason(self.ctx.content, start.line, NAME);
                    if allow_check.is_allowed() {
                        // If reason is required but not provided, create a separate violation
                        if self.rule.requires_allow_reason() && allow_check.reason().is_none() {
                            let location = Location::new(
                                self.ctx.relative_path.clone(),
                                start.line,
                                start.column + 1,
                            );
                            self.violations.push(
                                Violation::new(
                                    CODE,
                                    NAME,
                                    Severity::Warning,
                                    location,
                                    format!("Allow directive for '{NAME}' is missing required reason"),
                                )
                                .with_suggestion(Suggestion::new(
                                    "Add reason=\"...\" to explain why this exception is necessary",
                                )),
                            );
                        }
                        syn::visit::visit_item_impl(self, node);
                        return;
                    }

                    // Extract type information for better error message
                    let self_type = quote::quote!(#node.self_ty).to_string();

                    let location = Location::new(
                        self.ctx.relative_path.clone(),
                        start.line,
                        start.column + 1,
                    );

                    self.violations.push(
                        Violation::new(
                            CODE,
                            NAME,
                            self.rule.severity,
                            location,
                            format!("Implement `From` instead of `Into` for type `{self_type}`"),
                        )
                        .with_suggestion(Suggestion::new(
                            "Replace `impl Into<Target> for Source` with `impl From<Source> for Target`. \
                            The Into implementation will be provided automatically."
                        )),
                    );
                }
            }
        }

        syn::visit::visit_item_impl(self, node);
    }
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
        PreferFromOverInto::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_into_impl() {
        let violations = check_code(
            r"
struct MyType(String);

impl Into<String> for MyType {
    fn into(self) -> String {
        self.0
    }
}
",
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("Implement `From` instead"));
    }

    #[test]
    fn test_allows_from_impl() {
        let violations = check_code(
            r"
struct MyType(String);

impl From<MyType> for String {
    fn from(value: MyType) -> String {
        value.0
    }
}
",
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_other_traits() {
        let violations = check_code(
            r#"
struct MyType;

impl std::fmt::Display for MyType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "MyType")
    }
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r"
struct MyType(String);

#[arch_lint::allow(prefer_from_over_into)]
impl Into<String> for MyType {
    fn into(self) -> String {
        self.0
    }
}
",
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_into_impls() {
        let violations = check_code(
            r"
struct Type1(String);
struct Type2(i32);

impl Into<String> for Type1 {
    fn into(self) -> String {
        self.0
    }
}

impl Into<i32> for Type2 {
    fn into(self) -> i32 {
        self.0
    }
}
",
        );
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
struct MyType(String);

// arch-lint: allow(prefer-from-over-into) reason="External crate requires Into"
impl Into<String> for MyType {
    fn into(self) -> String {
        self.0
    }
}
"#,
        );
        assert!(violations.is_empty());
    }
}
