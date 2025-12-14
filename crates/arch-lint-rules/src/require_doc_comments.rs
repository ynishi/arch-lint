//! Rule to require documentation comments on public items.
//!
//! # Rationale
//!
//! Public APIs should be documented to help users understand how to use them.
//! Documentation improves code maintainability and makes `cargo doc` output useful.
//!
//! # Detected Patterns
//!
//! - Public functions without `///` or `//!` comments
//! - Public structs without documentation
//! - Public enums without documentation
//!
//! # Good Patterns
//!
//! ```ignore
//! /// Processes the input data and returns the result.
//! ///
//! /// # Errors
//! /// Returns `ProcessError` if the input is invalid.
//! pub fn process_data(input: &[u8]) -> Result<Output, ProcessError> {
//!     // ...
//! }
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::check_arch_lint_allow;
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
#[allow(unused_imports)]
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, ItemEnum, ItemFn, ItemMod, ItemStruct, Visibility};

/// Rule code for require-doc-comments.
pub const CODE: &str = "AL012";

/// Rule name for require-doc-comments.
pub const NAME: &str = "require-doc-comments";

/// Requires documentation comments on public items.
#[derive(Debug, Clone)]
pub struct RequireDocComments {
    /// Custom severity.
    pub severity: Severity,
    /// Require docs for public functions.
    pub require_fn_docs: bool,
    /// Require docs for public structs.
    pub require_struct_docs: bool,
    /// Require docs for public enums.
    pub require_enum_docs: bool,
}

impl Default for RequireDocComments {
    fn default() -> Self {
        Self::new()
    }
}

impl RequireDocComments {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            severity: Severity::Warning,
            require_fn_docs: true,
            require_struct_docs: true,
            require_enum_docs: true,
        }
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Sets whether to require docs for public functions.
    #[must_use]
    pub fn require_fn_docs(mut self, require: bool) -> Self {
        self.require_fn_docs = require;
        self
    }

    /// Sets whether to require docs for public structs.
    #[must_use]
    pub fn require_struct_docs(mut self, require: bool) -> Self {
        self.require_struct_docs = require;
        self
    }

    /// Sets whether to require docs for public enums.
    #[must_use]
    pub fn require_enum_docs(mut self, require: bool) -> Self {
        self.require_enum_docs = require;
        self
    }
}

impl Rule for RequireDocComments {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Requires documentation comments on public items"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = DocCommentsVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct DocCommentsVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a RequireDocComments,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl DocCommentsVisitor<'_> {
    /// Checks if an item has documentation.
    fn has_doc_comment(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| {
            // Check for #[doc = "..."] attribute
            attr.path().is_ident("doc")
        })
    }

    /// Checks if visibility is public.
    fn is_public(vis: &Visibility) -> bool {
        matches!(vis, Visibility::Public(_))
    }

    /// Reports a missing documentation violation.
    fn report_missing_doc(
        &mut self,
        item_type: &str,
        item_name: &str,
        span: proc_macro2::Span,
        attrs: &[Attribute],
    ) {
        if self.in_allowed_context {
            return;
        }

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

        // Check for attribute-level allow
        if check_arch_lint_allow(attrs, NAME).is_allowed() {
            return;
        }

        let location = Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

        self.violations.push(
            Violation::new(
                CODE,
                NAME,
                self.rule.severity,
                location,
                format!("Public {item_type} `{item_name}` is missing documentation"),
            )
            .with_suggestion(Suggestion::new(
                "Add documentation using `///` comments above the item",
            )),
        );
    }
}

impl<'ast> Visit<'ast> for DocCommentsVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if self.rule.require_fn_docs
            && Self::is_public(&node.vis)
            && !Self::has_doc_comment(&node.attrs)
        {
            let name = &node.sig.ident;
            self.report_missing_doc("function", &name.to_string(), name.span(), &node.attrs);
        }

        syn::visit::visit_item_fn(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        if self.rule.require_struct_docs
            && Self::is_public(&node.vis)
            && !Self::has_doc_comment(&node.attrs)
        {
            let name = &node.ident;
            self.report_missing_doc("struct", &name.to_string(), name.span(), &node.attrs);
        }

        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        if self.rule.require_enum_docs
            && Self::is_public(&node.vis)
            && !Self::has_doc_comment(&node.attrs)
        {
            let name = &node.ident;
            self.report_missing_doc("enum", &name.to_string(), name.span(), &node.attrs);
        }

        syn::visit::visit_item_enum(self, node);
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
        RequireDocComments::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_undocumented_pub_fn() {
        let violations = check_code(
            r#"
pub fn process_data() {
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("function"));
        assert!(violations[0].message.contains("process_data"));
    }

    #[test]
    fn test_allows_documented_pub_fn() {
        let violations = check_code(
            r#"
/// Processes the data.
pub fn process_data() {
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_undocumented_pub_struct() {
        let violations = check_code(
            r#"
pub struct Config {
    name: String,
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("struct"));
        assert!(violations[0].message.contains("Config"));
    }

    #[test]
    fn test_allows_documented_pub_struct() {
        let violations = check_code(
            r#"
/// Configuration data.
pub struct Config {
    name: String,
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_undocumented_pub_enum() {
        let violations = check_code(
            r#"
pub enum Status {
    Active,
    Inactive,
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("enum"));
        assert!(violations[0].message.contains("Status"));
    }

    #[test]
    fn test_allows_documented_pub_enum() {
        let violations = check_code(
            r#"
/// Status of the system.
pub enum Status {
    Active,
    Inactive,
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_ignores_private_items() {
        let violations = check_code(
            r#"
fn private_fn() {
}

struct PrivateStruct {
}

enum PrivateEnum {
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
#[arch_lint::allow(require_doc_comments)]
pub fn undocumented() {
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
// arch-lint: allow(require-doc-comments) reason="Internal implementation detail"
pub fn undocumented() {
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_undocumented() {
        let violations = check_code(
            r#"
pub fn foo() {}
pub fn bar() {}
pub struct Baz;
"#,
        );
        assert_eq!(violations.len(), 3);
    }
}
