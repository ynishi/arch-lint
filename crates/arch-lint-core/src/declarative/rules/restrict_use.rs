//! `[[restrict-use]]` (ALD001): denies configured imports and inline
//! qualified paths within a scope.

use std::sync::Arc;

use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{DeclarativeConfig, RestrictUse};
use crate::rule::Rule;
use crate::types::{Location, Violation};

use super::suppress::ScopeMark;
use super::use_bindings::{Mark, Resolution, UseBindings};
use super::{expand_use_tree, inline_path_to_string, Decision, Suppressions};

const RESTRICT_USE_NAME: &str = "restrict-use";
const RESTRICT_USE_CODE: &str = "ALD001";

/// A per-file rule that enforces `[[restrict-use]]` declarations.
///
/// For each file, determines which restrict-use rules apply based on
/// scope membership, then checks every `use` import — and, unless
/// `check-inline = false`, every inline qualified path — against the deny list.
///
/// An inline path is matched as written, and also as the `use` declarations of
/// the file resolve it: with `use uuid::Uuid;` in scope, `Uuid::new_v7()` is
/// matched as `uuid::Uuid::new_v7` too (see `UseBindings` for what is
/// resolved). A path is reported once, as written if that is denied. The
/// resolved path is tried only when the import it goes through (`uuid::Uuid`)
/// is neither denied nor excepted by the rule: the `use` line has already
/// decided that, so `deny = ["sqlx::**"]` reports `use sqlx::Pool;` and not
/// every `Pool::connect()` after it, and an allow on the `use` line is not
/// undone where the name is used.
///
/// Three limits follow from these rules:
///
/// - Asymmetry of `except`: with `deny = ["uuid::**"]` and
///   `except = ["uuid::Uuid"]`, `use uuid::Uuid;` makes `Uuid::new_v7()`
///   acceptable, while a written `uuid::Uuid::new_v7()` is reported. To forbid
///   the function, deny it by name (`uuid::Uuid::new_v7`) rather than excepting
///   the type.
/// - A name that resolves to an item of the same file (`mod uuid { .. }`) is
///   matched as written only, so it can still hit a deny for an external
///   crate of that name. That is how paths were matched before.
/// - A glob import in an inner block can hide a named import of the outer
///   scope, but globs are not resolved, so the outer import is used and the
///   path may be reported wrongly. Suppress it with an allow.
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

        let scopes = Suppressions::new(
            RESTRICT_USE_NAME,
            RESTRICT_USE_CODE,
            applicable
                .iter()
                .map(|r| (r.name().to_string(), r.severity())),
        );

        let mut visitor = RestrictUseVisitor {
            ctx,
            applicable,
            scopes,
            bindings: UseBindings::default(),
            violations: Vec::new(),
        };
        visitor.visit_file(ast);
        visitor.violations
    }
}

/// Opens a suppression scope for the attributes of the node being entered.
type EnterScope = fn(&mut Suppressions, &[syn::Attribute]) -> ScopeMark;

struct RestrictUseVisitor<'a> {
    ctx: &'a FileContext<'a>,
    applicable: Vec<&'a RestrictUse>,
    scopes: Suppressions,
    bindings: UseBindings,
    violations: Vec<Violation>,
}

impl RestrictUseVisitor<'_> {
    /// Runs `body` inside the suppression scope (when there is one) and the
    /// name-binding frame that `frame` opens, and leaves both afterwards, so
    /// the two stacks cannot drift apart.
    fn scoped(
        &mut self,
        suppress: Option<(&[syn::Attribute], EnterScope)>,
        frame: impl FnOnce(&mut UseBindings) -> Mark,
        body: impl FnOnce(&mut Self),
    ) {
        let scope_mark = suppress.map(|(attrs, enter)| enter(&mut self.scopes, attrs));
        let frame_mark = frame(&mut self.bindings);
        body(self);
        self.bindings.exit(frame_mark);
        if let Some(mark) = scope_mark {
            self.scopes.exit(mark);
        }
    }

    /// Opens only the frame for the type parameters of `generics`.
    fn with_generics(&mut self, generics: &syn::Generics, body: impl FnOnce(&mut Self)) {
        self.scoped(None, |bindings| bindings.enter_generics(generics), body);
    }

    /// Opens the frame for the type parameters of a struct, enum, union or
    /// trait, where `Self` is the item itself and not an enclosing `impl`.
    fn with_own_self(&mut self, generics: &syn::Generics, body: impl FnOnce(&mut Self)) {
        self.scoped(
            None,
            |bindings| {
                let mark = bindings.enter_generics(generics);
                bindings.hide_self();
                mark
            },
            body,
        );
    }
}

