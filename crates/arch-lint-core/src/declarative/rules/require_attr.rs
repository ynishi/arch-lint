//! `[[require-attr]]` (ALD006): flags items missing a required attribute.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RequireAttr};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, render_attr, walk_items};

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

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut |view| {
            let rendered: Vec<(String, Vec<String>)> = view.attrs.iter().map(render_attr).collect();

            for rule in &applicable {
                if !rule.applies_to(view.kind) || !rule.visibility().matches(view.is_pub) {
                    continue;
                }
                let present = rule.attrs().iter().any(|pattern| {
                    rendered
                        .iter()
                        .any(|(path, derives)| pattern.matches(path, derives))
                });
                if !present {
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
