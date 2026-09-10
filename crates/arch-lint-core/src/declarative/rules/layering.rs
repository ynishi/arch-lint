//! `[[layering]]` (ALD004): expands ordered layers into scope-dependency
//! constraints and checks them with the scope-dep visitor.

use std::sync::Arc;

use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, ScopeDep};
use crate::rule::Rule;
use crate::types::Violation;

use super::scope_dep::ScopeDepVisitor;

const LAYERING_NAME: &str = "layering";
const LAYERING_CODE: &str = "ALD004";

/// A per-file rule that enforces `[[layering]]` declarations.
///
/// Each layering is expanded into the equivalent [`ScopeDep`] constraints and
/// checked with the same machinery as [`ScopeDepRule`](super::ScopeDepRule),
/// but reported as `ALD004`. A layer may depend on the layers below it;
/// referencing a layer above it — or, with `adjacent_only`, skipping past the
/// next one — is a violation.
pub struct LayeringRule {
    config: Arc<DeclarativeConfig>,
    deps: Vec<ScopeDep>,
}

impl LayeringRule {
    /// Creates a new layering rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        let deps = config
            .layerings()
            .iter()
            .flat_map(crate::declarative::model::Layering::expand)
            .collect();
        Self { config, deps }
    }
}

impl Rule for LayeringRule {
    fn name(&self) -> &'static str {
        LAYERING_NAME
    }

    fn code(&self) -> &'static str {
        LAYERING_CODE
    }

    fn description(&self) -> &'static str {
        "Enforce ordered layer dependencies"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let file_scopes = self.config.scopes_for_path(&ctx.relative_path);
        if file_scopes.is_empty() {
            return vec![];
        }

        let applicable: Vec<&ScopeDep> = self
            .deps
            .iter()
            .filter(|dep| file_scopes.contains(&dep.from_scope()))
            .collect();

        if applicable.is_empty() {
            return vec![];
        }

        let mut visitor = ScopeDepVisitor {
            ctx,
            config: &self.config,
            applicable,
            code: LAYERING_CODE,
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{make_ctx, parse_file};
    use super::*;
    use crate::declarative::model::*;
    use crate::types::Severity;

    fn make_layering_config(adjacent_only: bool) -> Arc<DeclarativeConfig> {
        let scopes = vec![
            Scope::new(
                ScopeName::new("handler").unwrap(),
                vec![GlobPattern::new("src/handler/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let layering = Layering::new(
            Some("clean-arch".to_string()),
            vec![
                ScopeName::new("handler").unwrap(),
                ScopeName::new("domain").unwrap(),
                ScopeName::new("infra").unwrap(),
            ],
            "Layers may only depend downward.".to_string(),
            Some("ARCHITECTURE.md L20".to_string()),
            Severity::Error,
        )
        .unwrap()
        .with_adjacent_only(adjacent_only);

        Arc::new(
            DeclarativeConfig::builder()
                .scopes(scopes)
                .layerings(vec![layering])
                .build()
                .unwrap(),
        )
    }

    #[test]
    fn layering_denies_upward_dependency() {
        let rule = LayeringRule::new(make_layering_config(false));
        let code = "use crate::handler::api::Router;";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, LAYERING_CODE);
        assert_eq!(violations[0].rule, "clean-arch");
        assert!(violations[0].message.contains("scope `infra`"));
        assert!(violations[0].message.contains("scope `handler`"));
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L20")
        );
    }

    #[test]
    fn layering_allows_downward_dependency() {
        let rule = LayeringRule::new(make_layering_config(false));
        let code = "use crate::infra::db::Pool;\nuse crate::domain::entity::User;";
        let ctx = make_ctx("src/handler/api.rs", code);
        let ast = parse_file(code);

        assert!(rule.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn layering_adjacent_only_denies_skipping_a_layer() {
        let rule = LayeringRule::new(make_layering_config(true));
        let code = "use crate::infra::db::Pool;";
        let ctx = make_ctx("src/handler/api.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("scope `handler`"));
        assert!(violations[0].message.contains("scope `infra`"));
    }

    #[test]
    fn layering_adjacent_only_allows_next_layer() {
        let rule = LayeringRule::new(make_layering_config(true));
        let code = "use crate::domain::entity::User;";
        let ctx = make_ctx("src/handler/api.rs", code);
        let ast = parse_file(code);

        assert!(rule.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn layering_detects_inline_path() {
        let rule = LayeringRule::new(make_layering_config(false));
        let code = "fn f() { crate::handler::api::route(); }";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("crate::handler::api::route"));
    }

    #[test]
    fn layering_check_inline_false_skips_inline_path() {
        let scopes = vec![
            Scope::new(
                ScopeName::new("handler").unwrap(),
                vec![GlobPattern::new("src/handler/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let layering = Layering::new(
            None,
            vec![
                ScopeName::new("handler").unwrap(),
                ScopeName::new("infra").unwrap(),
            ],
            "Layers may only depend downward.".to_string(),
            None,
            Severity::Error,
        )
        .unwrap()
        .with_check_inline(false);
        let config = Arc::new(
            DeclarativeConfig::builder()
                .scopes(scopes)
                .layerings(vec![layering])
                .build()
                .unwrap(),
        );
        let rule = LayeringRule::new(config);
        let code = "use crate::handler::api::Router;\nfn f() { crate::handler::api::route(); }";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "layering");
    }

    #[test]
    fn layering_skips_file_outside_every_layer() {
        let rule = LayeringRule::new(make_layering_config(false));
        let code = "use crate::handler::api::Router;";
        let ctx = make_ctx("src/util/misc.rs", code);
        let ast = parse_file(code);

        assert!(rule.check(&ctx, &ast).is_empty());
    }
}
