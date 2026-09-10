//! `[[deny-scope-dep]]` (ALD003): denies scope-crossing dependencies, plus the
//! [`ScopeDepVisitor`] shared with the layering rule.

use std::sync::Arc;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, ScopeDep};
use crate::rule::Rule;
use crate::types::{Location, Violation};

use super::{expand_use_tree, inline_path_to_string};

const SCOPE_DEP_NAME: &str = "deny-scope-dep";
const SCOPE_DEP_CODE: &str = "ALD003";

/// A per-file rule that enforces `[[deny-scope-dep]]` declarations.
///
/// Detects `use crate::...` imports and inline qualified paths
/// (e.g. `crate::infra::db::connect()`) that cross denied scope boundaries.
/// For example, if `domain` must not depend on `infra`, then files in
/// `src/domain/**` must not reference `crate::infra::...`.
///
/// Inline path checking can be disabled per rule with `check-inline = false`.
///
/// # Limitations (v1)
///
/// - Only checks `crate::` prefixed paths (not `self::` or `super::`)
/// - Assumes standard `src/` layout for module-to-file mapping
pub struct ScopeDepRule {
    config: Arc<DeclarativeConfig>,
}

impl ScopeDepRule {
    /// Creates a new scope-dep rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for ScopeDepRule {
    fn name(&self) -> &'static str {
        SCOPE_DEP_NAME
    }

    fn code(&self) -> &'static str {
        SCOPE_DEP_CODE
    }

    fn description(&self) -> &'static str {
        "Deny scope-level dependencies"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        // Determine which scopes this file belongs to
        let file_scopes = self.config.scopes_for_path(&ctx.relative_path);
        if file_scopes.is_empty() {
            return vec![];
        }

        // Find applicable deny-scope-dep rules where this file's scope is the "from"
        let applicable: Vec<&ScopeDep> = self
            .config
            .scope_deps()
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
            code: SCOPE_DEP_CODE,
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

/// Shared visitor for scope-crossing dependencies.
///
/// Used by both [`ScopeDepRule`] (`ALD003`) and
/// [`LayeringRule`](super::LayeringRule) (`ALD004`); `code` selects which code
/// the violations are reported under.
pub(super) struct ScopeDepVisitor<'a> {
    pub(super) ctx: &'a FileContext<'a>,
    pub(super) config: &'a DeclarativeConfig,
    pub(super) applicable: Vec<&'a ScopeDep>,
    pub(super) code: &'static str,
    pub(super) violations: Vec<Violation>,
}

/// Converts a `crate::x::y::z` module path to candidate file paths
/// and returns the scopes that any candidate belongs to.
///
/// Tries both `src/x/y/z.rs` and `src/x/y/z/mod.rs` to handle
/// the two standard Rust module layout conventions.
///
/// Only handles `crate::` prefixed paths. External crates, `self::`,
/// and `super::` paths return an empty vec.
fn resolve_target_scopes<'a>(
    config: &'a DeclarativeConfig,
    use_path: &str,
) -> Vec<&'a crate::declarative::model::ScopeName> {
    let Some(rest) = use_path.strip_prefix("crate::") else {
        return vec![];
    };

    let segments: Vec<&str> = rest.split("::").collect();
    if segments.is_empty() {
        return vec![];
    }

    let joined = segments.join("/");

    // Try both module layout conventions:
    //   1. src/x/y/z.rs        (file-per-module)
    //   2. src/x/y/z/mod.rs    (directory-with-mod)
    let candidates = [format!("src/{joined}.rs"), format!("src/{joined}/mod.rs")];

    for candidate in &candidates {
        let scopes = config.scopes_for_path(std::path::Path::new(candidate));
        if !scopes.is_empty() {
            return scopes;
        }
    }

    vec![]
}

