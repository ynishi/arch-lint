//! `[[naming]]` (ALD007): flags identifiers that do not match the pattern.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, Naming};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, walk_items, Decision, Suppressions};

const NAMING_NAME: &str = "naming";
const NAMING_CODE: &str = "ALD007";

/// A per-file rule that enforces `[[naming]]` declarations.
///
/// Flags identifiers of the configured item kind that do not match the regex.
pub struct NamingRule {
    config: Arc<DeclarativeConfig>,
}

impl NamingRule {
    /// Creates a new naming rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for NamingRule {
    fn name(&self) -> &'static str {
        NAMING_NAME
    }

    fn code(&self) -> &'static str {
        NAMING_CODE
    }

    fn description(&self) -> &'static str {
        "Require identifiers to match a naming pattern"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&Naming> = self
            .config
            .namings()
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
            NAMING_NAME,
            NAMING_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut scopes, &mut |view, scopes| {
            let Some(ident) = view.ident else {
                return;
            };
            let name = ident.to_string();
            for (index, rule) in applicable.iter().enumerate() {
                if rule.item() != view.kind || !rule.visibility().matches(view.is_pub) {
                    continue;
                }
                if rule.pattern().is_match(&name) {
                    continue;
                }
                match scopes.decide(index, ctx, NAMING_CODE, view.span, Some(view.attrs)) {
                    Decision::Suppressed => {}
                    Decision::MissingReason(warning) => violations.push(*warning),
                    Decision::Report => violations.push(build_violation(
                        NAMING_CODE,
                        rule.name(),
                        rule.severity(),
                        ctx,
                        view.span,
                        format!(
                            "{}: `{} {}` does not match `{}`",
                            rule.message(),
                            view.kind,
                            name,
                            rule.pattern().as_str(),
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
    use super::*;
    use crate::declarative::model::*;
    use crate::types::Severity;

    fn naming_rule(item: ItemKind, pattern: &str, visibility: VisibilityFilter) -> NamingRule {
        let config = DeclarativeConfig::builder()
            .namings(vec![Naming::new(
                "domain-ports".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                item,
                RegexPattern::new(pattern).unwrap(),
                "Domain traits are ports.".to_string(),
                None,
                Severity::Warning,
            )
            .with_visibility(visibility)])
            .build()
            .unwrap();
        NamingRule::new(Arc::new(config))
    }

    #[test]
    fn naming_flags_non_matching_trait() {
        let rule = naming_rule(
            ItemKind::Trait,
            "^[A-Z][A-Za-z0-9]*(Repository|Port)$",
            VisibilityFilter::Pub,
        );
        let violations = crate::test_utils::check_rule(&rule, "pub trait UserService {}");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, NAMING_CODE);
        assert!(violations[0].message.contains("trait UserService"));
        assert!(violations[0].message.contains("Repository|Port"));
    }

    #[test]
    fn naming_accepts_matching_trait() {
        let rule = naming_rule(
            ItemKind::Trait,
            "^[A-Z][A-Za-z0-9]*(Repository|Port)$",
            VisibilityFilter::Pub,
        );
        let violations = crate::test_utils::check_rule(&rule, "pub trait UserRepository {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn naming_ignores_other_item_kinds() {
        let rule = naming_rule(ItemKind::Trait, "^X$", VisibilityFilter::Pub);
        let violations = crate::test_utils::check_rule(&rule, "pub struct UserService;");
        assert!(violations.is_empty());
    }

    #[test]
    fn naming_pub_visibility_skips_private_item() {
        let rule = naming_rule(ItemKind::Trait, "^X$", VisibilityFilter::Pub);
        assert!(crate::test_utils::check_rule(&rule, "trait UserService {}").is_empty());

        let any = naming_rule(ItemKind::Trait, "^X$", VisibilityFilter::Any);
        assert_eq!(
            crate::test_utils::check_rule(&any, "trait UserService {}").len(),
            1
        );
    }

    #[test]
    fn naming_checks_nested_module_items() {
        let rule = naming_rule(ItemKind::Struct, "^Dto[A-Z]", VisibilityFilter::Pub);
        let violations =
            crate::test_utils::check_rule(&rule, "pub mod inner {\n    pub struct User;\n}");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].location.line, 2);
    }

    // ── suppression ──

    fn ports_rule() -> NamingRule {
        naming_rule(
            ItemKind::Trait,
            "^[A-Z][A-Za-z0-9]*(Repository|Port)$",
            VisibilityFilter::Pub,
        )
    }

    #[test]
    fn suppress_attribute_on_the_item() {
        let violations = crate::test_utils::check_rule(
            &ports_rule(),
            "#[arch_lint::allow(domain_ports, reason = \"legacy name\")]\npub trait UserService {}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let violations = crate::test_utils::check_rule(
            &ports_rule(),
            "#[arch_lint::allow(naming, reason = \"legacy names\")]\npub mod legacy {\n    pub trait UserService {}\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        for name in ["naming", "domain-ports", "ALD007"] {
            let code = format!("// arch-lint: allow({name})\npub trait UserService {{}}");
            let violations = crate::test_utils::check_rule(&ports_rule(), &code);
            assert!(
                violations.is_empty(),
                "allow({name}) should suppress: {violations:#?}"
            );
        }
    }

    #[test]
    fn user_name_allow_does_not_suppress_a_sibling_rule() {
        let config = DeclarativeConfig::builder()
            .namings(vec![
                Naming::new(
                    "domain-ports".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                    ItemKind::Trait,
                    RegexPattern::new("^[A-Z][A-Za-z0-9]*(Repository|Port)$").unwrap(),
                    "Domain traits are ports.".to_string(),
                    None,
                    Severity::Warning,
                )
                .with_visibility(VisibilityFilter::Pub),
                Naming::new(
                    "traits-are-prefixed".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                    ItemKind::Trait,
                    RegexPattern::new("^Abstract").unwrap(),
                    "Domain traits start with Abstract.".to_string(),
                    None,
                    Severity::Warning,
                )
                .with_visibility(VisibilityFilter::Pub),
            ])
            .build()
            .unwrap();
        let rule = NamingRule::new(Arc::new(config));

        let violations = crate::test_utils::check_rule(
            &rule,
            "#[arch_lint::allow(domain_ports, reason = \"legacy name\")]\npub trait UserService {}",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].rule, "traits-are-prefixed");
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let config = DeclarativeConfig::builder()
            .namings(vec![Naming::new(
                "domain-ports".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                ItemKind::Trait,
                RegexPattern::new("^[A-Z][A-Za-z0-9]*(Repository|Port)$").unwrap(),
                "Domain traits are ports.".to_string(),
                None,
                Severity::Error,
            )
            .with_visibility(VisibilityFilter::Pub)])
            .build()
            .unwrap();
        let rule = NamingRule::new(Arc::new(config));

        let violations = crate::test_utils::check_rule(
            &rule,
            "// arch-lint: allow(domain-ports)\npub trait UserService {}",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].code, NAMING_CODE);
        assert!(violations[0].message.contains("missing required reason"));
    }
}
