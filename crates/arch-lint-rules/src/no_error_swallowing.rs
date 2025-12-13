//! Rule to forbid swallowing errors with only logging.
//!
//! # Rationale
//!
//! Catching errors and only logging them (without propagation) hides failures
//! and makes debugging difficult. Errors should either be propagated or handled
//! with explicit recovery logic.
//!
//! # Detected Patterns
//!
//! ```ignore
//! // BAD: Error is logged but not propagated
//! if let Err(e) = result {
//!     tracing::error!("Failed: {}", e);
//! }
//!
//! // BAD: Match arm only logs
//! match result {
//!     Ok(v) => v,
//!     Err(e) => {
//!         log::error!("{}", e);
//!         return;
//!     }
//! }
//! ```
//!
//! # Good Patterns
//!
//! ```ignore
//! // GOOD: Error is propagated
//! result?;
//!
//! // GOOD: Error is propagated with context
//! result.map_err(|e| {
//!     tracing::error!("Failed: {}", e);
//!     e
//! })?;
//!
//! // GOOD: Explicit recovery
//! let value = match result {
//!     Ok(v) => v,
//!     Err(e) => {
//!         tracing::warn!("Using fallback: {}", e);
//!         default_value()
//!     }
//! };
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Arm, Expr, ExprIf, ExprMatch, Pat, Stmt};

/// Rule code for no-error-swallowing.
pub const CODE: &str = "AL003";

/// Rule name for no-error-swallowing.
pub const NAME: &str = "no-error-swallowing";

/// Logging macro names to detect.
const LOGGING_MACROS: &[&str] = &[
    "error",
    "warn",
    "info",
    "debug",
    "trace",
    "log::error",
    "log::warn",
    "log::info",
    "log::debug",
    "log::trace",
    "tracing::error",
    "tracing::warn",
    "tracing::info",
    "tracing::debug",
    "tracing::trace",
    "eprintln",
    "println",
];

/// Forbids catching errors with only logging (no propagation).
#[derive(Debug, Clone)]
pub struct NoErrorSwallowing {
    /// Custom severity.
    pub severity: Severity,
}

impl Default for NoErrorSwallowing {
    fn default() -> Self {
        Self::new()
    }
}

impl NoErrorSwallowing {
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

impl Rule for NoErrorSwallowing {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Forbids catching errors with only logging (no propagation)"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = ErrorSwallowingVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct ErrorSwallowingVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoErrorSwallowing,
    violations: Vec<Violation>,
}

impl<'ast> Visit<'ast> for ErrorSwallowingVisitor<'_> {
    fn visit_expr_if(&mut self, node: &'ast ExprIf) {
        // Check for `if let Err(e) = expr { logging_only }`
        if let Expr::Let(expr_let) = &*node.cond {
            if is_err_pattern(&expr_let.pat) && is_only_logging_block(&node.then_branch.stmts) {
                self.report_violation(node.if_token.span);
            }
        }

        syn::visit::visit_expr_if(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        for arm in &node.arms {
            if is_err_pattern(&arm.pat) && is_only_logging_expr(&arm.body) {
                self.report_violation_at_arm(arm);
            }
        }

        syn::visit::visit_expr_match(self, node);
    }
}

impl ErrorSwallowingVisitor<'_> {
    fn report_violation(&mut self, span: proc_macro2::Span) {
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
            return;
        }

        let location = Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

        self.violations.push(
            Violation::new(
                CODE,
                NAME,
                self.rule.severity,
                location,
                "Error is caught but only logged, not propagated or handled",
            )
            .with_suggestion(Suggestion::new(
                "Propagate error with `?` or add explicit recovery logic",
            )),
        );
    }

    fn report_violation_at_arm(&mut self, arm: &Arm) {
        let span = arm.pat.span();
        let start = span.start();

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
            return;
        }

        let location = Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

        self.violations.push(
            Violation::new(
                CODE,
                NAME,
                self.rule.severity,
                location,
                "Error arm only logs without propagation or recovery",
            )
            .with_suggestion(Suggestion::new(
                "Return the error or provide fallback value",
            )),
        );
    }
}

/// Checks if a pattern matches `Err(...)`.
fn is_err_pattern(pat: &Pat) -> bool {
    match pat {
        Pat::TupleStruct(ts) => {
            // Check if the path is just "Err"
            if let Some(segment) = ts.path.segments.last() {
                segment.ident == "Err"
            } else {
                false
            }
        }
        Pat::Ident(ident) => ident.ident == "Err",
        _ => false,
    }
}

/// Checks if a block contains only logging statements.
fn is_only_logging_block(stmts: &[Stmt]) -> bool {
    if stmts.is_empty() {
        return false;
    }

    for stmt in stmts {
        match stmt {
            Stmt::Expr(expr, _) => {
                if !is_logging_expr(expr) && !is_return_unit(expr) {
                    return false;
                }
            }
            Stmt::Local(local) => {
                // Local bindings are generally OK in error handlers
                if let Some(init) = &local.init {
                    if !is_logging_expr(&init.expr) {
                        return false;
                    }
                }
            }
            Stmt::Macro(stmt_macro) => {
                if !is_logging_macro(&stmt_macro.mac) {
                    return false;
                }
            }
            Stmt::Item(_) => return false,
        }
    }

    // Must have at least one logging statement
    stmts
        .iter()
        .any(|s| matches!(s, Stmt::Macro(m) if is_logging_macro(&m.mac)))
}

/// Checks if an expression is only logging.
fn is_only_logging_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Block(block) => is_only_logging_block(&block.block.stmts),
        Expr::Macro(m) => is_logging_macro(&m.mac),
        _ => false,
    }
}

/// Checks if an expression is a logging call.
fn is_logging_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Macro(m) => is_logging_macro(&m.mac),
        Expr::Block(block) => block.block.stmts.iter().all(|s| {
            matches!(s, Stmt::Expr(e, _) if is_logging_expr(e))
                || matches!(s, Stmt::Macro(m) if is_logging_macro(&m.mac))
        }),
        _ => false,
    }
}

/// Checks if expression is `return` or `return ()`.
fn is_return_unit(expr: &Expr) -> bool {
    match expr {
        Expr::Return(ret) => match &ret.expr {
            None => true,
            Some(e) => matches!(&**e, Expr::Tuple(t) if t.elems.is_empty()),
        },
        _ => false,
    }
}

/// Checks if a macro is a logging macro.
fn is_logging_macro(mac: &syn::Macro) -> bool {
    // Get the path segments
    let segments: Vec<_> = mac
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    let path_str = segments.join("::");

    // Check against known logging macros
    LOGGING_MACROS.iter().any(|&name| {
        path_str == name
            || path_str.ends_with(&format!("::{name}"))
            || segments.last().map(String::as_str) == Some(name)
    })
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
        NoErrorSwallowing::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_if_let_err_with_logging() {
        let violations = check_code(
            r#"
fn foo() {
    if let Err(e) = do_something() {
        tracing::error!("Failed: {}", e);
    }
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
    }

    #[test]
    fn test_allows_error_propagation() {
        let violations = check_code(
            r#"
fn foo() -> Result<(), Error> {
    do_something()?;
    Ok(())
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_comment() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(no-error-swallowing)
    if let Err(e) = do_something() {
        tracing::error!("Failed: {}", e);
    }
}
"#,
        );
        assert!(violations.is_empty());
    }
}
