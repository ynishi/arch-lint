//! `[[deny-scope-dep]]` (ALD003): denies scope-crossing dependencies, plus the
//! [`ScopeDepVisitor`] shared with the layering rule.

use std::sync::Arc;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, ScopeDep};
use crate::rule::Rule;
use crate::types::{Location, Violation};

use super::{expand_use_tree, inline_path_to_string, Decision, Suppressions};

const SCOPE_DEP_NAME: &str = "deny-scope-dep";
const SCOPE_DEP_CODE: &str = "ALD003";

/// A per-file rule that enforces `[[deny-scope-dep]]` declarations.
///
/// Detects `use crate::...` imports and inline qualified paths
/// (e.g. `crate::infra::db::connect()`) that cross denied scope boundaries.
/// For example, if `domain` must not depend on `infra`, then files in
/// `src/domain/**` must not reference `crate::infra::...`.
///
/// Scope globs are relative to the analyzer root. `crate::`, `self::` and
/// `super::` paths are resolved against the package the referencing file
/// belongs to ([`FileContext::crate_root`]), so one config at a workspace root
/// covers its member crates (`crates/core/src/domain/**`).
///
/// Inline path checking can be disabled per rule with `check-inline = false`.
///
/// # Limitations (v1)
///
/// - Resolves `crate::`, `self::` and `super::` paths; bare paths
///   (`use infra::db;`, edition-2015 style), cross-crate paths
///   (`use other_crate::...`) and `pub use` re-export chasing are not followed
/// - Assumes standard `<package>/src/` layout for module-to-file mapping;
///   `[lib] path` / `[[bin]] path` overrides are ignored
/// - `self::` / `super::` are resolved only for files under the package's
///   `src/`, excluding `src/bin/` (each file under `src/bin/`, like `tests/`
///   and `benches/`, is its own crate root, so there is no single module path
///   to resolve against); `crate::` paths are unaffected by this and still
///   map to `<package>/src`
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

        let scopes = Suppressions::new(
            SCOPE_DEP_NAME,
            SCOPE_DEP_CODE,
            applicable.iter().map(|d| (d.display_name(), d.severity())),
        );

        let visitor = ScopeDepVisitor::new(ctx, &self.config, applicable, SCOPE_DEP_CODE, scopes);
        visitor.run(ast)
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
    /// Suppression state, one entry per rule in `applicable` and in the same
    /// order.
    pub(super) scopes: Suppressions,
    pub(super) violations: Vec<Violation>,
    /// Names of the inline `mod` blocks currently being visited, outermost
    /// first. Empty at the top level of the file.
    mod_stack: Vec<String>,
}

impl<'a> ScopeDepVisitor<'a> {
    /// Creates a visitor ready to run over one file's AST.
    pub(super) fn new(
        ctx: &'a FileContext<'a>,
        config: &'a DeclarativeConfig,
        applicable: Vec<&'a ScopeDep>,
        code: &'static str,
        scopes: Suppressions,
    ) -> Self {
        Self {
            ctx,
            config,
            applicable,
            code,
            scopes,
            violations: Vec::new(),
            mod_stack: Vec::new(),
        }
    }

    /// Visits `ast` and returns the violations collected.
    pub(super) fn run(mut self, ast: &syn::File) -> Vec<Violation> {
        self.visit_file(ast);
        self.violations
    }

    /// The module path at the visitor's current position: the file's own
    /// module path ([`FileContext::crate_module`]), plus the inline `mod`
    /// blocks currently entered. `None` when the file itself has no
    /// crate-relative module path, in which case `self::` / `super::` paths
    /// never resolve.
    fn current_module(&self) -> Option<Vec<String>> {
        module_at(self.ctx, &self.mod_stack)
    }
}

/// The module path inside `ctx`'s file after entering the inline `mod`
/// blocks `mod_stack` (outermost first): the file's own
/// [`FileContext::crate_module`] followed by `mod_stack`. `None` when the file
/// has no crate-relative module path.
pub(super) fn module_at(ctx: &FileContext<'_>, mod_stack: &[String]) -> Option<Vec<String>> {
    let mut module = ctx.crate_module()?.to_vec();
    module.extend(mod_stack.iter().cloned());
    Some(module)
}

