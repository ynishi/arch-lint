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
use arch_lint_core::utils::{path_to_string, ScopeTracker};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{Block, Expr, ExprCall, ExprMethodCall, ExprPath, ItemFn, ItemImpl, ItemMod, Local, Pat};

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

/// Methods that are pure accessors when called on `std::fs::FileType` /
/// `std::fs::Metadata` (no I/O), but perform a `stat` when called on a path.
///
/// Without type information the rule tells the two apart by the receiver's
/// *origin*: a value produced by `file_type()` / `metadata()` /
/// `symlink_metadata()` (see [`yields_metadata`]) is already-fetched metadata.
const METADATA_ACCESSORS: &[&str] = &["is_file", "is_dir"];

/// Wrapper methods that pass a metadata value through unchanged.
const PASSTHROUGH_METHODS: &[&str] = &[
    "unwrap",
    "expect",
    "unwrap_or",
    "unwrap_or_else",
    "unwrap_or_default",
    "ok",
    "as_ref",
    "as_deref",
    "clone",
    "cloned",
    "copied",
];

/// Returns `true` if `expr` evaluates to a `FileType` / `Metadata` value
/// (possibly wrapped in `Option` / `Result`), so that `is_file()` /
/// `is_dir()` on it is a pure accessor rather than a filesystem call.
///
/// Recognized origins: `x.file_type()`, `x.metadata()`, `x.symlink_metadata()`,
/// `fs::metadata(p)`, `fs::symlink_metadata(p)`, each optionally followed by
/// `?`, parentheses, `&`, or one of [`PASSTHROUGH_METHODS`].
fn yields_metadata(expr: &Expr) -> bool {
    match expr {
        Expr::Try(inner) => yields_metadata(&inner.expr),
        Expr::Paren(inner) => yields_metadata(&inner.expr),
        Expr::Reference(inner) => yields_metadata(&inner.expr),
        Expr::Await(inner) => yields_metadata(&inner.base),
        Expr::MethodCall(call) => {
            let name = call.method.to_string();
            match name.as_str() {
                "file_type" | "metadata" | "symlink_metadata" => true,
                other if PASSTHROUGH_METHODS.contains(&other) => yields_metadata(&call.receiver),
                _ => false,
            }
        }
        Expr::Call(call) => match &*call.func {
            Expr::Path(path) => path.path.segments.last().is_some_and(|seg| {
                let ident = seg.ident.to_string();
                ident == "metadata" || ident == "symlink_metadata"
            }),
            _ => false,
        },
        _ => false,
    }
}

/// Collects the identifiers bound by a pattern (`ft`, `(a, b)`, `ft: FileType`, `ref ft`).
fn pattern_idents(pat: &Pat, out: &mut Vec<String>) {
    match pat {
        Pat::Ident(ident) => out.push(ident.ident.to_string()),
        Pat::Type(typed) => pattern_idents(&typed.pat, out),
        Pat::Reference(reference) => pattern_idents(&reference.pat, out),
        Pat::Paren(paren) => pattern_idents(&paren.pat, out),
        Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                pattern_idents(elem, out);
            }
        }
        _ => {}
    }
}

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
            scope: ScopeTracker::new(NAME).with_fn_allow_lints(&[
                "sync_io",
                "startup_io",
                "startup_blocking",
            ]),
            metadata_bindings: Vec::new(),
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct SyncIoVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoSyncIo,
    violations: Vec<Violation>,
    scope: ScopeTracker,
    /// Local names currently bound to a `FileType` / `Metadata` value
    /// (`let ft = entry.file_type()?;`, closure params of
    /// `entry.file_type().is_some_and(|ft| ..)`). Scoped per block.
    metadata_bindings: Vec<String>,
}

impl SyncIoVisitor<'_> {
    /// Whether `receiver` is known to be a `FileType` / `Metadata` value.
    fn receiver_is_metadata(&self, receiver: &Expr) -> bool {
        match receiver {
            Expr::Paren(inner) => self.receiver_is_metadata(&inner.expr),
            Expr::Reference(inner) => self.receiver_is_metadata(&inner.expr),
            Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
                let ident = path.path.segments[0].ident.to_string();
                self.metadata_bindings.contains(&ident) || yields_metadata(receiver)
            }
            other => yields_metadata(other),
        }
    }

    /// Binds the parameters of every closure in `args` as metadata values.
    /// Returns how many names were pushed so the caller can pop them.
    fn bind_closure_params(
        &mut self,
        args: &syn::punctuated::Punctuated<Expr, syn::Token![,]>,
    ) -> usize {
        let before = self.metadata_bindings.len();
        for arg in args {
            if let Expr::Closure(closure) = arg {
                for input in &closure.inputs {
                    pattern_idents(input, &mut self.metadata_bindings);
                }
            }
        }
        self.metadata_bindings.len() - before
    }
}