impl<'ast> Visit<'ast> for ScopeDepVisitor<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            let target_scopes = resolve_target_scopes(self.config, &use_item.path);

            for dep in &self.applicable {
                for target_scope in &target_scopes {
                    if dep.is_denied(target_scope) {
                        let start = use_item.span.start();
                        let location = Location::new(
                            self.ctx.relative_path.clone(),
                            start.line,
                            start.column + 1,
                        );

                        let mut violation = Violation::new(
                            self.code,
                            dep.display_name(),
                            dep.severity(),
                            location,
                            format!(
                                "{}: `{}` (scope `{}` \u{2192} scope `{}`)",
                                dep.message(),
                                use_item.path,
                                dep.from_scope(),
                                target_scope,
                            ),
                        );
                        if let Some(doc) = dep.doc_ref() {
                            violation = violation.with_doc_ref(doc);
                        }

                        self.violations.push(violation);
                    }
                }
            }
        }

        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // `use` statements contain `UseTree` nodes, not `syn::Path`,
        // so this never double-reports an import already flagged above.
        if let Some(path) = inline_path_to_string(node) {
            let target_scopes = resolve_target_scopes(self.config, &path);

            for dep in &self.applicable {
                if !dep.check_inline() {
                    continue;
                }
                for target_scope in &target_scopes {
                    if dep.is_denied(target_scope) {
                        let start = node.span().start();
                        let location = Location::new(
                            self.ctx.relative_path.clone(),
                            start.line,
                            start.column + 1,
                        );

                        let mut violation = Violation::new(
                            self.code,
                            dep.display_name(),
                            dep.severity(),
                            location,
                            format!(
                                "{}: `{}` (scope `{}` \u{2192} scope `{}`)",
                                dep.message(),
                                path,
                                dep.from_scope(),
                                target_scope,
                            ),
                        );
                        if let Some(doc) = dep.doc_ref() {
                            violation = violation.with_doc_ref(doc);
                        }

                        self.violations.push(violation);
                    }
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

    fn make_scope_dep_config() -> Arc<DeclarativeConfig> {
        let scopes = vec![
            Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let deps = vec![ScopeDep::new(
            Some("no-domain-to-infra".to_string()),
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "Domain must not depend on infra.".to_string(),
            Some("ARCHITECTURE.md L10".to_string()),
            Severity::Error,
        )];
        Arc::new(DeclarativeConfig::new(scopes, vec![], vec![], deps).unwrap())
    }

    #[test]
    fn scope_dep_detects_forbidden_dependency() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use crate::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, SCOPE_DEP_CODE);
        assert_eq!(violations[0].rule, "no-domain-to-infra");
        assert_eq!(violations[0].severity, Severity::Error);
        assert!(violations[0].message.contains("domain"));
        assert!(violations[0].message.contains("infra"));
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L10")
        );
    }

    #[test]
    fn scope_dep_allows_same_scope_dependency() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use crate::domain::entity::User;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn scope_dep_allows_external_crate() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use sqlx::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn scope_dep_skips_file_not_in_any_scope() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use crate::infra::db::Pool;";
        let ctx = make_ctx("src/handlers/api.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn scope_dep_allows_reverse_direction() {
        // infra -> domain is allowed (only domain -> infra is denied)
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use crate::domain::entity::User;";
        let ctx = make_ctx("src/infra/db.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn scope_dep_detects_grouped_forbidden() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use crate::infra::{db::Pool, cache::Redis};";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn scope_dep_mixed_allowed_and_denied() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        // domain -> domain is OK, domain -> infra is denied
        let code = "use crate::domain::entity::User;\nuse crate::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("infra"));
    }

    #[test]
    fn scope_dep_detects_inline_expr_path() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "fn f() { crate::infra::db::connect(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("crate::infra::db::connect"));
    }

    #[test]
    fn scope_dep_detects_inline_type_position() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "fn f(db: crate::infra::db::Pool) {}";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("crate::infra::db::Pool"));
    }

    #[test]
    fn scope_dep_inline_allows_same_scope() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "fn f() { crate::domain::entity::User::new(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn scope_dep_check_inline_false_skips_inline_path() {
        let scopes = vec![
            Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let deps = vec![ScopeDep::new(
            Some("no-domain-to-infra".to_string()),
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "Domain must not depend on infra.".to_string(),
            None,
            Severity::Error,
        )
        .with_check_inline(false)];
        let config = Arc::new(DeclarativeConfig::new(scopes, vec![], vec![], deps).unwrap());
        let rule = ScopeDepRule::new(config);
        // The inline path is skipped, but the `use` import is still flagged.
        let code = "use crate::infra::db::Pool;\nfn f() { crate::infra::db::connect(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("crate::infra::db::Pool"));
    }

    #[test]
    fn scope_dep_without_name_uses_fallback() {
        let scopes = vec![
            Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let deps = vec![ScopeDep::new(
            None, // no explicit name
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "Domain must not depend on infra.".to_string(),
            None,
            Severity::Error,
        )];
        let config = Arc::new(DeclarativeConfig::new(scopes, vec![], vec![], deps).unwrap());
        let rule = ScopeDepRule::new(config);
        let code = "use crate::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "deny-scope-dep:domain");
    }

    // ── resolve_target_scopes: mod.rs fallback ──

    #[test]
    fn scope_dep_detects_mod_rs_layout() {
        // Scope uses "src/infra/**" which should match both
        // "src/infra/db.rs" and "src/infra/db/mod.rs".
        // When use path is `crate::infra::db::Pool`, resolve_target_scopes
        // tries "src/infra/db/Pool.rs" first, then "src/infra/db/Pool/mod.rs".
        // Both should match the "src/infra/**" glob.
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        // This simulates a module that uses mod.rs convention
        let code = "use crate::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn resolve_target_scopes_external_crate_returns_empty() {
        let config = make_scope_dep_config();
        let scopes = resolve_target_scopes(&config, "sqlx::Pool");
        assert!(scopes.is_empty());
    }

    #[test]
    fn resolve_target_scopes_self_path_returns_empty() {
        let config = make_scope_dep_config();
        let scopes = resolve_target_scopes(&config, "self::utils::helper");
        assert!(scopes.is_empty());
    }
}