/// Converts a `crate::x::y::z` module path to candidate file paths
/// and returns the scopes that any candidate belongs to.
///
/// `crate_root` is the root-relative directory of the package the referencing
/// file belongs to ([`FileContext::crate_root`]; empty when it is the
/// analyzer root). Tries both `<crate_root>/src/x/y/z.rs` and
/// `<crate_root>/src/x/y/z/mod.rs` to handle the two standard Rust module
/// layout conventions; scope globs are matched against these root-relative
/// candidates.
///
/// Only handles `crate::` prefixed paths; external crates return an empty
/// vec. `self::` and `super::` paths are rewritten to a `crate::` path by
/// [`resolve_relative_target_scopes`] before reaching here — calling this
/// function directly with a `self::` / `super::` path also returns an empty
/// vec, since it does not have the current module to rewrite against.
pub(super) fn resolve_target_scopes<'a>(
    config: &'a DeclarativeConfig,
    crate_root: &std::path::Path,
    use_path: &str,
) -> Vec<&'a crate::declarative::model::ScopeName> {
    let Some(rest) = use_path.strip_prefix("crate::") else {
        return vec![];
    };

    let segments: Vec<&str> = rest.split("::").collect();
    let Some((last, parents)) = segments.split_last() else {
        return vec![];
    };

    let mut module_dir = crate_root.join("src");
    module_dir.extend(parents);

    // Try both module layout conventions:
    //   1. <crate_root>/src/x/y/z.rs        (file-per-module)
    //   2. <crate_root>/src/x/y/z/mod.rs    (directory-with-mod)
    let candidates = [
        module_dir.join(format!("{last}.rs")),
        module_dir.join(last).join("mod.rs"),
    ];

    for candidate in &candidates {
        let scopes = config.scopes_for_path(candidate);
        if !scopes.is_empty() {
            return scopes;
        }
    }

    vec![]
}

/// Returns `true` for a `self::` / `super::` prefixed path (including the
/// bare `self` / `super`, which denotes the current / parent module itself).
pub(super) fn is_relative_path(path: &str) -> bool {
    path == "self" || path.starts_with("self::") || path == "super" || path.starts_with("super::")
}

/// Rewrites a `self::` or `super::` path relative to `current_module` into
/// the equivalent `crate::`-prefixed path. `current_module` is the module
/// path of the file (or inline `mod` block) containing the reference, in
/// the shape [`FileContext::crate_module`] returns.
///
/// Returns `None` when a `super::` climbs above the crate root. Paths that
/// are not `self::` / `super::` prefixed are returned unchanged (so this is
/// safe to call unconditionally).
fn rewrite_relative_path(path: &str, current_module: &[String]) -> Option<String> {
    let mut tokens = path.split("::");
    let Some(head) = tokens.next() else {
        return Some(path.to_string());
    };

    match head {
        "self" => {
            let mut segments = current_module.to_vec();
            segments.extend(tokens.map(str::to_string));
            Some(format!("crate::{}", segments.join("::")))
        }
        "super" => {
            let mut segments = current_module.to_vec();
            segments.pop()?;
            let mut rest = tokens;
            loop {
                match rest.next() {
                    Some("super") => {
                        segments.pop()?;
                    }
                    Some(other) => {
                        segments.push(other.to_string());
                        break;
                    }
                    None => break,
                }
            }
            segments.extend(rest.map(str::to_string));
            Some(format!("crate::{}", segments.join("::")))
        }
        _ => Some(path.to_string()),
    }
}

/// Resolves a `use`/inline path to scopes, following `crate::`, `self::`
/// and `super::` paths relative to `current_module`, inside the package at
/// `crate_root` ([`FileContext::crate_root`]).
///
/// `self::` and `super::` are rewritten to the equivalent `crate::` path
/// (see [`rewrite_relative_path`]) and resolved exactly as a `crate::` path
/// would be (see [`resolve_target_scopes`]); a `super::` that climbs above
/// the crate root, or a `self::` / `super::` path with no `current_module`
/// (the file itself has none, see [`FileContext::crate_module`]), resolves to
/// no scope. Bare paths (no recognized prefix) are left unresolved: following
/// them would require resolving `pub use` re-export chains, which is out of
/// scope (see [`ScopeDepRule`] limitations).
///
/// Callers should pass `current_module` only when `path` actually needs it
/// (`is_relative_path(path)`), so a `crate::`/bare/external path never pays
/// for computing the caller's current module.
pub(super) fn resolve_relative_target_scopes<'a>(
    config: &'a DeclarativeConfig,
    crate_root: &std::path::Path,
    current_module: Option<&[String]>,
    path: &str,
) -> Vec<&'a crate::declarative::model::ScopeName> {
    if path.starts_with("crate::") {
        return resolve_target_scopes(config, crate_root, path);
    }
    if is_relative_path(path) {
        let Some(current_module) = current_module else {
            return vec![];
        };
        return match rewrite_relative_path(path, current_module) {
            Some(rewritten) => resolve_target_scopes(config, crate_root, &rewritten),
            None => vec![],
        };
    }
    vec![]
}

