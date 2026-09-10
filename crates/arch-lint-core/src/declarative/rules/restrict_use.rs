//! `[[restrict-use]]` (ALD001): denies configured imports and inline
//! qualified paths within a scope.

use std::sync::Arc;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RestrictUse};
use crate::rule::Rule;
use crate::types::{Location, Violation};

use super::{expand_use_tree, inline_path_to_string};

const RESTRICT_USE_NAME: &str = "restrict-use";
const RESTRICT_USE_CODE: &str = "ALD001";

/// A per-file rule that enforces `[[restrict-use]]` declarations.
///
/// For each file, determines which restrict-use rules apply based on
/// scope membership, then checks every `use` import — and, unless
/// `check-inline = false`, every inline qualified path — against the deny list.
pub struct RestrictUseRule {
    config: Arc<DeclarativeConfig>,
}

impl RestrictUseRule {
    /// Creates a new restrict-use rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for RestrictUseRule {
    fn name(&self) -> &'static str {
        RESTRICT_USE_NAME
    }

    fn code(&self) -> &'static str {
        RESTRICT_USE_CODE
    }

    fn description(&self) -> &'static str {
        "Deny specified imports within a scope"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&RestrictUse> = self
            .config
            .restrict_uses()
            .iter()
            .filter(|r| {
                self.config
                    .scope_ref_contains(r.scope(), &ctx.relative_path)
            })
            .collect();

        if applicable.is_empty() {
            return vec![];
        }

        let mut visitor = RestrictUseVisitor {
            ctx,
            applicable,
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

struct RestrictUseVisitor<'a> {
    ctx: &'a FileContext<'a>,
    applicable: Vec<&'a RestrictUse>,
    violations: Vec<Violation>,
}

impl<'ast> Visit<'ast> for RestrictUseVisitor<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            for rule in &self.applicable {
                if rule.is_denied(&use_item.path) {
                    let start = use_item.span.start();
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

                    let mut violation = Violation::new(
                        RESTRICT_USE_CODE,
                        rule.name(),
                        rule.severity(),
                        location,
                        format!("{}: `{}`", rule.message(), use_item.path),
                    );
                    if let Some(doc) = rule.doc_ref() {
                        violation = violation.with_doc_ref(doc);
                    }

                    self.violations.push(violation);
                }
            }
        }

        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // `use` statements contain `UseTree` nodes, not `syn::Path`,
        // so this never double-reports an import already flagged above.
        if let Some(path) = inline_path_to_string(node) {
            for rule in &self.applicable {
                if rule.check_inline() && rule.is_denied(&path) {
                    let start = node.span().start();
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

                    let mut violation = Violation::new(
                        RESTRICT_USE_CODE,
                        rule.name(),
                        rule.severity(),
                        location,
                        format!("{}: `{}`", rule.message(), path),
                    );
                    if let Some(doc) = rule.doc_ref() {
                        violation = violation.with_doc_ref(doc);
                    }

                    self.violations.push(violation);
                }
            }
        }

        syn::visit::visit_path(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{make_ctx, parse_file};
    use super::*;
    use crate::declarative::model::*;
    use crate::types::Severity;

    fn make_restrict_config() -> Arc<DeclarativeConfig> {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "Domain must be DB-agnostic.".to_string(),
            Some("ARCHITECTURE.md L85".to_string()),
            Severity::Error,
        )];
        Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap())
    }

    #[test]
    fn restrict_detects_denied_import() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "no-sqlx-in-domain");
        assert_eq!(violations[0].code, RESTRICT_USE_CODE);
        assert_eq!(violations[0].severity, Severity::Error);
        assert!(violations[0].message.contains("sqlx::Pool"));
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L85")
        );
    }

    #[test]
    fn restrict_allows_non_denied_import() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "use serde::Serialize;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn restrict_skips_file_outside_scope() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::Pool;";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn restrict_detects_grouped_denied_import() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::{Pool, query};";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn restrict_inline_scope() {
        let config = Arc::new(
            DeclarativeConfig::new(
                vec![],
                vec![RestrictUse::new(
                    "no-sqlx-handlers".to_string(),
                    ScopeRef::Inline(vec![GlobPattern::new("src/handlers/**").unwrap()]),
                    vec![UsePattern::new("sqlx::*").unwrap()],
                    "Handlers must use repository.".to_string(),
                    None,
                    Severity::Warning,
                )],
                vec![],
                vec![],
            )
            .unwrap(),
        );
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::Pool;";
        let ctx = make_ctx("src/handlers/api.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(violations[0].doc_ref.is_none());
    }

    #[test]
    fn restrict_glob_import_denied() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::*;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        // "sqlx::*" matches pattern "sqlx::*"
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn restrict_detects_inline_expr_path() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = r#"fn f() { let _ = sqlx::query("SELECT 1"); }"#;
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sqlx::query"));
    }

    #[test]
    fn restrict_detects_inline_type_position() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "fn f(pool: sqlx::Pool) {}";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sqlx::Pool"));
    }

    #[test]
    fn restrict_check_inline_false_skips_inline_path() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "Domain must be DB-agnostic.".to_string(),
            None,
            Severity::Error,
        )
        .with_check_inline(false)];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        // The inline path is skipped, but the `use` import is still flagged.
        let code = "use sqlx::Pool;\nfn f() { sqlx::query(\"SELECT 1\"); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sqlx::Pool"));
    }

    #[test]
    fn restrict_inline_skips_self_and_super_paths() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "fn f() { self::helper(); super::helper(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn restrict_empty_file_no_violations() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    // ── restrict-use `except` / scope `exclude` ──

    #[test]
    fn restrict_except_suppresses_denied_import() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::**").unwrap()],
            "Domain must be DB-agnostic.".to_string(),
            None,
            Severity::Error,
        )
        .with_except(vec![UsePattern::new("sqlx::types::*").unwrap()])];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::types::Json;\nuse sqlx::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("sqlx::Pool"));
    }

    #[test]
    fn scope_exclude_takes_file_out_of_every_rule() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )
        .with_excludes(vec![GlobPattern::new("src/domain/tests/**").unwrap()])];
        let restrict = vec![RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "Domain must be DB-agnostic.".to_string(),
            None,
            Severity::Error,
        )];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        let code = "use sqlx::Pool;";
        let ast = parse_file(code);

        assert_eq!(
            rule.check(&make_ctx("src/domain/service.rs", code), &ast)
                .len(),
            1
        );
        assert!(rule
            .check(&make_ctx("src/domain/tests/service_test.rs", code), &ast)
            .is_empty());
    }
}
