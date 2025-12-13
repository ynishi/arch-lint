//! Rule to forbid `.unwrap()` and `.expect()` in production code.
//!
//! # Rationale
//!
//! Using `.unwrap()` or `.expect()` can cause panics at runtime, which is
//! undesirable in production code. This rule helps enforce proper error handling.
//!
//! # Configuration
//!
//! - `allow_in_tests`: Allow in test code (default: true)
//! - `allow_expect`: Allow `.expect()` but forbid `.unwrap()` (default: false)
//!
//! # Suppression
//!
//! - `#[allow(clippy::unwrap_used)]` on the item
//! - `// arch-lint: allow(no-unwrap-expect)` comment

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{has_allow_attr, has_cfg_test, has_test_attr};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{Expr, ExprMethodCall, ItemFn, ItemMod};

/// Rule code for no-unwrap-expect.
pub const CODE: &str = "AL001";

/// Rule name for no-unwrap-expect.
pub const NAME: &str = "no-unwrap-expect";

/// Forbids `.unwrap()` and `.expect()` calls in production code.
#[derive(Debug, Clone)]
pub struct NoUnwrapExpect {
    /// Allow in test code.
    pub allow_in_tests: bool,
    /// Allow `.expect()` (only forbid `.unwrap()`).
    pub allow_expect: bool,
    /// Custom severity.
    pub severity: Severity,
}

impl Default for NoUnwrapExpect {
    fn default() -> Self {
        Self::new()
    }
}

impl NoUnwrapExpect {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allow_in_tests: true,
            allow_expect: false,
            severity: Severity::Error,
        }
    }

    /// Sets whether to allow in test code.
    #[must_use]
    pub fn allow_in_tests(mut self, allow: bool) -> Self {
        self.allow_in_tests = allow;
        self
    }

    /// Sets whether to allow `.expect()`.
    #[must_use]
    pub fn allow_expect(mut self, allow: bool) -> Self {
        self.allow_expect = allow;
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl Rule for NoUnwrapExpect {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Forbids .unwrap() and .expect() in production code"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        // Skip test files if configured
        if self.allow_in_tests && ctx.is_test {
            return Vec::new();
        }

        let mut visitor = UnwrapExpectVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_test_context: false,
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct UnwrapExpectVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoUnwrapExpect,
    violations: Vec<Violation>,
    in_test_context: bool,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for UnwrapExpectVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        // Check for #[cfg(test)] module
        let was_in_test = self.in_test_context;
        if has_cfg_test(&node.attrs) {
            self.in_test_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_test_context = was_in_test;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        // Check for #[test] function or #[allow(...)] attribute
        let was_in_test = self.in_test_context;
        let was_allowed = self.in_allowed_context;

        if has_test_attr(&node.attrs) {
            self.in_test_context = true;
        }

        if has_allow_attr(&node.attrs, &["clippy::unwrap_used", "clippy::expect_used"]) {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);

        self.in_test_context = was_in_test;
        self.in_allowed_context = was_allowed;
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        // Skip if in test context and tests are allowed
        if self.rule.allow_in_tests && self.in_test_context {
            syn::visit::visit_expr_method_call(self, node);
            return;
        }

        // Skip if in allowed context
        if self.in_allowed_context {
            syn::visit::visit_expr_method_call(self, node);
            return;
        }

        let method_name = node.method.to_string();
        let is_unwrap = method_name == "unwrap";
        let is_expect = method_name == "expect";

        if is_unwrap || (is_expect && !self.rule.allow_expect) {
            let span = node.method.span();
            let start = span.start();

            // Check for inline allow comment
            let allow_check = check_allow_with_reason(self.ctx.content, start.line, NAME);
            if allow_check.is_allowed() {
                // If reason is required but not provided, create a separate violation
                if self.rule.requires_allow_reason() && allow_check.reason().is_none() {
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);
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
                syn::visit::visit_expr_method_call(self, node);
                return;
            }

            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            let (message, suggestion) = if is_unwrap {
                (
                    ".unwrap() is forbidden in production code".to_string(),
                    Suggestion::new("Use `?` operator, `.ok_or(Error)?`, or pattern matching"),
                )
            } else {
                (
                    ".expect() is forbidden in production code".to_string(),
                    Suggestion::new("Use `?` operator with `.context()` or custom error"),
                )
            };

            // Check for partial_cmp().unwrap() pattern (NaN danger)
            let is_partial_cmp_unwrap = is_unwrap && is_partial_cmp_chain(&node.receiver);
            let message = if is_partial_cmp_unwrap {
                format!("{message} (NaN comparison danger with partial_cmp)")
            } else {
                message
            };

            self.violations.push(
                Violation::new(CODE, NAME, self.rule.severity, location, message)
                    .with_suggestion(suggestion),
            );
        }

        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Checks if the receiver is a `partial_cmp()` call.
fn is_partial_cmp_chain(expr: &Expr) -> bool {
    if let Expr::MethodCall(call) = expr {
        call.method == "partial_cmp"
    } else {
        false
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
        NoUnwrapExpect::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_unwrap() {
        let violations = check_code(
            r#"
fn foo() {
    let x = Some(1).unwrap();
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
    }

    #[test]
    fn test_detects_expect() {
        let violations = check_code(
            r#"
fn foo() {
    let x = Some(1).expect("should exist");
}
"#,
        );
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn test_allows_in_test_fn() {
        let violations = check_code(
            r#"
#[test]
fn test_foo() {
    let x = Some(1).unwrap();
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_in_cfg_test_mod() {
        let violations = check_code(
            r#"
#[cfg(test)]
mod tests {
    fn helper() {
        let x = Some(1).unwrap();
    }
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_clippy_allow() {
        let violations = check_code(
            r#"
#[allow(clippy::unwrap_used)]
fn foo() {
    let x = Some(1).unwrap();
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_comment_but_warns_missing_reason() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    let x = Some(1).unwrap();
}
"#,
        );
        // Allow directive without reason generates a warning
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("missing required reason"));
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    #[test]
    fn test_requires_reason_when_severity_error() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    let x = Some(1).unwrap();
}
"#,
        );
        // Should have a warning about missing reason
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("missing required reason"));
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    #[test]
    fn test_accepts_reason() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(no-unwrap-expect) reason="Startup initialization, cannot fail"
    let x = Some(1).unwrap();
}
"#,
        );
        // Should not have any violations when reason is provided
        assert!(violations.is_empty());
    }
}
