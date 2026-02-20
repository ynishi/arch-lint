//! Rule to forbid silently discarding `Result` error information.
//!
//! # Rationale
//!
//! Methods like `.unwrap_or()`, `.unwrap_or_default()`, `.unwrap_or_else()`,
//! and `.ok()` on `Result` silently discard the `Err` variant. Unlike `.unwrap()`
//! (which panics and is caught by AL001), these compile without warning and
//! produce subtle data-loss bugs — e.g. `version.workspace = true` silently
//! falling back to `"0.1.0"`.
//!
//! # Detected Patterns
//!
//! ```ignore
//! // BAD: Error silently replaced with default
//! let v = result.unwrap_or("fallback".to_owned());
//! let v = result.unwrap_or_default();
//! let v = result.unwrap_or_else(|| compute_default());
//!
//! // BAD: Err information erased
//! let opt = result.ok();
//!
//! // BAD: Result explicitly discarded
//! let _ = fallible_operation();
//! ```
//!
//! # Good Patterns
//!
//! ```ignore
//! // GOOD: Propagate the error
//! let v = result?;
//!
//! // GOOD: Handle with explicit match and log/recover
//! let v = match result {
//!     Ok(v) => v,
//!     Err(e) => {
//!         tracing::warn!(error = %e, "falling back to default");
//!         default_value()
//!     }
//! };
//!
//! // GOOD: Map error to a different error type
//! let v = result.map_err(|e| MyError::from(e))?;
//! ```
//!
//! # Configuration
//!
//! - `allow_in_tests`: Allow in test code (default: true)
//! - `allow_ok`: Allow `.ok()` conversion (default: false)
//! - `allow_let_underscore`: Allow `let _ = ...` (default: false)

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{check_arch_lint_allow, has_allow_attr, has_cfg_test, has_test_attr};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{ExprMethodCall, ItemFn, ItemImpl, ItemMod, Local, Pat};

/// Rule code for no-silent-result-drop.
pub const CODE: &str = "AL013";

/// Rule name for no-silent-result-drop.
pub const NAME: &str = "no-silent-result-drop";

/// Method names that silently discard the `Err` variant of a `Result`.
const SILENT_DROP_METHODS: &[&str] = &["unwrap_or", "unwrap_or_default", "unwrap_or_else", "ok"];

/// Forbids silently discarding `Result` error information.
#[derive(Debug, Clone)]
pub struct NoSilentResultDrop {
    /// Allow in test code.
    pub allow_in_tests: bool,
    /// Allow `.ok()` conversion (less strict mode).
    pub allow_ok: bool,
    /// Allow `let _ = expr` pattern (less strict mode).
    pub allow_let_underscore: bool,
    /// Custom severity.
    pub severity: Severity,
}

impl Default for NoSilentResultDrop {
    fn default() -> Self {
        Self::new()
    }
}

impl NoSilentResultDrop {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allow_in_tests: true,
            allow_ok: false,
            allow_let_underscore: false,
            severity: Severity::Warning,
        }
    }

    /// Sets whether to allow in test code.
    #[must_use]
    pub fn allow_in_tests(mut self, allow: bool) -> Self {
        self.allow_in_tests = allow;
        self
    }

    /// Sets whether to allow `.ok()` conversion.
    #[must_use]
    pub fn allow_ok(mut self, allow: bool) -> Self {
        self.allow_ok = allow;
        self
    }

    /// Sets whether to allow `let _ = ...` pattern.
    #[must_use]
    pub fn allow_let_underscore(mut self, allow: bool) -> Self {
        self.allow_let_underscore = allow;
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl Rule for NoSilentResultDrop {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Forbids silently discarding Result error information"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        if self.allow_in_tests && ctx.is_test {
            return Vec::new();
        }

        let mut visitor = SilentResultDropVisitor {
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

struct SilentResultDropVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoSilentResultDrop,
    violations: Vec<Violation>,
    in_test_context: bool,
    in_allowed_context: bool,
}

impl SilentResultDropVisitor<'_> {
    fn is_skipped(&self) -> bool {
        self.in_allowed_context || (self.rule.allow_in_tests && self.in_test_context)
    }

    fn report_method_violation(&mut self, method_name: &str, span: proc_macro2::Span) {
        let start = span.start();

        // Check for inline allow comment
        let allow_check = check_allow_with_reason(self.ctx.content, start.line, NAME);
        if allow_check.is_allowed() {
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

        let (message, suggestion) = match method_name {
            "unwrap_or" => (
                ".unwrap_or() silently discards the error — use `match` or `?` to handle it"
                    .to_owned(),
                Suggestion::new(
                    "Use `match` with explicit error handling, or propagate with `?`",
                ),
            ),
            "unwrap_or_default" => (
                ".unwrap_or_default() silently discards the error — use `match` or `?` to handle it"
                    .to_owned(),
                Suggestion::new(
                    "Use `match` with explicit error handling, or propagate with `?`",
                ),
            ),
            "unwrap_or_else" => (
                ".unwrap_or_else() silently discards the error — use `match` or `?` to handle it"
                    .to_owned(),
                Suggestion::new(
                    "Use `match` with explicit error handling, or propagate with `?`",
                ),
            ),
            "ok" => (
                ".ok() erases error information by converting Result to Option".to_owned(),
                Suggestion::new(
                    "Propagate with `?`, or use `match` to handle both variants explicitly",
                ),
            ),
            _ => unreachable!("only called for known methods"),
        };

        self.violations.push(
            Violation::new(CODE, NAME, self.rule.severity, location, message)
                .with_suggestion(suggestion),
        );
    }

    fn report_let_underscore_violation(&mut self, span: proc_macro2::Span) {
        let start = span.start();

        let allow_check = check_allow_with_reason(self.ctx.content, start.line, NAME);
        if allow_check.is_allowed() {
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
                "`let _ = ...` discards a Result without inspecting the error",
            )
            .with_suggestion(Suggestion::new(
                "Handle the error explicitly, or use `if let Err(e) = ...` to at least log it",
            )),
        );
    }
}

