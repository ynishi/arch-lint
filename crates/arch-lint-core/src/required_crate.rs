//! Required crate enforcement utilities.
//!
//! This module provides a builder API for creating rules that enforce
//! consistent crate usage across a workspace.
//!
//! # Example
//!
//! ```ignore
//! use arch_lint_core::RequiredCrateRule;
//!
//! let rule = RequiredCrateRule::new("PROJ001", "prefer-utoipa")
//!     .prefer("utoipa")
//!     .over(&["paperclip", "okapi"])
//!     .detect_macro_path()
//!     .build();
//! ```

use crate::utils::allowance::check_allow_with_reason;
use crate::utils::{check_arch_lint_allow, path_to_string};
use crate::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{ItemFn, ItemImpl, ItemMod};

/// Detection pattern for required crate checks.
#[derive(Debug, Clone)]
pub enum DetectionPattern {
    /// Detects macro calls with alternative prefixes.
    ///
    /// Example: Detect `log::info!` when requiring `tracing::info!`
    MacroPath,

    /// Detects type suffixes with derive requirements.
    ///
    /// Example: Detect `*Error` types without `thiserror::Error`
    TypeSuffix {
        /// Suffix to match (e.g., "Error")
        suffix: String,
        /// Expected derive attribute (e.g., "`thiserror::Error`")
        expected_derive: String,
    },

    /// Checks Cargo.toml dependencies (future).
    CargoToml,
}

/// Builder for creating required crate rules.
///
/// This provides a fluent API for defining rules that enforce
/// consistent crate usage patterns.
#[derive(Debug, Clone)]
pub struct RequiredCrateRule {
    code: &'static str,
    name: &'static str,
    description: String,
    preferred: String,
    alternatives: Vec<String>,
    detection: DetectionPattern,
    severity: Severity,
}

impl RequiredCrateRule {
    /// Creates a new required crate rule builder.
    ///
    /// # Arguments
    ///
    /// * `code` - Rule code (e.g., "AL006")
    /// * `name` - Rule name (e.g., "require-tracing")
    #[must_use]
    pub fn new(code: &'static str, name: &'static str) -> Self {
        Self {
            code,
            name,
            description: String::new(),
            preferred: String::new(),
            alternatives: Vec::new(),
            detection: DetectionPattern::MacroPath,
            severity: Severity::Warning,
        }
    }

    /// Sets the required crate name.
    #[must_use]
    pub fn prefer(mut self, crate_name: impl Into<String>) -> Self {
        self.preferred = crate_name.into();
        self
    }

    /// Sets the alternative (discouraged) crate names.
    #[must_use]
    pub fn over(mut self, alternatives: &[&str]) -> Self {
        self.alternatives = alternatives.iter().map(|s| (*s).to_string()).collect();
        self
    }

    /// Sets a custom description.
    #[must_use]
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Uses macro path detection pattern.
    ///
    /// Detects macros like `alternative::macro!()` and suggests
    /// `preferred::macro!()` instead.
    #[must_use]
    pub fn detect_macro_path(mut self) -> Self {
        self.detection = DetectionPattern::MacroPath;
        self
    }

    /// Uses type suffix detection pattern.
    ///
    /// Detects types ending with `suffix` and checks for `expected_derive`.
    #[must_use]
    pub fn detect_type_suffix(
        mut self,
        suffix: impl Into<String>,
        expected_derive: impl Into<String>,
    ) -> Self {
        self.detection = DetectionPattern::TypeSuffix {
            suffix: suffix.into(),
            expected_derive: expected_derive.into(),
        };
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    fn is_alternative_macro(&self, path: &str) -> bool {
        self.alternatives
            .iter()
            .any(|alt| path.starts_with(&format!("{alt}::")))
    }

    fn get_macro_name(&self, path: &str) -> Option<String> {
        for alt in &self.alternatives {
            let prefix = format!("{alt}::");
            if let Some(name) = path.strip_prefix(&prefix) {
                return Some(name.to_string());
            }
        }
        None
    }
}

impl Rule for RequiredCrateRule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn code(&self) -> &'static str {
        self.code
    }

    fn description(&self) -> &'static str {
        // For now, return a static description
        // TODO: Support dynamic descriptions in the future
        "Enforces required crate usage"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        match &self.detection {
            DetectionPattern::MacroPath => {
                let mut visitor = MacroPathVisitor {
                    ctx,
                    rule: self,
                    violations: Vec::new(),
                    in_allowed_context: false,
                };
                visitor.visit_file(ast);
                visitor.violations
            }
            DetectionPattern::TypeSuffix { .. } => {
                // TODO: Implement type suffix detection
                Vec::new()
            }
            DetectionPattern::CargoToml => {
                // TODO: Implement Cargo.toml detection
                Vec::new()
            }
        }
    }
}

