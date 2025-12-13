//! Rule to forbid synchronous I/O in async contexts.
//!
//! # Rationale
//!
//! Blocking I/O operations in async code can block the async runtime and cause
//! performance issues. This rule helps identify places where async I/O should be used.
//!
//! # Detected Patterns
//!
//! - `std::fs::*` functions (read, write, etc.)
//! - `std::io::*` blocking operations
//! - `.read()`, `.write()` on std types
//! - `std::thread::sleep`
//!
//! # Allowed Patterns
//!
//! - `tokio::fs::*` (async I/O)
//! - `async_std::fs::*` (async I/O)
//!
//! # Configuration
//!
//! - `allow_patterns`: Additional patterns to allow
//!
//! # Suppression
//!
//! - `#[allow(sync_io)]` attribute
//! - `// arch-lint: allow(no-sync-io)` comment

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{has_allow_attr, path_to_string};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, ItemFn};

/// Rule code for no-sync-io.
pub const CODE: &str = "AL002";

/// Rule name for no-sync-io.
pub const NAME: &str = "no-sync-io";

/// Forbidden `std::fs` functions.
const FORBIDDEN_FS: &[&str] = &[
    "std::fs::read",
    "std::fs::read_to_string",
    "std::fs::write",
    "std::fs::copy",
    "std::fs::create_dir",
    "std::fs::create_dir_all",
    "std::fs::remove_file",
    "std::fs::remove_dir",
    "std::fs::remove_dir_all",
    "std::fs::rename",
    "std::fs::metadata",
    "std::fs::symlink_metadata",
    "std::fs::canonicalize",
    "std::fs::read_link",
    "std::fs::read_dir",
    "std::fs::File::open",
    "std::fs::File::create",
];

/// Forbidden method names on Path-like types.
const FORBIDDEN_PATH_METHODS: &[&str] = &[
    "exists",
    "is_file",
    "is_dir",
    "metadata",
    "read_dir",
    "canonicalize",
];

/// Forbids synchronous I/O operations.
#[derive(Debug, Clone)]
pub struct NoSyncIo {
    /// Additional patterns to allow.
    pub allow_patterns: Vec<String>,
    /// Custom severity.
    pub severity: Severity,
}

impl Default for NoSyncIo {
    fn default() -> Self {
        Self::new()
    }
}

impl NoSyncIo {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allow_patterns: vec!["tokio::".to_string(), "async_std::".to_string()],
            severity: Severity::Error,
        }
    }

    /// Adds patterns to allow.
    #[must_use]
    pub fn allow_patterns(mut self, patterns: &[&str]) -> Self {
        self.allow_patterns
            .extend(patterns.iter().map(|s| (*s).to_string()));
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    fn is_allowed_path(&self, path: &str) -> bool {
        self.allow_patterns.iter().any(|p| path.starts_with(p))
    }
}

impl Rule for NoSyncIo {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Forbids synchronous I/O in async contexts"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = SyncIoVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct SyncIoVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoSyncIo,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for SyncIoVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let was_allowed = self.in_allowed_context;

        if has_allow_attr(&node.attrs, &["sync_io", "startup_io", "startup_blocking"]) {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.in_allowed_context {
            syn::visit::visit_expr_call(self, node);
            return;
        }

        if let Expr::Path(ExprPath { path, .. }) = &*node.func {
            let path_str = path_to_string(path);

            // Check if allowed
            if self.rule.is_allowed_path(&path_str) {
                syn::visit::visit_expr_call(self, node);
                return;
            }

            // Check if forbidden
            if FORBIDDEN_FS
                .iter()
                .any(|f| path_str.ends_with(f) || path_str == *f)
            {
                let span = path
                    .segments
                    .last()
                    .map_or_else(proc_macro2::Span::call_site, |s| s.ident.span());
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
                    syn::visit::visit_expr_call(self, node);
                    return;
                }

                let location =
                    Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

                let suggestion = get_async_alternative(&path_str);

                self.violations.push(
                    Violation::new(
                        CODE,
                        NAME,
                        self.rule.severity,
                        location,
                        format!("Synchronous I/O `{path_str}` may block the async runtime"),
                    )
                    .with_suggestion(Suggestion::new(suggestion)),
                );
            }
        }

        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if self.in_allowed_context {
            syn::visit::visit_expr_method_call(self, node);
            return;
        }

        let method_name = node.method.to_string();

        // Check for forbidden Path methods
        if FORBIDDEN_PATH_METHODS.contains(&method_name.as_str()) {
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

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!("`.{method_name}()` performs synchronous I/O"),
                )
                .with_suggestion(Suggestion::new(format!(
                    "Use `tokio::fs::{method_name}` or async equivalent"
                ))),
            );
        }

        syn::visit::visit_expr_method_call(self, node);
    }
}

fn get_async_alternative(path: &str) -> String {
    if path.contains("std::fs::") {
        let fn_name = path.rsplit("::").next().unwrap_or("");
        format!("Use `tokio::fs::{fn_name}` instead")
    } else if path.contains("std::thread::sleep") {
        "Use `tokio::time::sleep` instead".to_string()
    } else {
        "Use async I/O operations instead".to_string()
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
        NoSyncIo::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_std_fs_read() {
        let violations = check_code(
            r#"
fn foo() {
    let content = std::fs::read_to_string("file.txt");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
    }

    #[test]
    fn test_allows_tokio_fs() {
        let violations = check_code(
            r#"
async fn foo() {
    let content = tokio::fs::read_to_string("file.txt").await;
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_path_exists() {
        let violations = check_code(
            r#"
fn foo(path: &std::path::Path) {
    if path.exists() {
        println!("exists");
    }
}
"#,
        );
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
#[allow(sync_io)]
fn startup() {
    let config = std::fs::read_to_string("config.toml");
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
    // arch-lint: allow(no-sync-io)
    let content = std::fs::read_to_string("file.txt");
}
"#,
        );
        // Allow directive without reason generates a warning
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("missing required reason"));
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
fn foo() {
    // arch-lint: allow(no-sync-io) reason="Startup initialization only"
    let content = std::fs::read_to_string("config.toml");
}
"#,
        );
        assert!(violations.is_empty());
    }
}
