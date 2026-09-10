//! `[[require-attr]]` (ALD006): flags items missing a required attribute.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RequireAttr};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, render_attr, walk_items, Decision, Suppressions};

const REQUIRE_ATTR_NAME: &str = "require-attr";
const REQUIRE_ATTR_CODE: &str = "ALD006";

/// A per-file rule that enforces `[[require-attr]]` declarations.
///
/// Flags items of the configured kinds and visibility that carry none of the
/// required attributes.
pub struct RequireAttrRule {
    config: Arc<DeclarativeConfig>,
}

impl RequireAttrRule {
    /// Creates a new require-attr rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for RequireAttrRule {
    fn name(&self) -> &'static str {
        REQUIRE_ATTR_NAME
    }

    fn code(&self) -> &'static str {
        REQUIRE_ATTR_CODE
    }

    fn description(&self) -> &'static str {
        "Require specified attributes on items within a scope"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&RequireAttr> = self
            .config
            .require_attrs()
            .iter()
            .filter(|r| {
                self.config
                    .scope_ref_contains(r.scope(), &ctx.relative_path)
            })
            .collect();

        if applicable.is_empty() {
            return vec![];
        }

        let mut scopes = Suppressions::new(
            REQUIRE_ATTR_NAME,
            REQUIRE_ATTR_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut scopes, &mut |view, scopes| {
            let rendered: Vec<(String, Vec<String>)> = view.attrs.iter().map(render_attr).collect();

            for (index, rule) in applicable.iter().enumerate() {
                if !rule.applies_to(view.kind) || !rule.visibility().matches(view.is_pub) {
                    continue;
                }
                let present = rule.attrs().iter().any(|pattern| {
                    rendered
                        .iter()
                        .any(|(path, derives)| pattern.matches(path, derives))
                });
                if present {
                    continue;
                }
                match scopes.decide(index, ctx, REQUIRE_ATTR_CODE, view.span, Some(view.attrs)) {
                    Decision::Suppressed => {}
                    Decision::MissingReason(warning) => violations.push(*warning),
                    Decision::Report => {
                        let expected = rule
                            .attrs()
                            .iter()
                            .map(|p| format!("`{}`", p.as_str()))
                            .collect::<Vec<_>>()
                            .join(" or ");
                        violations.push(build_violation(
                            REQUIRE_ATTR_CODE,
                            rule.name(),
                            rule.severity(),
                            ctx,
                            view.span,
                            format!(
                                "{}: `{} {}` is missing {}",
                                rule.message(),
                                view.kind,
                                view.name,
                                expected,
                            ),
                            rule.doc_ref(),
                        ));
                    }
                }
            }
        });
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::*;
    use crate::types::Severity;

    fn require_attr_rule(visibility: VisibilityFilter, on: Vec<ItemKind>) -> RequireAttrRule {
        let config = DeclarativeConfig::builder()
            .require_attrs(vec![RequireAttr::new(
                "instrument-handlers".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                vec![AttrPattern::new("tracing::instrument").unwrap()],
                on,
                "Handlers must be instrumented.".to_string(),
                None,
                Severity::Warning,
            )
            .with_visibility(visibility)])
            .build()
            .unwrap();
        RequireAttrRule::new(Arc::new(config))
    }

    #[test]
    fn require_attr_flags_missing_attribute() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "pub fn handler() {}");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, REQUIRE_ATTR_CODE);
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(violations[0].message.contains("`tracing::instrument`"));
    }

    #[test]
    fn require_attr_accepts_present_attribute() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations =
            crate::test_utils::check_rule(&rule, "#[tracing::instrument]\npub fn handler() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn require_attr_pub_visibility_skips_private_fn() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "fn helper() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn require_attr_any_visibility_flags_private_fn() {
        let rule = require_attr_rule(VisibilityFilter::Any, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "fn helper() {}");
        assert_eq!(violations.len(), 1);
    }

    // ── suppression ──

    #[test]
    fn suppress_attribute_on_the_item() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "#[arch_lint::allow(instrument_handlers, reason = \"hot path\")]\npub fn handler() {}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "#[arch_lint::allow(require_attr, reason = \"hot path\")]\npub mod fast {\n    pub fn handler() {}\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_impl() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\n#[arch_lint::allow(ALD006, reason = \"hot path\")]\nimpl S {\n    pub fn handler() {}\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        for name in ["require-attr", "instrument-handlers", "ALD006"] {
            let code = format!("// arch-lint: allow({name})\npub fn handler() {{}}");
            let violations = crate::test_utils::check_rule(&rule, &code);
            assert!(
                violations.is_empty(),
                "allow({name}) should suppress: {violations:#?}"
            );
        }
    }

    #[test]
    fn comment_without_reason_suppresses_a_warning_rule() {
        // `instrument-handlers` is a warning, so no reason is required — the
        // same formula the built-in rules use via `requires_allow_reason`.
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "// arch-lint: allow(ALD006)\npub fn handler() {}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let config = DeclarativeConfig::builder()
            .require_attrs(vec![RequireAttr::new(
                "instrument-handlers".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                vec![AttrPattern::new("tracing::instrument").unwrap()],
                vec![ItemKind::Fn],
                "Handlers must be instrumented.".to_string(),
                None,
                Severity::Error,
            )
            .with_visibility(VisibilityFilter::Pub)])
            .build()
            .unwrap();
        let rule = RequireAttrRule::new(Arc::new(config));

        let violations = crate::test_utils::check_rule(
            &rule,
            "// arch-lint: allow(instrument-handlers)\npub fn handler() {}",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(violations[0].message.contains("missing required reason"));
    }

    #[test]
    fn require_attr_treats_trait_impl_methods_as_public() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\ntrait T { fn go(); }\nimpl T for S {\n    fn go() {}\n}",
        );
        // the trait declaration is private (`trait T`), the impl method counts as public
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].location.line, 4);
    }
}
