//! `[[require-use]]` (ALD002): flags imports of discouraged crates and
//! suggests the preferred one instead.

use std::sync::Arc;

use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RequireUse};
use crate::rule::Rule;
use crate::types::{Location, Severity, Violation};

use super::{expand_use_tree, Decision, Suppressions};

const REQUIRE_USE_NAME: &str = "require-use";
const REQUIRE_USE_CODE: &str = "ALD002";

/// A per-file rule that enforces `[[require-use]]` declarations.
///
/// Flags imports of discouraged crates (`over`) and suggests the
/// preferred crate (`prefer`) instead.
pub struct RequireUseRule {
    config: Arc<DeclarativeConfig>,
}

impl RequireUseRule {
    /// Creates a new require-use rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for RequireUseRule {
    fn name(&self) -> &'static str {
        REQUIRE_USE_NAME
    }

    fn code(&self) -> &'static str {
        REQUIRE_USE_CODE
    }

    fn description(&self) -> &'static str {
        "Require preferred imports over alternatives"
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&RequireUse> = self
            .config
            .require_uses()
            .iter()
            .filter(|r| {
                self.config
                    .scope_ref_contains(r.scope(), &ctx.relative_path)
            })
            .collect();

        if applicable.is_empty() {
            return vec![];
        }

        let scopes = Suppressions::new(
            REQUIRE_USE_NAME,
            REQUIRE_USE_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut visitor = RequireUseVisitor {
            ctx,
            applicable,
            scopes,
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

struct RequireUseVisitor<'a> {
    ctx: &'a FileContext<'a>,
    applicable: Vec<&'a RequireUse>,
    scopes: Suppressions,
    violations: Vec<Violation>,
}

impl<'ast> Visit<'ast> for RequireUseVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let mark = self.scopes.enter_item_mod(&node.attrs);
        syn::visit::visit_item_mod(self, node);
        self.scopes.exit(mark);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let mark = self.scopes.enter_item_fn(&node.attrs);
        syn::visit::visit_item_fn(self, node);
        self.scopes.exit(mark);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let mark = self.scopes.enter_block(&node.attrs);
        syn::visit::visit_item_impl(self, node);
        self.scopes.exit(mark);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            let crate_name = use_item.path.split("::").next().unwrap_or(&use_item.path);

            for (index, rule) in self.applicable.iter().enumerate() {
                if !rule.over().iter().any(|o| o == crate_name) {
                    continue;
                }
                match self.scopes.decide(
                    index,
                    self.ctx,
                    REQUIRE_USE_CODE,
                    use_item.span,
                    Some(&node.attrs),
                ) {
                    Decision::Suppressed => {}
                    Decision::MissingReason(warning) => self.violations.push(*warning),
                    Decision::Report => {
                        let start = use_item.span.start();
                        let location = Location::new(
                            self.ctx.relative_path.clone(),
                            start.line,
                            start.column + 1,
                        );

                        let mut violation = Violation::new(
                            REQUIRE_USE_CODE,
                            rule.name(),
                            rule.severity(),
                            location,
                            format!(
                                "{}: use `{}` instead of `{}`",
                                rule.message(),
                                rule.prefer(),
                                crate_name,
                            ),
                        );
                        if let Some(doc) = rule.doc_ref() {
                            violation = violation.with_doc_ref(doc);
                        }

                        self.violations.push(violation);
                    }
                }
            }
        }

        syn::visit::visit_item_use(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{make_ctx, parse_file};
    use super::*;
    use crate::declarative::model::*;

    fn make_require_config() -> Arc<DeclarativeConfig> {
        Arc::new(
            DeclarativeConfig::new(
                vec![],
                vec![],
                vec![RequireUse::new(
                    "require-tracing".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
                    "tracing".to_string(),
                    vec!["log".to_string()],
                    "Use tracing, not log.".to_string(),
                    None,
                    Severity::Warning,
                )],
                vec![],
            )
            .unwrap(),
        )
    }

    #[test]
    fn require_detects_discouraged_import() {
        let config = make_require_config();
        let rule = RequireUseRule::new(config);
        let code = "use log::info;";
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "require-tracing");
        assert_eq!(violations[0].code, REQUIRE_USE_CODE);
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(violations[0].message.contains("tracing"));
        assert!(violations[0].message.contains("log"));
    }

    #[test]
    fn require_allows_preferred_import() {
        let config = make_require_config();
        let rule = RequireUseRule::new(config);
        let code = "use tracing::info;";
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn require_skips_file_outside_scope() {
        let config = make_require_config();
        let rule = RequireUseRule::new(config);
        let code = "use log::info;";
        // tests/ is not matched by src/**
        let ctx = make_ctx("tests/integration.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn require_detects_multiple_discouraged() {
        let config = Arc::new(
            DeclarativeConfig::new(
                vec![],
                vec![],
                vec![RequireUse::new(
                    "require-tracing".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
                    "tracing".to_string(),
                    vec!["log".to_string(), "env_logger".to_string()],
                    "Use tracing.".to_string(),
                    Some("LOGGING.md".to_string()),
                    Severity::Warning,
                )],
                vec![],
            )
            .unwrap(),
        );
        let rule = RequireUseRule::new(config);
        let code = "use log::info;\nuse env_logger::Builder;";
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].doc_ref.as_deref(), Some("LOGGING.md"));
        assert_eq!(violations[1].doc_ref.as_deref(), Some("LOGGING.md"));
    }

    #[test]
    fn require_allows_unrelated_import() {
        let config = make_require_config();
        let rule = RequireUseRule::new(config);
        let code = "use serde::Serialize;";
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    // ── suppression ──

    fn check_src(code: &str) -> Vec<Violation> {
        let rule = RequireUseRule::new(make_require_config());
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);
        rule.check(&ctx, &ast)
    }

    #[test]
    fn suppress_attribute_on_the_use_item() {
        let code =
            "#[arch_lint::allow(require_tracing, reason = \"third-party bridge\")]\nuse log::info;";
        assert!(check_src(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let code = "#[arch_lint::allow(require_use, reason = \"bridge\")]\nmod bridge {\n    use log::info;\n}";
        assert!(check_src(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_fn() {
        let code =
            "#[arch_lint::allow(ALD002, reason = \"bridge\")]\nfn bridge() {\n    use log::info;\n}";
        assert!(check_src(code).is_empty());
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        for name in ["require-use", "require-tracing", "ALD002"] {
            let code = format!("// arch-lint: allow({name})\nuse log::info;");
            assert!(
                check_src(&code).is_empty(),
                "allow({name}) should suppress the violation"
            );
        }
    }

    #[test]
    fn comment_without_reason_suppresses_a_warning_rule() {
        // `require-tracing` is a warning, so `requires_allow_reason` is false
        // and the missing reason is not reported — as for built-in warnings.
        let code = "// arch-lint: allow(require-tracing)\nuse log::info;";
        assert!(check_src(code).is_empty());
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let config = Arc::new(
            DeclarativeConfig::new(
                vec![],
                vec![],
                vec![RequireUse::new(
                    "require-tracing".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
                    "tracing".to_string(),
                    vec!["log".to_string()],
                    "Use tracing, not log.".to_string(),
                    None,
                    Severity::Error,
                )],
                vec![],
            )
            .unwrap(),
        );
        let rule = RequireUseRule::new(config);
        let code = "// arch-lint: allow(require-tracing)\nuse log::info;";
        let ctx = make_ctx("src/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].code, REQUIRE_USE_CODE);
        assert!(violations[0].message.contains("missing required reason"));
    }
}
