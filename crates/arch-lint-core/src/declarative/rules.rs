//! Declarative rule implementations (syn visitors).
//!
//! Converts domain model rules into [`Rule`] trait implementations
//! that analyze `syn` ASTs.

use std::sync::Arc;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RequireUse, RestrictUse, ScopeDep};
use crate::rule::Rule;
use crate::types::{Location, Severity, Violation};

// ────────────────────────────────────────────
// UseTree expansion
// ────────────────────────────────────────────

/// A resolved use-path with its source span.
pub(crate) struct ResolvedUse {
    /// Full path like `sqlx::Pool` or `std::collections::HashMap`.
    pub(crate) path: String,
    /// Span of the leaf node for error reporting.
    pub(crate) span: proc_macro2::Span,
}

/// Recursively expands a [`syn::UseTree`] into flat `::` separated paths.
///
/// For example, `use std::collections::{HashMap, BTreeMap};` expands to
/// `["std::collections::HashMap", "std::collections::BTreeMap"]`.
pub(crate) fn expand_use_tree(tree: &syn::UseTree, prefix: &str) -> Vec<ResolvedUse> {
    match tree {
        syn::UseTree::Path(p) => {
            let new_prefix = if prefix.is_empty() {
                p.ident.to_string()
            } else {
                format!("{prefix}::{}", p.ident)
            };
            expand_use_tree(&p.tree, &new_prefix)
        }
        syn::UseTree::Name(n) => {
            let path = if prefix.is_empty() {
                n.ident.to_string()
            } else {
                format!("{prefix}::{}", n.ident)
            };
            vec![ResolvedUse {
                path,
                span: n.ident.span(),
            }]
        }
        syn::UseTree::Rename(r) => {
            let path = if prefix.is_empty() {
                r.ident.to_string()
            } else {
                format!("{prefix}::{}", r.ident)
            };
            vec![ResolvedUse {
                path,
                span: r.ident.span(),
            }]
        }
        syn::UseTree::Glob(g) => {
            let path = if prefix.is_empty() {
                "*".to_string()
            } else {
                format!("{prefix}::*")
            };
            vec![ResolvedUse {
                path,
                span: g.span(),
            }]
        }
        syn::UseTree::Group(g) => g
            .items
            .iter()
            .flat_map(|item| expand_use_tree(item, prefix))
            .collect(),
    }
}

// ────────────────────────────────────────────
// RestrictUseRule
// ────────────────────────────────────────────

const RESTRICT_USE_NAME: &str = "restrict-use";
const RESTRICT_USE_CODE: &str = "ALD001";

/// A per-file rule that enforces `[[restrict-use]]` declarations.
///
/// For each file, determines which restrict-use rules apply based on
/// scope membership, then checks every `use` import against the deny list.
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
}

// ────────────────────────────────────────────
// RequireUseRule
// ────────────────────────────────────────────

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

        let mut visitor = RequireUseVisitor {
            ctx,
            applicable,
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

struct RequireUseVisitor<'a> {
    ctx: &'a FileContext<'a>,
    applicable: Vec<&'a RequireUse>,
    violations: Vec<Violation>,
}

impl<'ast> Visit<'ast> for RequireUseVisitor<'_> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            let crate_name = use_item.path.split("::").next().unwrap_or(&use_item.path);

            for rule in &self.applicable {
                if rule.over().iter().any(|o| o == crate_name) {
                    let start = use_item.span.start();
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

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

        syn::visit::visit_item_use(self, node);
    }
}

// ────────────────────────────────────────────
// ScopeDepRule
// ────────────────────────────────────────────

const SCOPE_DEP_NAME: &str = "deny-scope-dep";
const SCOPE_DEP_CODE: &str = "ALD003";

/// A per-file rule that enforces `[[deny-scope-dep]]` declarations.
///
/// Detects `use crate::...` imports that cross denied scope boundaries.
/// For example, if `domain` must not depend on `infra`, then files in
/// `src/domain/**` must not contain `use crate::infra::...`.
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
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

struct ScopeDepVisitor<'a> {
    ctx: &'a FileContext<'a>,
    config: &'a DeclarativeConfig,
    applicable: Vec<&'a ScopeDep>,
    violations: Vec<Violation>,
}

/// Converts a `crate::x::y::z` module path to a candidate file path
/// and returns the scopes that path belongs to.
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

    // Construct candidate file path: src/<segments joined by />.rs
    // Even if the last segments are types (not modules), the glob
    // pattern `src/infra/**` still matches `src/infra/db/Pool.rs`.
    let candidate = format!("src/{}.rs", segments.join("/"));
    config.scopes_for_path(std::path::Path::new(&candidate))
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
                            SCOPE_DEP_CODE,
                            SCOPE_DEP_NAME,
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
}

// ────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::*;
    use std::path::PathBuf;

    fn parse_file(code: &str) -> syn::File {
        syn::parse_file(code).expect("test code should parse")
    }

    fn make_ctx<'a>(path: &'a str, content: &'a str) -> FileContext<'a> {
        FileContext {
            path: std::path::Path::new(path),
            content,
            is_test: false,
            module_path: vec![],
            relative_path: PathBuf::from(path),
        }
    }

    fn extract_use_tree(code: &str) -> syn::ItemUse {
        let file = parse_file(code);
        match file.items.into_iter().next() {
            Some(syn::Item::Use(u)) => u,
            _ => panic!("expected a use item"),
        }
    }

    // ── expand_use_tree ──

    #[test]
    fn expand_simple_path() {
        let item = extract_use_tree("use sqlx::Pool;");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].path, "sqlx::Pool");
    }

    #[test]
    fn expand_grouped_paths() {
        let item = extract_use_tree("use std::collections::{HashMap, BTreeMap};");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 2);
        let strs: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
        assert!(strs.contains(&"std::collections::HashMap"));
        assert!(strs.contains(&"std::collections::BTreeMap"));
    }

    #[test]
    fn expand_nested_group() {
        let item = extract_use_tree("use std::{collections::{HashMap, HashSet}, io::Read};");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 3);
        let strs: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
        assert!(strs.contains(&"std::collections::HashMap"));
        assert!(strs.contains(&"std::collections::HashSet"));
        assert!(strs.contains(&"std::io::Read"));
    }

    #[test]
    fn expand_glob() {
        let item = extract_use_tree("use sqlx::*;");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].path, "sqlx::*");
    }

    #[test]
    fn expand_rename() {
        let item = extract_use_tree("use sqlx::Pool as DbPool;");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].path, "sqlx::Pool");
    }

    #[test]
    fn expand_single_ident() {
        let item = extract_use_tree("use serde;");
        let paths = expand_use_tree(&item.tree, "");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].path, "serde");
    }

    // ── RestrictUseRule ──

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
    fn restrict_empty_file_no_violations() {
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    // ── RequireUseRule ──

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

    // ── ScopeDepRule ──

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
}
