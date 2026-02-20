//! Rule to prevent hardcoded log levels in tracing initialization.
//!
//! # Rationale
//!
//! Hardcoding log levels prevents runtime configuration via environment variables.
//! Using `EnvFilter::from_default_env()` allows flexible log level control through
//! `RUST_LOG` environment variable.
//!
//! # Detected Patterns
//!
//! - `EnvFilter::new("debug")` - hardcoded level
//! - `EnvFilter::new("info")` - hardcoded level
//! - Any string literal passed to `EnvFilter::new()`
//!
//! # Good Patterns
//!
//! ```ignore
//! use tracing_subscriber::EnvFilter;
//!
//! // Use environment variable (RUST_LOG)
//! let filter = EnvFilter::from_default_env();
//!
//! // Or with fallback
//! let filter = EnvFilter::try_from_default_env()
//!     .unwrap_or_else(|_| EnvFilter::new("info"));
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{check_arch_lint_allow, path_to_string};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprLit, ExprMethodCall, ExprPath, ItemFn, ItemImpl, ItemMod, Lit};

/// Rule code for tracing-env-init.
pub const CODE: &str = "AL007";

/// Rule name for tracing-env-init.
pub const NAME: &str = "tracing-env-init";

/// Method names on `EnvFilter` that attempt env-based initialization.
const ENV_INIT_METHODS: &[&str] = &["try_from_default_env", "try_from_env"];

/// Fallback methods whose arguments should be exempt when the receiver
/// is an env-based init call.
const FALLBACK_METHODS: &[&str] = &["unwrap_or_else", "unwrap_or"];

/// Prevents hardcoded log levels in tracing initialization.
#[derive(Debug, Clone)]
pub struct TracingEnvInit {
    /// Severity level.
    pub severity: Severity,
}

impl Default for TracingEnvInit {
    fn default() -> Self {
        Self::new()
    }
}

impl TracingEnvInit {
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

impl Rule for TracingEnvInit {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Prevents hardcoded log levels in tracing initialization"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = EnvInitVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
            in_env_fallback: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct EnvInitVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a TracingEnvInit,
    violations: Vec<Violation>,
    in_allowed_context: bool,
    /// True when inside the fallback argument of
    /// `try_from_default_env().unwrap_or_else(|| ...)`.
    in_env_fallback: bool,
}

/// Checks whether an expression is a call to an env-based `EnvFilter` method
/// (e.g. `EnvFilter::try_from_default_env()`).
fn is_env_init_call(expr: &Expr) -> bool {
    if let Expr::Call(ExprCall { func, .. }) = expr {
        if let Expr::Path(ExprPath { path, .. }) = &**func {
            let path_str = path_to_string(path);
            return ENV_INIT_METHODS.iter().any(|m| path_str.ends_with(m));
        }
    }
    false
}

impl<'ast> Visit<'ast> for EnvInitVisitor<'_> {
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

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method = node.method.to_string();

        // Detect `try_from_default_env().unwrap_or_else(|| EnvFilter::new(...))`
        if FALLBACK_METHODS.contains(&method.as_str()) && is_env_init_call(&node.receiver) {
            // Visit the receiver normally (it's the env call, no hardcoded level)
            self.visit_expr(&node.receiver);

            // Visit fallback arguments with the flag set
            let was_in_fallback = self.in_env_fallback;
            self.in_env_fallback = true;
            for arg in &node.args {
                self.visit_expr(arg);
            }
            self.in_env_fallback = was_in_fallback;
            return;
        }

        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.in_allowed_context || self.in_env_fallback {
            syn::visit::visit_expr_call(self, node);
            return;
        }

        // Check if this is EnvFilter::new()
        if let Expr::Path(ExprPath { path, .. }) = &*node.func {
            let path_str = path_to_string(path);

            if path_str.ends_with("EnvFilter::new") || path_str == "new" {
                // Check if arguments contain string literals
                for arg in &node.args {
                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(lit_str),
                        ..
                    }) = arg
                    {
                        let span = lit_str.span();
                        let start = span.start();

                        // Check for inline allow comment
                        let allow_check =
                            check_allow_with_reason(self.ctx.content, start.line, NAME);
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
                                        format!(
                                            "Allow directive for '{NAME}' is missing required reason"
                                        ),
                                    )
                                    .with_suggestion(Suggestion::new(
                                        "Add reason=\"...\" to explain why this exception is necessary",
                                    )),
                                );
                            }
                            continue;
                        }

                        let location = Location::new(
                            self.ctx.relative_path.clone(),
                            start.line,
                            start.column + 1,
                        );

                        let level = lit_str.value();
                        self.violations.push(
                            Violation::new(
                                CODE,
                                NAME,
                                self.rule.severity,
                                location,
                                format!("Hardcoded log level `\"{level}\"` in EnvFilter::new()"),
                            )
                            .with_suggestion(Suggestion::new(
                                "Use `EnvFilter::from_default_env()` to allow configuration via RUST_LOG environment variable",
                            )),
                        );
                    }
                }
            }
        }

        syn::visit::visit_expr_call(self, node);
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
        TracingEnvInit::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_hardcoded_debug() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::new("debug");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("debug"));
    }

    #[test]
    fn test_detects_hardcoded_info() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::new("info");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("info"));
    }

    #[test]
    fn test_allows_from_default_env() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::from_default_env();
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_from_env() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::from_env("MY_LOG");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

#[arch_lint::allow(tracing_env_init)]
fn init_test() {
    let filter = EnvFilter::new("debug");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    // arch-lint: allow(tracing-env-init) reason="Test environment with fixed level"
    let filter = EnvFilter::new("debug");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_in_builder_pattern() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();
}
"#,
        );
        assert_eq!(violations.len(), 1);
    }

    // ── env fallback tests (bug fix) ──

    #[test]
    fn test_allows_try_from_default_env_unwrap_or_else_fallback() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
}
"#,
        );
        assert!(
            violations.is_empty(),
            "env fallback should not trigger: {violations:?}"
        );
    }

    #[test]
    fn test_allows_try_from_env_unwrap_or_else_fallback() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let filter = EnvFilter::try_from_env("MY_LOG")
        .unwrap_or_else(|_| EnvFilter::new("warn"));
}
"#,
        );
        assert!(
            violations.is_empty(),
            "env fallback should not trigger: {violations:?}"
        );
    }

    #[test]
    fn test_allows_try_from_default_env_unwrap_or_fallback() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    let default = EnvFilter::new("info");
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or(default);
}
"#,
        );
        // The `EnvFilter::new("info")` on the first line is standalone — not in a fallback.
        // The `.unwrap_or(default)` just passes a variable, no new `EnvFilter::new` inside.
        assert_eq!(
            violations.len(),
            1,
            "standalone EnvFilter::new should still trigger"
        );
    }

    #[test]
    fn test_allows_builder_with_env_fallback() {
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
"#,
        );
        assert!(
            violations.is_empty(),
            "builder with env fallback should not trigger: {violations:?}"
        );
    }

    #[test]
    fn test_detects_hardcoded_without_env_attempt() {
        // EnvFilter::new("info") without try_from_default_env is still a violation
        let violations = check_code(
            r#"
use tracing_subscriber::EnvFilter;

fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("info"))
        .init();
}
"#,
        );
        assert_eq!(violations.len(), 1);
    }
}