impl<'ast> Visit<'ast> for SyncIoVisitor<'_> {
    fn visit_block(&mut self, node: &'ast Block) {
        let bindings = self.metadata_bindings.len();
        syn::visit::visit_block(self, node);
        self.metadata_bindings.truncate(bindings);
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if node
            .init
            .as_ref()
            .is_some_and(|init| yields_metadata(&init.expr))
        {
            pattern_idents(&node.pat, &mut self.metadata_bindings);
        }
        syn::visit::visit_local(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let scope = self.scope.enter_item_mod(&node.attrs);
        syn::visit::visit_item_mod(self, node);
        self.scope.exit(scope);
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let scope = self.scope.enter_item_fn(&node.attrs);
        syn::visit::visit_item_fn(self, node);
        self.scope.exit(scope);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let scope = self.scope.enter_item_impl(&node.attrs);
        syn::visit::visit_item_impl(self, node);
        self.scope.exit(scope);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.scope.suppressed() {
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
        if self.scope.suppressed() {
            syn::visit::visit_expr_method_call(self, node);
            return;
        }

        let method_name = node.method.to_string();

        // `is_file()` / `is_dir()` on an already-fetched `FileType` / `Metadata`
        // is a pure accessor, not a filesystem call.
        let pure_accessor = METADATA_ACCESSORS.contains(&method_name.as_str())
            && self.receiver_is_metadata(&node.receiver);

        // Check for forbidden Path methods
        if FORBIDDEN_PATH_METHODS.contains(&method_name.as_str()) && !pure_accessor {
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
            } else {
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
        }

        // Closure parameters fed from a metadata value, e.g.
        // `entry.file_type().is_some_and(|ft| ft.is_file())`, are metadata too.
        let pushed = if yields_metadata(&node.receiver) {
            self.bind_closure_params(&node.args)
        } else {
            0
        };
        syn::visit::visit_expr_method_call(self, node);
        let remaining = self.metadata_bindings.len() - pushed;
        self.metadata_bindings.truncate(remaining);
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
    use arch_lint_core::test_utils::check_rule;

    fn check_code(code: &str) -> Vec<Violation> {
        check_rule(&NoSyncIo::new(), code)
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
    fn test_detects_path_is_file() {
        let violations = check_code(
            r"
fn foo(path: &std::path::Path) -> bool {
    path.is_file()
}
",
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("is_file"));
    }

    #[test]
    fn test_metadata_accessor_via_chain_is_not_io() {
        // `.metadata()` is the I/O call (and is reported); `.is_file()` on
        // its result is a pure accessor.
        let violations = check_code(
            r"
fn foo(path: &std::path::Path) -> std::io::Result<bool> {
    Ok(path.metadata()?.is_file())
}
fn bar(entry: &std::fs::DirEntry) -> bool {
    entry.file_type().unwrap().is_dir()
}
fn baz(path: &std::path::Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}
",
        );
        let messages: Vec<&str> = violations.iter().map(|v| v.message.as_str()).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(
            messages.iter().all(|m| m.contains("metadata")),
            "{messages:?}"
        );
    }

    #[test]
    fn test_metadata_accessor_via_closure_param_is_not_io() {
        let violations = check_code(
            r"
fn foo(entry: &ignore::DirEntry) -> bool {
    entry.file_type().is_some_and(|ft| ft.is_file())
}
",
        );
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn test_metadata_accessor_via_let_binding_is_not_io() {
        let violations = check_code(
            r"
fn foo(entry: &std::fs::DirEntry) -> std::io::Result<bool> {
    let ft = entry.file_type()?;
    let meta: std::fs::Metadata = entry.metadata()?;
    Ok(ft.is_file() && meta.is_dir())
}
",
        );
        // `entry.metadata()` is I/O and stays reported; the accessors do not.
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].message.contains("metadata"));
    }

    #[test]
    fn test_metadata_binding_does_not_leak_out_of_its_block() {
        let violations = check_code(
            r"
fn foo(entry: &std::fs::DirEntry, ft: &std::path::Path) -> std::io::Result<bool> {
    {
        let ft = entry.file_type()?;
        let _ = ft.is_file();
    }
    Ok(ft.is_file())
}
",
        );
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(violations[0].message.contains("is_file"));
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