struct MacroPathVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a RequiredCrateRule,
    violations: Vec<Violation>,
    in_allowed_context: bool,
}

impl<'ast> Visit<'ast> for MacroPathVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, self.rule.name).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, self.rule.name).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, self.rule.name).is_allowed() {
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

        if self.rule.is_alternative_macro(&path_str) {
            let Some(first_segment) = node.path.segments.first() else {
                syn::visit::visit_macro(self, node);
                return;
            };
            let span = first_segment.ident.span();
            let start = span.start();

            // Check for inline allow comment
            let allow_check = check_allow_with_reason(self.ctx.content, start.line, self.rule.name);
            if allow_check.is_allowed() {
                if self.rule.requires_allow_reason() && allow_check.reason().is_none() {
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);
                    self.violations.push(
                        Violation::new(
                            self.rule.code,
                            self.rule.name,
                            Severity::Warning,
                            location,
                            format!(
                                "Allow directive for '{}' is missing required reason",
                                self.rule.name
                            ),
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

            if let Some(macro_name) = self.rule.get_macro_name(&path_str) {
                self.violations.push(
                    Violation::new(
                        self.rule.code,
                        self.rule.name,
                        self.rule.severity,
                        location,
                        format!(
                            "Use `{}::{macro_name}!` instead of `{path_str}!`",
                            self.rule.preferred
                        ),
                    )
                    .with_suggestion(Suggestion::new(format!(
                        "Replace with `{}::{macro_name}!` for consistency",
                        self.rule.preferred
                    ))),
                );
            }
        }

        syn::visit::visit_macro(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn check_code(rule: &RequiredCrateRule, code: &str) -> Vec<Violation> {
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

    #[test]
    fn test_macro_path_detection() {
        let rule = RequiredCrateRule::new("TEST001", "test-rule")
            .prefer("tracing")
            .over(&["log"])
            .detect_macro_path();

        let violations = check_code(
            &rule,
            r#"
fn foo() {
    log::info!("message");
}
"#,
        );

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("tracing::info"));
    }

    #[test]
    fn test_allows_preferred() {
        let rule = RequiredCrateRule::new("TEST001", "test-rule")
            .prefer("tracing")
            .over(&["log"])
            .detect_macro_path();

        let violations = check_code(
            &rule,
            r#"
fn foo() {
    tracing::info!("message");
}
"#,
        );

        assert!(violations.is_empty());
    }

    #[test]
    fn test_multiple_alternatives() {
        let rule = RequiredCrateRule::new("TEST002", "test-rule")
            .prefer("utoipa")
            .over(&["paperclip", "okapi"])
            .detect_macro_path();

        let violations = check_code(
            &rule,
            r#"
fn foo() {
    paperclip::path!("/api");
    okapi::openapi!();
}
"#,
        );

        assert_eq!(violations.len(), 2);
        assert!(violations[0].message.contains("utoipa::path"));
        assert!(violations[1].message.contains("utoipa::openapi"));
    }

    #[test]
    fn test_severity_setting() {
        let rule = RequiredCrateRule::new("TEST003", "test-rule")
            .prefer("tracing")
            .over(&["log"])
            .severity(Severity::Error)
            .detect_macro_path();

        assert_eq!(rule.default_severity(), Severity::Error);
    }
}
