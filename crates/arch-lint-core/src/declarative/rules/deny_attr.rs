//! `[[deny-attr]]` (ALD005): flags items carrying a denied attribute.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, DenyAttr};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, render_attr, walk_items, Decision, Suppressions};

const DENY_ATTR_NAME: &str = "deny-attr";
const DENY_ATTR_CODE: &str = "ALD005";

/// A per-file rule that enforces `[[deny-attr]]` declarations.
///
/// Flags module-level and impl-level items of the configured kinds that carry
/// one of the denied attributes.
pub struct DenyAttrRule {
    config: Arc<DeclarativeConfig>,
}

impl DenyAttrRule {
    /// Creates a new deny-attr rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for DenyAttrRule {
    fn name(&self) -> &'static str {
        DENY_ATTR_NAME
    }

    fn code(&self) -> &'static str {
        DENY_ATTR_CODE
    }

    fn description(&self) -> &'static str {
        "Deny specified attributes on items within a scope"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&DenyAttr> = self
            .config
            .deny_attrs()
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
            DENY_ATTR_NAME,
            DENY_ATTR_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut scopes, &mut |view, scopes| {
            let rendered: Vec<(String, Vec<String>)> = view.attrs.iter().map(render_attr).collect();

            for (index, rule) in applicable.iter().enumerate() {
                if !rule.applies_to(view.kind) {
                    continue;
                }
                let hit = rule.attrs().iter().find(|pattern| {
                    rendered
                        .iter()
                        .any(|(path, derives)| pattern.matches(path, derives))
                });
                let Some(pattern) = hit else {
                    continue;
                };
                match scopes.decide(index, ctx, DENY_ATTR_CODE, view.span, Some(view.attrs)) {
                    Decision::Suppressed => {}
                    Decision::MissingReason(warning) => violations.push(*warning),
                    Decision::Report => violations.push(build_violation(
                        DENY_ATTR_CODE,
                        rule.name(),
                        rule.severity(),
                        ctx,
                        view.span,
                        format!(
                            "{}: `{} {}` has attribute `{}`",
                            rule.message(),
                            view.kind,
                            view.name,
                            pattern.as_str(),
                        ),
                        rule.doc_ref(),
                    )),
                }
            }
        });
        violations
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{make_ctx, parse_file};
    use super::*;
    use crate::declarative::model::*;
    use crate::types::Severity;

    fn deny_attr_rule(attrs: Vec<&str>, on: Vec<ItemKind>) -> DenyAttrRule {
        let config = DeclarativeConfig::builder()
            .deny_attrs(vec![DenyAttr::new(
                "no-orm-derives-in-domain".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                attrs
                    .into_iter()
                    .map(|a| AttrPattern::new(a).unwrap())
                    .collect(),
                on,
                "Domain types must stay ORM-agnostic.".to_string(),
                None,
                Severity::Error,
            )])
            .build()
            .unwrap();
        DenyAttrRule::new(Arc::new(config))
    }

    #[test]
    fn deny_attr_flags_derive_on_struct() {
        let rule = deny_attr_rule(
            vec!["derive(sqlx::FromRow)"],
            vec![ItemKind::Struct, ItemKind::Enum],
        );
        let violations = crate::test_utils::check_rule(
            &rule,
            "#[derive(Debug, sqlx::FromRow)]\npub struct User;",
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, DENY_ATTR_CODE);
        assert_eq!(violations[0].rule, "no-orm-derives-in-domain");
        assert!(violations[0].message.contains("struct User"));
        assert!(violations[0].message.contains("derive(sqlx::FromRow)"));
    }

    #[test]
    fn deny_attr_reports_once_per_item_even_with_several_matches() {
        let rule = deny_attr_rule(
            vec!["sqlx::FromRow", "serde::Serialize"],
            ItemKind::ALL.to_vec(),
        );
        let violations = crate::test_utils::check_rule(
            &rule,
            "#[derive(sqlx::FromRow, serde::Serialize)]\npub struct User;",
        );
        // reported once per rule, at the item's identifier, naming the first match
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].location.line, 2);
        assert!(violations[0].message.contains("sqlx::FromRow"));
    }

    #[test]
    fn deny_attr_respects_on_filter() {
        let rule = deny_attr_rule(vec!["tracing::instrument"], vec![ItemKind::Struct]);
        let violations =
            crate::test_utils::check_rule(&rule, "#[tracing::instrument]\npub fn handler() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn deny_attr_matches_plain_attribute_path() {
        let rule = deny_attr_rule(vec!["tracing::instrument"], vec![ItemKind::Fn]);
        let violations =
            crate::test_utils::check_rule(&rule, "#[tracing::instrument]\npub fn handler() {}");
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("fn handler"));
    }

    #[test]
    fn deny_attr_ignores_items_inside_fn_body() {
        let rule = deny_attr_rule(vec!["sqlx::FromRow"], ItemKind::ALL.to_vec());
        let violations = crate::test_utils::check_rule(
            &rule,
            "pub fn f() { #[derive(sqlx::FromRow)] struct Inner; }",
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn deny_attr_skips_file_outside_scope() {
        let config = DeclarativeConfig::builder()
            .deny_attrs(vec![DenyAttr::new(
                "no-orm".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("src/domain/**").unwrap()]),
                vec![AttrPattern::new("sqlx::FromRow").unwrap()],
                ItemKind::ALL.to_vec(),
                "msg".to_string(),
                None,
                Severity::Error,
            )])
            .build()
            .unwrap();
        let rule = DenyAttrRule::new(Arc::new(config));
        let code = "#[derive(sqlx::FromRow)]\npub struct User;";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        assert!(rule.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn deny_attr_checks_impl_methods() {
        let rule = deny_attr_rule(vec!["tracing::instrument"], vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\nimpl S {\n    #[tracing::instrument]\n    pub fn go() {}\n}",
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("fn go"));
    }

    // ── suppression ──

    fn struct_rule() -> DenyAttrRule {
        deny_attr_rule(
            vec!["derive(sqlx::FromRow)"],
            vec![ItemKind::Struct, ItemKind::Enum],
        )
    }

    #[test]
    fn suppress_attribute_on_the_item() {
        let violations = crate::test_utils::check_rule(
            &struct_rule(),
            "#[arch_lint::allow(no_orm_derives_in_domain, reason = \"legacy row\")]\n#[derive(sqlx::FromRow)]\npub struct User;",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let violations = crate::test_utils::check_rule(
            &struct_rule(),
            "#[arch_lint::allow(deny_attr, reason = \"legacy rows\")]\npub mod rows {\n    #[derive(sqlx::FromRow)]\n    pub struct User;\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_impl_for_method() {
        let rule = deny_attr_rule(vec!["tracing::instrument"], vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\n#[arch_lint::allow(no_orm_derives_in_domain, reason = \"traced on purpose\")]\nimpl S {\n    #[tracing::instrument]\n    pub fn go() {}\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        for name in ["deny-attr", "no-orm-derives-in-domain", "ALD005"] {
            let code = format!(
                "#[derive(sqlx::FromRow)]\n// arch-lint: allow({name}) reason=\"legacy row\"\npub struct User;"
            );
            let violations = crate::test_utils::check_rule(&struct_rule(), &code);
            assert!(
                violations.is_empty(),
                "allow({name}) should suppress: {violations:#?}"
            );
        }
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let violations = crate::test_utils::check_rule(
            &struct_rule(),
            "#[derive(sqlx::FromRow)]\n// arch-lint: allow(no-orm-derives-in-domain)\npub struct User;",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].code, DENY_ATTR_CODE);
        assert!(violations[0].message.contains("missing required reason"));
    }

    #[test]
    fn user_name_allow_does_not_suppress_a_sibling_rule() {
        let config = DeclarativeConfig::builder()
            .deny_attrs(vec![
                DenyAttr::new(
                    "no-orm-derives-in-domain".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                    vec![AttrPattern::new("derive(sqlx::FromRow)").unwrap()],
                    vec![ItemKind::Struct],
                    "Domain types must stay ORM-agnostic.".to_string(),
                    None,
                    Severity::Error,
                ),
                DenyAttr::new(
                    "no-derive-macros-in-domain".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                    vec![AttrPattern::new("derive(sqlx::FromRow)").unwrap()],
                    vec![ItemKind::Struct],
                    "Domain types derive nothing external.".to_string(),
                    None,
                    Severity::Error,
                ),
            ])
            .build()
            .unwrap();
        let rule = DenyAttrRule::new(Arc::new(config));

        let violations = crate::test_utils::check_rule(
            &rule,
            "#[arch_lint::allow(no_orm_derives_in_domain, reason = \"legacy row\")]\n#[derive(sqlx::FromRow)]\npub struct User;",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].rule, "no-derive-macros-in-domain");
    }

    #[test]
    fn cfg_test_module_is_still_checked() {
        let violations = crate::test_utils::check_rule(
            &struct_rule(),
            "#[cfg(test)]\nmod tests {\n    #[derive(sqlx::FromRow)]\n    pub struct Row;\n}",
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
    }
}
