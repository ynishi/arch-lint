//! `[[require-return]]` (ALD008): flags functions whose return type does not
//! match the pattern.

use std::sync::Arc;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RequireReturn};
use crate::rule::Rule;
use crate::types::Violation;

use super::{build_violation, render_return_type, walk_items, Decision, Suppressions};

const REQUIRE_RETURN_NAME: &str = "require-return";
const REQUIRE_RETURN_CODE: &str = "ALD008";

/// A per-file rule that enforces `[[require-return]]` declarations.
///
/// Flags functions whose normalized return type does not match the regex.
pub struct RequireReturnRule {
    config: Arc<DeclarativeConfig>,
}

impl RequireReturnRule {
    /// Creates a new require-return rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for RequireReturnRule {
    fn name(&self) -> &'static str {
        REQUIRE_RETURN_NAME
    }

    fn code(&self) -> &'static str {
        REQUIRE_RETURN_CODE
    }

    fn description(&self) -> &'static str {
        "Require functions to return a matching type"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&RequireReturn> = self
            .config
            .require_returns()
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
            REQUIRE_RETURN_NAME,
            REQUIRE_RETURN_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut scopes, &mut |view, scopes| {
            let Some(sig) = view.sig else {
                return;
            };
            let rendered = render_return_type(sig);
            let is_async = sig.asyncness.is_some();
            for (index, rule) in applicable.iter().enumerate() {
                if !rule.visibility().matches(view.is_pub) || !rule.matches_asyncness(is_async) {
                    continue;
                }
                if rule.pattern().is_match(&rendered) {
                    continue;
                }
                match scopes.decide(index, ctx, REQUIRE_RETURN_CODE, view.span, Some(view.attrs)) {
                    Decision::Suppressed => {}
                    Decision::MissingReason(warning) => violations.push(*warning),
                    Decision::Report => violations.push(build_violation(
                        REQUIRE_RETURN_CODE,
                        rule.name(),
                        rule.severity(),
                        ctx,
                        view.span,
                        format!(
                            "{}: `fn {}` returns `{}`, expected `{}`",
                            rule.message(),
                            view.name,
                            rendered,
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

    fn require_return_rule(
        pattern: &str,
        visibility: VisibilityFilter,
        asyncness: Option<bool>,
    ) -> RequireReturnRule {
        let config = DeclarativeConfig::builder()
            .require_returns(vec![RequireReturn::new(
                "handlers-return-app-result".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                RegexPattern::new(pattern).unwrap(),
                "Handlers must return Result<_, AppError>.".to_string(),
                None,
                Severity::Error,
            )
            .with_visibility(visibility)
            .with_asyncness(asyncness)])
            .build()
            .unwrap();
        RequireReturnRule::new(Arc::new(config))
    }

    const APP_RESULT: &str = r"^Result<.*,\s*AppError>$";

    #[test]
    fn require_return_flags_wrong_return_type() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, Some(true));
        let violations =
            crate::test_utils::check_rule(&rule, "pub async fn handler() -> String { todo!() }");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, REQUIRE_RETURN_CODE);
        assert!(violations[0].message.contains("returns `String`"));
    }

    #[test]
    fn require_return_accepts_matching_return_type() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, Some(true));
        let violations = crate::test_utils::check_rule(
            &rule,
            "pub async fn handler() -> Result<Json<User>, AppError> { todo!() }",
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn require_return_async_filter_skips_sync_fn() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, Some(true));
        let violations =
            crate::test_utils::check_rule(&rule, "pub fn helper() -> String { todo!() }");
        assert!(violations.is_empty());
    }

    #[test]
    fn require_return_without_async_filter_checks_every_fn() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, None);
        let violations =
            crate::test_utils::check_rule(&rule, "pub fn helper() -> String { todo!() }");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn require_return_missing_return_type_renders_as_unit() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, None);
        let violations = crate::test_utils::check_rule(&rule, "pub fn handler() {}");
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("returns `()`"));
    }

    #[test]
    fn require_return_checks_impl_methods() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, None);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\nimpl S {\n    pub fn go() -> String { todo!() }\n}",
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].location.line, 3);
    }

    #[test]
    fn require_return_private_fn_skipped_by_default_visibility() {
        let rule = require_return_rule(APP_RESULT, VisibilityFilter::Pub, None);
        assert!(
            crate::test_utils::check_rule(&rule, "fn helper() -> String { todo!() }").is_empty()
        );
    }

    // ── suppression ──

    fn handler_rule() -> RequireReturnRule {
        require_return_rule(APP_RESULT, VisibilityFilter::Pub, Some(true))
    }

    #[test]
    fn suppress_attribute_on_the_item() {
        let violations = crate::test_utils::check_rule(
            &handler_rule(),
            "#[arch_lint::allow(handlers_return_app_result, reason = \"streaming handler\")]\npub async fn handler() -> String { todo!() }",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let violations = crate::test_utils::check_rule(
            &handler_rule(),
            "#[arch_lint::allow(require_return, reason = \"streaming handlers\")]\npub mod stream {\n    pub async fn handler() -> String { todo!() }\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_attribute_on_enclosing_impl() {
        let violations = crate::test_utils::check_rule(
            &handler_rule(),
            "struct S;\n#[arch_lint::allow(ALD008, reason = \"streaming handlers\")]\nimpl S {\n    pub async fn handler() -> String { todo!() }\n}",
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        for name in ["require-return", "handlers-return-app-result", "ALD008"] {
            let code = format!(
                "// arch-lint: allow({name}) reason=\"streaming handler\"\npub async fn handler() -> String {{ todo!() }}"
            );
            let violations = crate::test_utils::check_rule(&handler_rule(), &code);
            assert!(
                violations.is_empty(),
                "allow({name}) should suppress: {violations:#?}"
            );
        }
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let violations = crate::test_utils::check_rule(
            &handler_rule(),
            "// arch-lint: allow(handlers-return-app-result)\npub async fn handler() -> String { todo!() }",
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].code, REQUIRE_RETURN_CODE);
        assert!(violations[0].message.contains("missing required reason"));
    }
}
