//! `[[naming]]` (ALD007): flags identifiers that do not match the pattern.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, Naming};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, walk_items};

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

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut |view| {
            let Some(ident) = view.ident else {
                return;
            };
            let name = ident.to_string();
            for rule in &applicable {
                if rule.item() != view.kind || !rule.visibility().matches(view.is_pub) {
                    continue;
                }
                if !rule.pattern().is_match(&name) {
                    violations.push(build_violation(
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
}
