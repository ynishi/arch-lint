//! Rule to require `tracing` crate instead of `log` crate.
//!
//! # Rationale
//!
//! `tracing` provides structured logging with better context and performance.
//! It's designed for async applications and offers more powerful diagnostics.
//!
//! # Detected Patterns
//!
//! - `log::info!`, `log::error!`, `log::warn!`, `log::debug!`, `log::trace!`
//! - Any macro from `log::` crate
//!
//! # Good Patterns
//!
//! ```ignore
//! // Use tracing instead
//! tracing::info!("message");
//! tracing::error!("error occurred");
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{check_arch_lint_allow, path_to_string};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{ExprMacro, ItemFn, ItemImpl, ItemMod};

/// Rule code for require-tracing.
pub const CODE: &str = "AL006";

/// Rule name for require-tracing.
pub const NAME: &str = "require-tracing";

/// Requires `tracing` crate instead of `log` crate.
#[derive(Debug, Clone)]
pub struct RequireTracing {
    /// Severity level.
    pub severity: Severity,
}

impl Default for RequireTracing {
    fn default() -> Self {
        Self::new()
    }
}

impl RequireTracing {
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

impl Rule for RequireTracing {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Requires tracing crate instead of log crate"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = TracingVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct TracingVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a RequireTracing,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for TracingVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_impl(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if self.in_allowed_context {
            syn::visit::visit_macro(self, node);
            return;
        }

        let path_str = path_to_string(&node.path);

        // Check if this is a log:: macro
        if path_str.starts_with("log::") {
            let Some(first_segment) = node.path.segments.first() else {
                syn::visit::visit_macro(self, node);
                return;
            };
            let span = first_segment.ident.span();
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
                syn::visit::visit_macro(self, node);
                return;
            }

            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            // Extract macro name (e.g., "info" from "log::info")
            let macro_name = path_str.strip_prefix("log::").unwrap_or(&path_str);

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!("Use `tracing::{macro_name}!` instead of `log::{macro_name}!`"),
                )
                .with_suggestion(Suggestion::new(format!(
                    "Replace with `tracing::{macro_name}!` for structured logging"
                ))),
            );
        }

        syn::visit::visit_macro(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        if self.in_allowed_context {
            syn::visit::visit_expr_macro(self, node);
            return;
        }

        let path_str = path_to_string(&node.mac.path);

        // Check if this is a log:: macro
        if path_str.starts_with("log::") {
            let Some(first_segment) = node.mac.path.segments.first() else {
                syn::visit::visit_expr_macro(self, node);
                return;
            };
            let span = first_segment.ident.span();
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
                syn::visit::visit_expr_macro(self, node);
                return;
            }

            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            // Extract macro name (e.g., "info" from "log::info")
            let macro_name = path_str.strip_prefix("log::").unwrap_or(&path_str);

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!("Use `tracing::{macro_name}!` instead of `log::{macro_name}!`"),
                )
                .with_suggestion(Suggestion::new(format!(
                    "Replace with `tracing::{macro_name}!` for structured logging"
                ))),
            );
        }

        syn::visit::visit_expr_macro(self, node);
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
        RequireTracing::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_log_info() {
        let violations = check_code(
            r#"
fn foo() {
    log::info!("message");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("tracing::info"));
    }

    #[test]
    fn test_detects_log_error() {
        let violations = check_code(
            r#"
fn foo() {
    log::error!("error: {}", e);
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("tracing::error"));
    }

    #[test]
    fn test_allows_tracing() {
        let violations = check_code(
            r#"
fn foo() {
    tracing::info!("message");
    tracing::error!("error");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
#[arch_lint::allow(require_tracing)]
fn legacy_code() {
    log::info!("old code");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(require-tracing) reason="Legacy dependency uses log"
    log::info!("message");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_log_macros() {
        let violations = check_code(
            r#"
fn foo() {
    log::debug!("debug");
    log::info!("info");
    log::warn!("warn");
    log::error!("error");
    log::trace!("trace");
}
"#,
        );
        assert_eq!(violations.len(), 5);
    }
}