impl<'ast> Visit<'ast> for SilentResultDropVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_in_test = self.in_test_context;
        let was_allowed = self.in_allowed_context;

        if has_cfg_test(&node.attrs) {
            self.in_test_context = true;
        }
        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);

        self.in_test_context = was_in_test;
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let was_in_test = self.in_test_context;
        let was_allowed = self.in_allowed_context;

        if has_test_attr(&node.attrs) {
            self.in_test_context = true;
        }
        if has_allow_attr(&node.attrs, &["clippy::let_underscore_must_use"]) {
            self.in_allowed_context = true;
        }
        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);

        self.in_test_context = was_in_test;
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
        if self.is_skipped() {
            syn::visit::visit_expr_method_call(self, node);
            return;
        }

        let method_name = node.method.to_string();

        // Check if this is a silent-drop method
        if SILENT_DROP_METHODS.contains(&method_name.as_str()) {
            // Skip .ok() if configured to allow
            if method_name == "ok" && self.rule.allow_ok {
                syn::visit::visit_expr_method_call(self, node);
                return;
            }

            self.report_method_violation(&method_name, node.method.span());
        }

        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if self.is_skipped() || self.rule.allow_let_underscore {
            syn::visit::visit_local(self, node);
            return;
        }

        // Detect `let _ = expr;` pattern
        if let Pat::Wild(wild) = &node.pat {
            if node.init.is_some() {
                self.report_let_underscore_violation(wild.underscore_token.span);
            }
        }

        syn::visit::visit_local(self, node);
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
        NoSilentResultDrop::new().check(&ctx, &ast)
    }

    fn check_code_with(code: &str, rule: NoSilentResultDrop) -> Vec<Violation> {
        let ast = syn::parse_file(code).expect("Failed to parse");
        let ctx = FileContext {
            path: Path::new("test.rs"),
            content: code,
            is_test: false,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("test.rs"),
        };
        rule.check(&ctx, &ast)
    }

    fn check_test_code(code: &str) -> Vec<Violation> {
        let ast = syn::parse_file(code).expect("Failed to parse");
        let ctx = FileContext {
            path: Path::new("tests/test.rs"),
            content: code,
            is_test: true,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("tests/test.rs"),
        };
        NoSilentResultDrop::new().check(&ctx, &ast)
    }

    // ── .unwrap_or() ──

    #[test]
    fn detects_unwrap_or() {
        let violations = check_code(
            r#"
fn foo() -> String {
    let result: Result<String, Error> = try_parse();
    result.unwrap_or("default".to_owned())
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("unwrap_or"));
    }

    // ── .unwrap_or_default() ──

    #[test]
    fn detects_unwrap_or_default() {
        let violations = check_code(
            r#"
fn foo() -> String {
    let result: Result<String, Error> = try_parse();
    result.unwrap_or_default()
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unwrap_or_default"));
    }

    // ── .unwrap_or_else() ──

    #[test]
    fn detects_unwrap_or_else() {
        let violations = check_code(
            r#"
fn foo() -> String {
    let result: Result<String, Error> = try_parse();
    result.unwrap_or_else(|_| "fallback".to_owned())
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unwrap_or_else"));
    }

    // ── .ok() ──

    #[test]
    fn detects_ok() {
        let violations = check_code(
            r#"
fn foo() -> Option<String> {
    let result: Result<String, Error> = try_parse();
    result.ok()
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains(".ok()"));
    }

    #[test]
    fn allow_ok_config_skips_ok() {
        let violations = check_code_with(
            r#"
fn foo() -> Option<String> {
    let result: Result<String, Error> = try_parse();
    result.ok()
}
"#,
            NoSilentResultDrop::new().allow_ok(true),
        );
        assert!(violations.is_empty());
    }

    // ── let _ = ... ──

    #[test]
    fn detects_let_underscore() {
        let violations = check_code(
            r#"
fn foo() {
    let _ = fallible_operation();
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("let _ = ..."));
    }

    #[test]
    fn allow_let_underscore_config_skips() {
        let violations = check_code_with(
            r#"
fn foo() {
    let _ = fallible_operation();
}
"#,
            NoSilentResultDrop::new().allow_let_underscore(true),
        );
        assert!(violations.is_empty());
    }

    // ── Test context ──

    #[test]
    fn allows_in_test_file() {
        let violations = check_test_code(
            r#"
fn test_foo() {
    let _ = fallible_operation();
    result.unwrap_or_default();
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_in_test_fn() {
        let violations = check_code(
            r#"
#[test]
fn test_foo() {
    result.unwrap_or_default();
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_in_cfg_test_mod() {
        let violations = check_code(
            r#"
#[cfg(test)]
mod tests {
    fn helper() {
        result.unwrap_or_default();
    }
}
"#,
        );
        assert!(violations.is_empty());
    }

    // ── Suppression ──

    #[test]
    fn allows_with_arch_lint_comment_and_reason() {
        let violations = check_code(
            r#"
fn foo() -> String {
    // arch-lint: allow(no-silent-result-drop) reason="Option is the correct domain type here"
    result.ok().unwrap_or_default()
}
"#,
        );
        // .ok() is allowed by the comment, but .unwrap_or_default() on the next call
        // is also on the same line so it's also covered
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_with_arch_lint_attr() {
        let violations = check_code(
            r#"
#[arch_lint::allow(no_silent_result_drop, reason = "Intentional conversion")]
fn foo() -> Option<String> {
    result.ok()
}
"#,
        );
        assert!(violations.is_empty());
    }

    // ── Good patterns (no false positives) ──

    #[test]
    fn allows_question_mark_operator() {
        let violations = check_code(
            r#"
fn foo() -> Result<String, Error> {
    let v = try_parse()?;
    Ok(v)
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_match_with_explicit_handling() {
        let violations = check_code(
            r#"
fn foo() -> String {
    match try_parse() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "falling back");
            "default".to_owned()
        }
    }
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_map_err() {
        let violations = check_code(
            r#"
fn foo() -> Result<String, MyError> {
    let v = try_parse().map_err(MyError::from)?;
    Ok(v)
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_unwrap_or_on_option() {
        // unwrap_or on Option is fine — no error to lose
        let violations = check_code(
            r#"
fn foo() -> String {
    let opt: Option<String> = None;
    opt.unwrap_or("default".to_owned())
}
"#,
        );
        // Note: syn-based analysis cannot distinguish Option from Result at AST level.
        // This is a known limitation — the rule triggers on the method name regardless
        // of receiver type. Users should use `allow` directives for Option chains.
        // This is by design: false positives are preferable to silent data loss.
        assert_eq!(violations.len(), 1);
    }

    // ── Multiple violations ──

    #[test]
    fn detects_multiple_violations_in_one_fn() {
        let violations = check_code(
            r#"
fn foo() {
    let a = result1.unwrap_or(0);
    let b = result2.unwrap_or_default();
    let _ = result3;
    let c = result4.ok();
}
"#,
        );
        assert_eq!(violations.len(), 4);
    }

    // ── Chained calls ──

    #[test]
    fn detects_chained_ok_unwrap_or() {
        let violations = check_code(
            r#"
fn foo() -> String {
    result.ok().unwrap_or("default".to_owned())
}
"#,
        );
        // Both .ok() and .unwrap_or() trigger
        assert_eq!(violations.len(), 2);
    }

    // ── Named binding (not wildcard) is fine ──

    #[test]
    fn allows_named_let_binding() {
        let violations = check_code(
            r#"
fn foo() {
    let _result = fallible_operation();
}
"#,
        );
        // `let _result = ...` is different from `let _ = ...`
        // The former keeps the value alive; the latter drops immediately
        assert!(violations.is_empty());
    }
}