impl<'ast> Visit<'ast> for RestrictUseVisitor<'_> {
    fn visit_file(&mut self, node: &'ast syn::File) {
        self.scoped(
            None,
            |bindings| bindings.enter_module(&node.items),
            |v| syn::visit::visit_file(v, node),
        );
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.scoped(
            Some((&node.attrs, Suppressions::enter_item_mod)),
            |bindings| match &node.content {
                Some((_, items)) => bindings.enter_module(items),
                // `mod foo;` has no body to scope: open nothing.
                None => bindings.enter_block(&[]),
            },
            |v| syn::visit::visit_item_mod(v, node),
        );
    }

    fn visit_block(&mut self, node: &'ast syn::Block) {
        self.scoped(
            None,
            |bindings| bindings.enter_block(&node.stmts),
            |v| syn::visit::visit_block(v, node),
        );
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.scoped(
            Some((&node.attrs, Suppressions::enter_item_fn)),
            |bindings| bindings.enter_generics(&node.sig.generics),
            |v| syn::visit::visit_item_fn(v, node),
        );
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.scoped(
            Some((&node.attrs, Suppressions::enter_block)),
            |bindings| {
                // `Self` is bound inside the generics, so a type parameter
                // as the self type is seen; `exit` leaves both frames.
                let mark = bindings.enter_generics(&node.generics);
                bindings.enter_self(&node.self_ty);
                mark
            },
            |v| syn::visit::visit_item_impl(v, node),
        );
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.with_own_self(&node.generics, |v| syn::visit::visit_item_trait(v, node));
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        self.with_own_self(&node.generics, |v| syn::visit::visit_item_struct(v, node));
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.with_own_self(&node.generics, |v| syn::visit::visit_item_enum(v, node));
    }

    fn visit_item_union(&mut self, node: &'ast syn::ItemUnion) {
        self.with_own_self(&node.generics, |v| syn::visit::visit_item_union(v, node));
    }

    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        self.with_generics(&node.generics, |v| syn::visit::visit_item_type(v, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.with_generics(&node.sig.generics, |v| {
            syn::visit::visit_impl_item_fn(v, node);
        });
    }

    fn visit_impl_item_type(&mut self, node: &'ast syn::ImplItemType) {
        self.with_generics(&node.generics, |v| {
            syn::visit::visit_impl_item_type(v, node);
        });
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.with_generics(&node.sig.generics, |v| {
            syn::visit::visit_trait_item_fn(v, node);
        });
    }

    fn visit_trait_item_type(&mut self, node: &'ast syn::TraitItemType) {
        self.with_generics(&node.generics, |v| {
            syn::visit::visit_trait_item_type(v, node);
        });
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let resolved = expand_use_tree(&node.tree, "");

        for use_item in &resolved {
            for (index, rule) in self.applicable.iter().enumerate() {
                if !rule.is_denied(&use_item.path) {
                    continue;
                }
                match self.scopes.decide(
                    index,
                    self.ctx,
                    RESTRICT_USE_CODE,
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
        }

        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // `use` statements contain `UseTree` nodes, not `syn::Path`,
        // so this never double-reports an import already flagged above.
        if let Some(spelled) = inline_path_to_string(node) {
            // Resolved on first need: most paths are denied by no rule.
            let mut imported: Option<Option<(Option<String>, String)>> = None;
            for (index, rule) in self.applicable.iter().enumerate() {
                if !rule.check_inline() {
                    continue;
                }
                // The path as written wins. Only when it is not denied is
                // the path through the import tried, and only if the rule
                // has no opinion on the import itself: the `use` line has
                // already decided that, and deciding again here would
                // report it twice or break its suppression.
                let shown = if rule.is_denied(&spelled) {
                    format!("`{spelled}`")
                } else if let Some((_, full)) = imported
                    .get_or_insert_with(|| match self.bindings.resolve(node) {
                        Resolution::Imported { decided, full } => Some((decided, full)),
                        Resolution::Local | Resolution::Unbound => None,
                    })
                    .as_ref()
                    .filter(|(decided, full)| {
                        !decided.as_deref().is_some_and(|d| rule.mentions(d))
                            && rule.is_denied(full)
                    })
                {
                    format!("`{spelled}` (resolves to `{full}`)")
                } else {
                    continue;
                };
                match self
                    .scopes
                    .decide(index, self.ctx, RESTRICT_USE_CODE, node.span(), None)
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
                            RESTRICT_USE_CODE,
                            rule.name(),
                            rule.severity(),
                            location,
                            format!("{}: {shown}", rule.message()),
                        );
                        if let Some(doc) = rule.doc_ref() {
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
    fn restrict_inline_matches_self_and_super_paths_verbatim() {
        // `self::helper` / `super::helper` don't match the `sqlx::*` deny
        // pattern of `make_restrict_config`, so this stays empty — but that
        // is because the *pattern* doesn't match, not because `self::` /
        // `super::` inline paths are skipped (see the next test for proof
        // they are matched like any other path, verbatim, same as `use`
        // items already were).
        let config = make_restrict_config();
        let rule = RestrictUseRule::new(config);
        let code = "fn f() { self::helper(); super::helper(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);
        assert!(violations.is_empty());
    }

    #[test]
    fn restrict_inline_super_path_matches_a_super_prefixed_deny_pattern() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![RestrictUse::new(
            "no-super-helper-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("super::*").unwrap()],
            "Domain must not reach into the parent module.".to_string(),
            None,
            Severity::Error,
        )];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        let code = "fn f() { super::helper(); }";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("super::helper"));
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

    // ── suppression: attribute, comment, name forms ──

    fn check_domain(code: &str) -> Vec<Violation> {
        let rule = RestrictUseRule::new(make_restrict_config());
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);
        rule.check(&ctx, &ast)
    }

    #[test]
    fn suppress_attribute_on_the_use_item() {
        let code = "#[arch_lint::allow(no_sqlx_in_domain, reason = \"legacy\")]\nuse sqlx::Pool;";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_mod() {
        let code = "#[arch_lint::allow(restrict_use, reason = \"legacy\")]\nmod inner {\n    use sqlx::Pool;\n}";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_fn() {
        let code = "#[arch_lint::allow(no_sqlx_in_domain, reason = \"legacy\")]\nfn f() {\n    let _ = sqlx::query(\"SELECT 1\");\n}";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_attribute_on_enclosing_impl() {
        let code = "struct S;\n#[arch_lint::allow(no_sqlx_in_domain, reason = \"legacy\")]\nimpl S {\n    fn f(&self) {\n        let _ = sqlx::query(\"SELECT 1\");\n    }\n}";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_inline_comment_on_previous_line() {
        let code = "// arch-lint: allow(no-sqlx-in-domain) reason=\"legacy\"\nuse sqlx::Pool;";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_inline_comment_on_the_same_line() {
        let code = "use sqlx::Pool; // arch-lint: allow(no-sqlx-in-domain) reason=\"legacy\"";
        // Trailing-allow-comment fix: the directive no longer has to be the
        // whole comment line — a comment after the offending code, on the
        // same line, suppresses the violation on *that* line. This is not
        // identical to a comment on the line above: a leading comment
        // targets the line *below* it, while a trailing comment targets
        // only its own line — it does not also reach the next line (see
        // `scope_dep.rs`'s trailing-allow-comment tests).
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn suppress_by_kind_name_user_name_and_code() {
        for name in ["restrict-use", "no-sqlx-in-domain", "ALD001"] {
            let code = format!("// arch-lint: allow({name}) reason=\"legacy\"\nuse sqlx::Pool;");
            assert!(
                check_domain(&code).is_empty(),
                "allow({name}) should suppress the violation"
            );
        }
    }

    #[test]
    fn suppress_by_unrelated_name_does_not_apply() {
        let code = "// arch-lint: allow(no-reqwest-in-domain) reason=\"legacy\"\nuse sqlx::Pool;";
        assert_eq!(check_domain(code).len(), 1);
    }

    #[test]
    fn user_name_allow_does_not_suppress_a_sibling_rule() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![
            RestrictUse::new(
                "no-sqlx-in-domain".to_string(),
                ScopeRef::Named(ScopeName::new("domain").unwrap()),
                vec![UsePattern::new("sqlx::*").unwrap()],
                "Domain must be DB-agnostic.".to_string(),
                None,
                Severity::Error,
            ),
            RestrictUse::new(
                "no-orm-in-domain".to_string(),
                ScopeRef::Named(ScopeName::new("domain").unwrap()),
                vec![UsePattern::new("sqlx::*").unwrap()],
                "Domain must not touch the ORM.".to_string(),
                None,
                Severity::Error,
            ),
        ];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        let code = "#[arch_lint::allow(no_sqlx_in_domain, reason = \"legacy\")]\nuse sqlx::Pool;";
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);

        let violations = rule.check(&ctx, &ast);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].rule, "no-orm-in-domain");
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let code = "// arch-lint: allow(no-sqlx-in-domain)\nuse sqlx::Pool;";
        let violations = check_domain(code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert_eq!(violations[0].code, RESTRICT_USE_CODE);
        assert_eq!(violations[0].rule, "no-sqlx-in-domain");
        assert!(violations[0].message.contains("missing required reason"));
    }

    #[test]
    fn comment_without_reason_on_warning_rule_suppresses() {
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
        let code = "// arch-lint: allow(no-sqlx-handlers)\nuse sqlx::Pool;";
        let ctx = make_ctx("src/handlers/api.rs", code);
        let ast = parse_file(code);

        assert!(rule.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn attribute_without_reason_suppresses_like_built_in_rules() {
        // The built-in rules only require a reason on the *comment* path, so an
        // attribute without one suppresses silently here too.
        let code = "#[arch_lint::allow(no_sqlx_in_domain)]\nuse sqlx::Pool;";
        assert!(check_domain(code).is_empty());
    }

    #[test]
    fn cfg_test_module_is_still_checked() {
        let code = "#[cfg(test)]\nmod tests {\n    use sqlx::Pool;\n}";
        assert_eq!(
            check_domain(code).len(),
            1,
            "declarative rules express architecture constraints and stay on in test code"
        );
    }

    #[test]
    fn scope_is_restored_after_the_allowed_item() {
        let code = "#[arch_lint::allow(no_sqlx_in_domain, reason = \"legacy\")]\nmod allowed {\n    use sqlx::Pool;\n}\nmod plain {\n    use sqlx::Executor;\n}";
        let violations = check_domain(code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("sqlx::Executor"));
    }

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

    // ── names bound by `use` ──

    /// Runs one rule over `src/domain/service.rs` with the given patterns.
    fn check_names(deny: &[&str], except: &[&str], code: &str) -> Vec<Violation> {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let pattern = |p: &&str| UsePattern::new(p).unwrap();
        let restrict = vec![RestrictUse::new(
            "no-id-gen".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            deny.iter().map(pattern).collect(),
            "No id generation in domain.".to_string(),
            None,
            Severity::Error,
        )
        .with_except(except.iter().map(pattern).collect())];
        let config = Arc::new(DeclarativeConfig::new(scopes, restrict, vec![], vec![]).unwrap());
        let rule = RestrictUseRule::new(config);
        let ctx = make_ctx("src/domain/service.rs", code);
        let ast = parse_file(code);
        rule.check(&ctx, &ast)
    }

    const NEW_V7: &[&str] = &["uuid::Uuid::new_v7"];

    #[test]
    fn name_bound_by_use_is_matched_as_the_imported_path() {
        let code = "use uuid::Uuid;\nfn f() { let _ = Uuid::new_v7(); }";
        let violations = check_names(NEW_V7, &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 2);
    }

    #[test]
    fn renamed_import_is_matched_as_the_original_path() {
        let code = "use uuid::Uuid as U;\nfn f() { let _ = U::new_v7(); }";
        assert_eq!(check_names(NEW_V7, &[], code).len(), 1);
    }

    #[test]
    fn self_in_a_group_binds_the_parent_name() {
        let deny = ["chrono::Utc::now"];
        let code =
            "use chrono::{self, Utc};\nfn f() { let _ = Utc::now(); let _ = chrono::Utc::now(); }";
        let violations = check_names(&deny, &[], code);

        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].message.contains("(resolves to"));
        assert!(!violations[1].message.contains("(resolves to"));
    }

    #[test]
    fn use_inside_a_fn_body_binds_until_the_body_ends() {
        let code = "fn f() { use uuid::Uuid; let _ = Uuid::new_v7(); }\nfn g() { let _ = Uuid::new_v7(); }";
        let violations = check_names(NEW_V7, &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 1);
    }

    #[test]
    fn nested_mod_does_not_see_the_outer_import() {
        let outer = "use uuid::Uuid;\nmod inner { fn f() { let _ = Uuid::new_v7(); } }";
        assert!(check_names(NEW_V7, &[], outer).is_empty());

        let own = "mod inner { use uuid::Uuid; fn f() { let _ = Uuid::new_v7(); } }";
        assert_eq!(check_names(NEW_V7, &[], own).len(), 1);
    }

    #[test]
    fn struct_in_a_block_hides_the_outer_import() {
        let code = "use uuid::Uuid;\nfn f() { struct Uuid; let _ = Uuid::new_v7(); }";
        assert!(check_names(NEW_V7, &[], code).is_empty());
    }

    #[test]
    fn const_and_fn_in_a_block_do_not_hide_the_outer_import() {
        let code = "use uuid::Uuid;\nfn a() { const Uuid: u8 = 0; let _ = Uuid::new_v7(); }\nfn b() { fn Uuid() {} let _ = Uuid::new_v7(); }";
        assert_eq!(check_names(NEW_V7, &[], code).len(), 2);
    }

    #[test]
    fn glob_import_is_not_resolved() {
        let code = "use uuid::*;\nfn f() { let _ = Uuid::new_v7(); }";
        assert!(check_names(NEW_V7, &[], code).is_empty());
    }

    #[test]
    fn type_parameter_hides_the_outer_import() {
        let code = "use uuid::Uuid;\nfn f<Uuid: Tr>() { let _ = Uuid::new_v7(); }\nstruct S<Uuid: Tr>(Uuid::Out);";
        assert!(check_names(&["uuid::Uuid::*"], &[], code).is_empty());
    }

    #[test]
    fn self_in_an_impl_follows_the_self_type() {
        let code = "use uuid::Uuid;\nstruct W;\nimpl Tr for Uuid { fn f() { let _ = Self::new_v7(); } }\nimpl Tr for W { fn f() { let _ = Self::new_v7(); } }";
        let violations = check_names(NEW_V7, &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 3);
        assert!(violations[0]
            .message
            .contains("`Self::new_v7` (resolves to `uuid::Uuid::new_v7`)"));
    }

    #[test]
    fn extern_crate_rename_is_matched_as_the_crate() {
        let code = "extern crate uuid as id;\nfn f() { let _ = id::Uuid::new_v7(); }";
        assert_eq!(check_names(NEW_V7, &[], code).len(), 1);
    }

    #[test]
    fn extern_crate_is_resolved_even_when_the_rule_names_the_crate() {
        // No `extern crate` line is checked, so nothing has decided yet.
        let code = "extern crate uuid as id;\nfn f() { let _ = id::Uuid::new_v7(); }";
        assert_eq!(check_names(&["uuid::**"], &[], code).len(), 1);
    }

    #[test]
    fn denied_group_self_is_not_reported_again_through_its_alias() {
        // `chrono::*` denies the `use` leaf `chrono::self`, not `chrono`.
        let code = "use chrono::{self as c};\nfn f() { let _ = c::Utc; }";
        let violations = check_names(&["chrono::*"], &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 1);
    }

    #[test]
    fn item_in_an_impl_method_has_its_own_self() {
        let code = "use uuid::Uuid;\nimpl Tr for Uuid { fn f() { trait L { fn g() { let _ = Self::new_v7(); } } } }";
        assert!(check_names(NEW_V7, &[], code).is_empty());
    }

    #[test]
    fn except_applies_to_the_resolved_path() {
        let code = "use uuid::Uuid;\nfn f() { let _ = Uuid::new_v7(); let _ = Uuid::nil(); }";
        let violations = check_names(&["uuid::Uuid::*"], &["uuid::Uuid::nil"], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("uuid::Uuid::new_v7"));
    }

    #[test]
    fn short_deny_pattern_still_matches_the_path_as_written() {
        let code = "use uuid::Uuid;\nfn f() { let _ = Uuid::new_v7(); }";
        let violations = check_names(&["Uuid::new_v7"], &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(!violations[0].message.contains("resolves to"));
    }

    #[test]
    fn denied_import_is_not_reported_again_where_the_name_is_used() {
        let code = "use sqlx::Pool;\nfn f() { let _ = Pool::connect(); }";
        let violations = check_names(&["sqlx::**"], &[], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 1);
    }

    #[test]
    fn allowed_import_does_not_turn_red_where_the_name_is_used() {
        let code = "// arch-lint: allow(no-id-gen) reason=\"legacy\"\nuse sqlx::Pool;\nfn f() { let _ = Pool::connect(); }";
        assert!(check_names(&["sqlx::**"], &[], code).is_empty());
    }

    #[test]
    fn excepted_import_does_not_turn_red_where_the_name_is_used() {
        let code = "use sqlx::types::Json;\nfn f() { let _ = Json::from(1); }";
        let violations = check_names(&["sqlx::**"], &["sqlx::types::*"], code);

        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn resolved_violation_is_suppressed_at_the_use_site() {
        let by_comment = "use uuid::Uuid;\nfn f() {\n    // arch-lint: allow(no-id-gen) reason=\"legacy\"\n    let _ = Uuid::new_v7();\n}";
        assert!(check_names(NEW_V7, &[], by_comment).is_empty());

        let by_attribute = "use uuid::Uuid;\n#[arch_lint::allow(no_id_gen, reason = \"legacy\")]\nfn f() { let _ = Uuid::new_v7(); }";
        assert!(check_names(NEW_V7, &[], by_attribute).is_empty());
    }

    #[test]
    fn message_names_the_resolved_path_only_when_it_is_what_matched() {
        let code = "use uuid::Uuid;\nfn f() { let _ = Uuid::new_v7(); }";
        let resolved = check_names(NEW_V7, &[], code);
        assert_eq!(
            resolved[0].message,
            "No id generation in domain.: `Uuid::new_v7` (resolves to `uuid::Uuid::new_v7`)"
        );

        // Both the written and the resolved path are denied: one report, in
        // the written form.
        let both = check_names(&["Uuid::new_v7", "uuid::Uuid::new_v7"], &[], code);
        assert_eq!(both.len(), 1, "{both:#?}");
        assert_eq!(
            both[0].message,
            "No id generation in domain.: `Uuid::new_v7`"
        );
    }

    #[test]
    fn except_on_the_import_also_accepts_deeper_paths_through_it() {
        let code = "use uuid::Uuid;\nfn f() {\n    let _ = Uuid::new_v7();\n    let _ = uuid::Uuid::new_v7();\n}";
        let violations = check_names(&["uuid::**"], &["uuid::Uuid"], code);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].location.line, 4);
        assert!(!violations[0].message.contains("resolves to"));
    }

    #[test]
    fn type_parameters_hide_the_import_in_every_kind_of_item() {
        let templates = [
            "struct S<{P}: Tr> { f: Uuid::Out }",
            "enum E<{P}: Tr> { A(Uuid::Out) }",
            "union U<{P}: Tr> { f: Uuid::Out }",
            "type A<{P}: Tr> = Uuid::Out;",
            "trait T<{P}: Tr> { fn f() -> Uuid::Out; }",
            "struct S; impl S { fn f<{P}: Tr>() -> Uuid::Out { loop {} } }",
            "trait T { fn f<{P}: Tr>() -> Uuid::Out; }",
            "trait T { type A<{P}> = Uuid::Out; }",
            "struct S; impl Tr for S { type A<{P}> = Uuid::Out; }",
        ];
        for template in templates {
            let code = |param: &str| format!("use uuid::Uuid;\n{}", template.replace("{P}", param));
            // Control: another parameter name leaves the import visible.
            let visible = check_names(&["uuid::Uuid::Out"], &[], &code("T"));
            assert_eq!(visible.len(), 1, "{template}: {visible:#?}");

            let hidden = check_names(&["uuid::Uuid::Out"], &[], &code("Uuid"));
            assert!(hidden.is_empty(), "{template}: {hidden:#?}");
        }
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
