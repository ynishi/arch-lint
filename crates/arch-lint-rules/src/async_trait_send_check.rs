//! Rule to check proper usage of `async_trait` Send bounds.
//!
//! # Rationale
//!
//! The `#[async_trait]` macro from the `async-trait` crate automatically adds
//! `Send` bounds to async trait methods by default. In single-threaded async
//! runtimes or local executors, this `Send` bound is unnecessary and can be
//! overly restrictive.
//!
//! # Detected Patterns
//!
//! - `#[async_trait]` without `?Send` (warns to consider if Send is needed)
//! - Suggests using `#[async_trait(?Send)]` for single-threaded contexts
//!
//! # Good Patterns
//!
//! ```ignore
//! // Single-threaded context - use ?Send
//! #[async_trait(?Send)]
//! trait Handler {
//!     async fn handle(&self);
//! }
//!
//! // Multi-threaded context - explicit Send
//! #[async_trait]
//! trait Service: Send + Sync {
//!     async fn process(&self);
//! }
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::check_arch_lint_allow;
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, ItemMod, ItemTrait};

/// Rule code for async-trait-send-check.
pub const CODE: &str = "AL009";

/// Rule name for async-trait-send-check.
pub const NAME: &str = "async-trait-send-check";

/// Checks proper usage of `async_trait` Send bounds.
#[derive(Debug, Clone)]
pub struct AsyncTraitSendCheck {
    /// Severity level.
    pub severity: Severity,
    /// Runtime mode: "single-thread" or "multi-thread".
    pub runtime_mode: RuntimeMode,
}

/// Runtime execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    /// Single-threaded runtime (e.g., `LocalSet`, wasm).
    SingleThread,
    /// Multi-threaded runtime (e.g., tokio multi-thread).
    MultiThread,
}

impl Default for AsyncTraitSendCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncTraitSendCheck {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            severity: Severity::Warning,
            runtime_mode: RuntimeMode::SingleThread,
        }
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Sets the runtime mode.
    #[must_use]
    pub fn runtime_mode(mut self, mode: RuntimeMode) -> Self {
        self.runtime_mode = mode;
        self
    }
}

impl Rule for AsyncTraitSendCheck {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Checks proper usage of async_trait Send bounds"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        // Skip check for multi-threaded runtime
        if self.runtime_mode == RuntimeMode::MultiThread {
            return Vec::new();
        }

        let mut visitor = AsyncTraitVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct AsyncTraitVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a AsyncTraitSendCheck,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for AsyncTraitVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        // Check if this trait is explicitly allowed
        if self.in_allowed_context || check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            syn::visit::visit_item_trait(self, node);
            return;
        }

        // Check if this trait has #[async_trait]
        if let Some((attr, has_send_opt_out)) = find_async_trait_attr(&node.attrs) {
            // If ?Send is already specified, no violation
            if has_send_opt_out {
                syn::visit::visit_item_trait(self, node);
                return;
            }

            // Check for inline allow comment
            let span = attr.span();
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
                syn::visit::visit_item_trait(self, node);
                return;
            }

            // Report violation
            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            let trait_name = &node.ident;

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!(
                        "Trait `{trait_name}` uses #[async_trait] without ?Send - consider if Send bound is necessary"
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "Use #[async_trait(?Send)] for single-threaded contexts, or keep default for multi-threaded"
                )),
            );
        }

        syn::visit::visit_item_trait(self, node);
    }
}

/// Finds `async_trait` attribute and checks if it has `?Send`.
///
/// Returns `Some((attr, has_send_opt_out))` if found, `None` otherwise.
fn find_async_trait_attr(attrs: &[Attribute]) -> Option<(&Attribute, bool)> {
    for attr in attrs {
        // Check if the path is `async_trait`
        if attr.path().is_ident("async_trait") {
            // Check if arguments contain `?Send`
            let has_send_opt_out = attr
                .meta
                .require_list()
                .ok()
                .map(|meta_list| {
                    // Parse tokens to check for `?Send`
                    let tokens = meta_list.tokens.to_string();
                    tokens.contains("? Send") || tokens.contains("?Send")
                })
                .unwrap_or(false);

            return Some((attr, has_send_opt_out));
        }
    }
    None
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
        AsyncTraitSendCheck::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_async_trait_without_send() {
        let violations = check_code(
            r#"
use async_trait::async_trait;

#[async_trait]
trait MyTrait {
    async fn foo(&self);
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("?Send"));
    }

    #[test]
    fn test_allows_async_trait_with_send_opt_out() {
        let violations = check_code(
            r#"
use async_trait::async_trait;

#[async_trait(?Send)]
trait MyTrait {
    async fn foo(&self);
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_regular_traits() {
        let violations = check_code(
            r#"
trait MyTrait {
    fn foo(&self);
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
use async_trait::async_trait;

#[arch_lint::allow(async_trait_send_check)]
#[async_trait]
trait MyTrait {
    async fn foo(&self);
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_multi_threaded_mode_skips_check() {
        let ast = syn::parse_file(
            r#"
use async_trait::async_trait;

#[async_trait]
trait MyTrait {
    async fn foo(&self);
}
"#,
        )
        .expect("Failed to parse");

        let ctx = FileContext {
            path: Path::new("test.rs"),
            content: "",
            is_test: false,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("test.rs"),
        };

        let rule = AsyncTraitSendCheck::new().runtime_mode(RuntimeMode::MultiThread);
        let violations = rule.check(&ctx, &ast);

        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_traits() {
        let violations = check_code(
            r#"
use async_trait::async_trait;

#[async_trait]
trait Trait1 {
    async fn foo(&self);
}

#[async_trait]
trait Trait2 {
    async fn bar(&self);
}
"#,
        );
        assert_eq!(violations.len(), 2);
    }
}