impl<'ast> Visit<'ast> for ScopeDepVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let mark = self.scopes.enter_item_mod(&node.attrs);
        let pushed_mod = node.content.is_some();
        if pushed_mod {
            self.mod_stack.push(node.ident.to_string());
        }
        syn::visit::visit_item_mod(self, node);
        if pushed_mod {
            self.mod_stack.pop();
        }
        self.scopes.exit(mark);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let mark = self.scopes.enter_item_fn(&node.attrs);
        syn::visit::visit_item_fn(self, node);
        self.scopes.exit(mark);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let mark = self.scopes.enter_block(&node.attrs);
        syn::visit::visit_item_impl(self, node);
        self.scopes.exit(mark);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            // Only `self::`/`super::` paths need the current module; a
            // `crate::`/bare/external path never pays for computing it.
            let current_module = is_relative_path(&use_item.path)
                .then(|| self.current_module())
                .flatten();
            let target_scopes = resolve_relative_target_scopes(
                self.config,
                self.ctx.crate_root(),
                current_module.as_deref(),
                &use_item.path,
            );

            for (index, dep) in self.applicable.iter().enumerate() {
                for target_scope in &target_scopes {
                    if !dep.is_denied(target_scope) {
                        continue;
                    }
                    match self.scopes.decide(
                        index,
                        self.ctx,
                        self.code,
                        use_item.span,
                        Some(&node.attrs),
                    ) {
                        Decision::Suppressed => {}
                        Decision::MissingReason(warning) => self.violations.push(*warning),
                        Decision::Report => {
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
        }

        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // `use` statements contain `UseTree` nodes, not `syn::Path`,
        // so this never double-reports an import already flagged above.
        if let Some(path) = inline_path_to_string(node) {
            let current_module = is_relative_path(&path)
                .then(|| self.current_module())
                .flatten();
            let target_scopes = resolve_relative_target_scopes(
                self.config,
                self.ctx.crate_root(),
                current_module.as_deref(),
                &path,
            );

            for (index, dep) in self.applicable.iter().enumerate() {
                if !dep.check_inline() {
                    continue;
                }
                for target_scope in &target_scopes {
                    if !dep.is_denied(target_scope) {
                        continue;
                    }
                    match self
                        .scopes
                        .decide(index, self.ctx, self.code, node.span(), None)
                    {
                        Decision::Suppressed => {}
                        Decision::MissingReason(warning) => self.violations.push(*warning),
                        Decision::Report => {
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
    use std::path::Path;

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
        let scopes = resolve_target_scopes(&config, Path::new(""), "sqlx::Pool");
        assert!(scopes.is_empty());
    }

    #[test]
    fn resolve_target_scopes_self_path_returns_empty() {
        let config = make_scope_dep_config();
        let scopes = resolve_target_scopes(&config, Path::new(""), "self::utils::helper");
        assert!(scopes.is_empty());
    }

    /// The candidates are built under the crate root, so the same path
    /// resolves to a different scope (or none) per package.
    #[test]
    fn resolve_target_scopes_is_relative_to_the_crate_root() {
        let scopes = vec![
            Scope::new(
                ScopeName::new("a-infra").unwrap(),
                vec![GlobPattern::new("crates/a/src/infra/**").unwrap()],
            ),
            Scope::new(
                ScopeName::new("b-infra").unwrap(),
                vec![GlobPattern::new("crates/b/src/infra/**").unwrap()],
            ),
        ];
        let config = DeclarativeConfig::new(scopes, vec![], vec![], vec![]).unwrap();
        let names = |crate_root: &str| -> Vec<String> {
            resolve_target_scopes(&config, Path::new(crate_root), "crate::infra::db::Pool")
                .iter()
                .map(|s| s.as_str().to_string())
                .collect()
        };

        assert_eq!(names("crates/a"), vec!["a-infra"]);
        assert_eq!(names("crates/b"), vec!["b-infra"]);
        assert!(names("").is_empty());
        assert!(names("crates/c").is_empty());
    }

    // ── rewrite_relative_path ──

    #[test]
    fn rewrite_self_path_prepends_current_module() {
        let current = vec!["domain".to_string(), "service".to_string()];
        assert_eq!(
            rewrite_relative_path("self::helpers::f", &current).as_deref(),
            Some("crate::domain::service::helpers::f")
        );
    }

    #[test]
    fn rewrite_double_super_climbs_two_levels() {
        let current = vec!["domain".to_string(), "service".to_string()];
        assert_eq!(
            rewrite_relative_path("super::super::infra::db::Pool", &current).as_deref(),
            Some("crate::infra::db::Pool")
        );
    }

    #[test]
    fn rewrite_super_above_crate_root_is_none() {
        let current = vec!["domain".to_string()];
        assert!(rewrite_relative_path("super::super::infra::db::Pool", &current).is_none());
    }

    // ── resolve_relative_target_scopes ──

    /// Direct (non-end-to-end) check that resolution, not just same-scope
    /// coincidence, is what makes a single `super::` from
    /// `src/domain/service.rs` land in `domain`: it must resolve to the
    /// scope `domain` specifically, not merely "no edge/violation".
    #[test]
    fn resolve_relative_target_scopes_single_super_resolves_to_domain() {
        let config = make_scope_dep_config();
        let current_module = vec!["domain".to_string(), "service".to_string()];
        let scopes = resolve_relative_target_scopes(
            &config,
            Path::new(""),
            Some(&current_module),
            "super::entity::User",
        );
        let names: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
        assert_eq!(names, vec!["domain"]);
    }

    #[test]
    fn resolve_relative_target_scopes_none_current_module_is_unresolved() {
        let config = make_scope_dep_config();
        assert!(
            resolve_relative_target_scopes(&config, Path::new(""), None, "self::helpers::f")
                .is_empty(),
            "a self::/super:: path with no current module (file outside src/, or src/bin/) \
             must not resolve"
        );
    }

    // ── acceptance: self:: / super:: resolution (issue #5) ──

    /// Acceptance 1: `use super::super::infra::db::Pool;` in
    /// `src/domain/service.rs` resolves to `crate::infra::db::Pool`
    /// (two `super`s from `crate::domain::service`) and is reported by
    /// `[[deny-scope-dep]] from = "domain" to = ["infra"]`.
    #[test]
    fn scope_dep_detects_double_super_use_path() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use super::super::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].code, SCOPE_DEP_CODE);
        assert!(violations[0]
            .message
            .contains("super::super::infra::db::Pool"));
        assert!(violations[0].message.contains("infra"));
    }

    /// Acceptance 2: `use self::helpers::f;` in `src/domain/mod.rs`
    /// resolves to `crate::domain::helpers::f` — scope `domain`, same as the
    /// file's own scope — so it produces no violation.
    #[test]
    fn scope_dep_self_path_same_scope_is_allowed() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "use self::helpers::f;";
        let ctx = make_ctx("src/domain/mod.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    /// Bonus acceptance case: an inline `super::` path (not just a `use`
    /// item) is resolved and checked the same way. Two `super`s, exactly as
    /// in acceptance 1's `use` case: from `crate::domain::service` one
    /// `super` lands on `crate::domain`, two on the crate root, where
    /// `infra` is a real sibling scope of `domain`.
    #[test]
    fn scope_dep_detects_inline_super_path() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "fn f() { super::super::infra::db::connect(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0]
            .message
            .contains("super::super::infra::db::connect"));
    }

    /// `self::` / `super::` inside an inline `mod` block resolve relative to
    /// that inline module, not the file's own module: three `super`s are
    /// needed here to reach the crate root (one for `wiring`, one for
    /// `service`, one for `domain`), proving the mod stack is tracked.
    #[test]
    fn scope_dep_super_climbs_relative_to_inline_mod() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "mod wiring {\n    use super::super::super::infra::db::Pool;\n}\n";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("infra"));
    }

    /// `self::` inside an inline `mod` block still resolves within the same
    /// file's scope (`domain`), so it stays allowed.
    #[test]
    fn scope_dep_self_path_inside_inline_mod_stays_in_scope() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "mod wiring {\n    use self::helpers::f;\n}\n";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    /// A bodyless `mod helpers;` declaration must not push onto the mod
    /// stack: it has no content for `super::`/`self::` inside it to be
    /// relative to. A following sibling `use` at file level must still
    /// resolve against the *file's* module, not get stuck one level deep.
    #[test]
    fn scope_dep_bodyless_mod_declaration_does_not_affect_following_use() {
        let config = make_scope_dep_config();
        let rule = ScopeDepRule::new(config);
        let code = "mod helpers;\nuse super::super::infra::db::Pool;\n";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert_eq!(violations.len(), 1, "{violations:#?}");
    }

    /// A `super::` that would climb above the crate root resolves to no
    /// scope, so no violation fires even though a same-named
    /// `[[deny-scope-dep]]` would otherwise catch a reference to `infra`.
    #[test]
    fn scope_dep_super_above_crate_root_is_unresolved() {
        let scopes = vec![
            Scope::new(
                ScopeName::new("root").unwrap(),
                vec![GlobPattern::new("src/*.rs").unwrap()],
            ),
            Scope::new(
                ScopeName::new("infra").unwrap(),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
        ];
        let deps = vec![ScopeDep::new(
            Some("no-root-to-infra".to_string()),
            ScopeName::new("root").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "Root must not depend on infra.".to_string(),
            None,
            Severity::Error,
        )];
        let config = Arc::new(DeclarativeConfig::new(scopes, vec![], vec![], deps).unwrap());
        let rule = ScopeDepRule::new(config);
        // src/lib.rs's module is the crate root ([]); one `super::` from
        // there has nothing to climb above, and must not resolve.
        let code = "use super::infra::db::Pool;";
        let ctx = make_ctx("src/lib.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty(), "{violations:#?}");
    }

    // ── suppression ──

    fn check_domain(code: &str) -> Vec<Violation> {
        let rule = ScopeDepRule::new(make_scope_dep_config());
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);
        rule.check(&ctx, &ast)
    }

    #[test]
    fn suppress_attribute_on_the_use_item() {
        let code =
            "#[arch_lint::allow(no_domain_to_infra, reason = \"bootstrap\")]\nuse crate::infra::db::Pool;";
        assert!(check_domain(code).is_empty());
    }

    /// Acceptance 1/2 (kebab-attribute-allow): `allow(deny-scope-dep)` by
    /// kind name, in attribute form, suppresses the violation — the
    /// `snake_case` spelling (`deny_scope_dep`, below) and the code
    /// (`ALD003`, below) are the regression check that the fix did not
    /// disturb what already worked.
    #[test]
    fn suppress_attribute_by_kind_user_and_code_names() {
        for name in [
            "deny-scope-dep",
            "deny_scope_dep",
            "no-domain-to-infra",
            "ALD003",
        ] {
            let code = format!(
                "#[arch_lint::allow({name}, reason = \"bootstrap\")]\nuse crate::infra::db::Pool;"
            );
            assert!(
                check_domain(&code).is_empty(),
                "allow({name}) as an attribute should suppress the violation"
            );
        }
    }

    #[test]
    fn suppress_attribute_on_enclosing_fn_for_inline_path() {
        let code = "#[arch_lint::allow(deny_scope_dep, reason = \"bootstrap\")]\nfn wire() {\n    crate::infra::db::connect();\n}";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let code = "#[arch_lint::allow(ALD003, reason = \"bootstrap\")]\nmod wiring {\n    use crate::infra::db::Pool;\n}";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_comment_by_kind_user_and_code_names() {
        for name in ["deny-scope-dep", "no-domain-to-infra", "ALD003"] {
            let code = format!(
                "// arch-lint: allow({name}) reason=\"bootstrap\"\nuse crate::infra::db::Pool;"
            );
            assert!(
                check_domain(&code).is_empty(),
                "allow({name}) should suppress the violation"
            );
        }
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let code = "// arch-lint: allow(no-domain-to-infra)\nuse crate::infra::db::Pool;";
        let violations = check_domain(code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].rule, "no-domain-to-infra");
        assert!(violations[0].message.contains("missing required reason"));
    }

    /// Acceptance 4 of the trailing-allow-comment fix: the same
    /// missing-reason warning fires for the trailing form of the comment.
    #[test]
    fn trailing_comment_without_reason_on_error_rule_warns_instead() {
        let code = "use crate::infra::db::Pool; // arch-lint: allow(no-domain-to-infra)";
        let violations = check_domain(code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].rule, "no-domain-to-infra");
        assert!(violations[0].message.contains("missing required reason"));
    }

    #[test]
    fn user_name_allow_does_not_suppress_a_sibling_rule() {
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
        let deps = vec![
            ScopeDep::new(
                Some("no-domain-to-infra".to_string()),
                ScopeName::new("domain").unwrap(),
                vec![ScopeName::new("infra").unwrap()],
                "Domain must not depend on infra.".to_string(),
                None,
                Severity::Error,
            ),
            ScopeDep::new(
                Some("domain-stays-pure".to_string()),
                ScopeName::new("domain").unwrap(),
                vec![ScopeName::new("infra").unwrap()],
                "Domain stays pure.".to_string(),
                None,
                Severity::Error,
            ),
        ];
        let config = Arc::new(DeclarativeConfig::new(scopes, vec![], vec![], deps).unwrap());
        let rule = ScopeDepRule::new(config);
        let code =
            "#[arch_lint::allow(no_domain_to_infra, reason = \"bootstrap\")]\nuse crate::infra::db::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].rule, "domain-stays-pure");
    }
}
