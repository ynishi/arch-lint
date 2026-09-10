//! Declarative rule implementations (syn visitors).
//!
//! Converts domain model rules into [`Rule`] trait implementations
//! that analyze `syn` ASTs.

use std::sync::Arc;

use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::context::FileContext;
use crate::declarative::model::{
    DeclarativeConfig, DenyAttr, ItemKind, Naming, RequireAttr, RequireReturn, RequireUse,
    RestrictUse, ScopeDep,
};
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

/// Converts an inline qualified path (expression, type, trait bound, or
/// macro position — anywhere outside a `use` statement) to a flat
/// `::` separated string.
///
/// Returns `None` for paths that cannot denote a cross-scope reference:
/// single-segment paths (local names) and `self::` / `super::` relative
/// paths (v1 limitation, same as the use-based checks). Generic arguments
/// are dropped, so `crate::infra::Db::<T>` becomes `crate::infra::Db`;
/// paths nested inside generic arguments are visited separately by syn.
pub(crate) fn inline_path_to_string(path: &syn::Path) -> Option<String> {
    if path.segments.len() < 2 {
        return None;
    }
    let first = path.segments[0].ident.to_string();
    if first == "self" || first == "super" {
        return None;
    }
    Some(
        path.segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::"),
    )
}

// ────────────────────────────────────────────
// RestrictUseRule
// ────────────────────────────────────────────

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
/// Used by both [`ScopeDepRule`] (`ALD003`) and [`LayeringRule`] (`ALD004`);
/// `code` selects which code the violations are reported under.
struct ScopeDepVisitor<'a> {
    ctx: &'a FileContext<'a>,
    config: &'a DeclarativeConfig,
    applicable: Vec<&'a ScopeDep>,
    code: &'static str,
    violations: Vec<Violation>,
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

// ────────────────────────────────────────────
// LayeringRule
// ────────────────────────────────────────────

const LAYERING_NAME: &str = "layering";
const LAYERING_CODE: &str = "ALD004";

/// A per-file rule that enforces `[[layering]]` declarations.
///
/// Each layering is expanded into the equivalent [`ScopeDep`] constraints and
/// checked with the same machinery as [`ScopeDepRule`], but reported as
/// `ALD004`. A layer may depend on the layers below it; referencing a layer
/// above it — or, with `adjacent_only`, skipping past the next one — is a
/// violation.
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

// ────────────────────────────────────────────
// Item walking (shared by the item-level rules)
// ────────────────────────────────────────────

/// One item seen by the attribute / naming / return-type rules.
pub(crate) struct ItemView<'ast> {
    /// What kind of item this is.
    pub(crate) kind: ItemKind,
    /// Display name (identifier, or the rendered self type of an `impl`).
    pub(crate) name: String,
    /// The identifier, when the item has one.
    pub(crate) ident: Option<&'ast syn::Ident>,
    /// Span used for reporting (the identifier where there is one).
    pub(crate) span: proc_macro2::Span,
    /// Outer attributes on the item.
    pub(crate) attrs: &'ast [syn::Attribute],
    /// Whether the item is publicly reachable (see [`walk_items`]).
    pub(crate) is_pub: bool,
    /// The signature, for functions.
    pub(crate) sig: Option<&'ast syn::Signature>,
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// Walks module-level and impl-level items, calling `f` for each.
///
/// Function bodies are never entered, so items declared inside a `fn` are not
/// reported. Visibility is the item's own `pub` for free, module and inherent
/// impl items; items of a trait definition inherit the trait's visibility, and
/// items of a trait impl (and `impl` blocks themselves) count as public.
pub(crate) fn walk_items<'ast>(items: &'ast [syn::Item], f: &mut impl FnMut(&ItemView<'ast>)) {
    for item in items {
        match item {
            syn::Item::Struct(i) => emit_named(
                f,
                ItemKind::Struct,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Enum(i) => emit_named(
                f,
                ItemKind::Enum,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Union(i) => emit_named(
                f,
                ItemKind::Union,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Fn(i) => emit_named(
                f,
                ItemKind::Fn,
                &i.sig.ident,
                &i.attrs,
                is_public(&i.vis),
                Some(&i.sig),
            ),
            syn::Item::Type(i) => emit_named(
                f,
                ItemKind::Type,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Const(i) => emit_named(
                f,
                ItemKind::Const,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Static(i) => emit_named(
                f,
                ItemKind::Static,
                &i.ident,
                &i.attrs,
                is_public(&i.vis),
                None,
            ),
            syn::Item::Mod(i) => {
                emit_named(
                    f,
                    ItemKind::Mod,
                    &i.ident,
                    &i.attrs,
                    is_public(&i.vis),
                    None,
                );
                if let Some((_, nested)) = &i.content {
                    walk_items(nested, f);
                }
            }
            syn::Item::Trait(i) => walk_trait(i, f),
            syn::Item::Impl(i) => walk_impl(i, f),
            _ => {}
        }
    }
}

/// Emits a trait and its associated items; items inherit the trait's visibility.
fn walk_trait<'ast>(item: &'ast syn::ItemTrait, f: &mut impl FnMut(&ItemView<'ast>)) {
    let trait_is_pub = is_public(&item.vis);
    emit_named(
        f,
        ItemKind::Trait,
        &item.ident,
        &item.attrs,
        trait_is_pub,
        None,
    );
    for trait_item in &item.items {
        match trait_item {
            syn::TraitItem::Fn(m) => emit_named(
                f,
                ItemKind::Fn,
                &m.sig.ident,
                &m.attrs,
                trait_is_pub,
                Some(&m.sig),
            ),
            syn::TraitItem::Const(c) => {
                emit_named(f, ItemKind::Const, &c.ident, &c.attrs, trait_is_pub, None);
            }
            syn::TraitItem::Type(t) => {
                emit_named(f, ItemKind::Type, &t.ident, &t.attrs, trait_is_pub, None);
            }
            _ => {}
        }
    }
}

/// Emits an `impl` block and its items; items of a trait impl count as public.
fn walk_impl<'ast>(item: &'ast syn::ItemImpl, f: &mut impl FnMut(&ItemView<'ast>)) {
    f(&ItemView {
        kind: ItemKind::Impl,
        name: normalize_rendered(&item.self_ty.to_token_stream().to_string()),
        ident: None,
        span: item.self_ty.span(),
        attrs: &item.attrs,
        is_pub: true,
        sig: None,
    });

    let is_trait_impl = item.trait_.is_some();
    for impl_item in &item.items {
        match impl_item {
            syn::ImplItem::Fn(m) => emit_named(
                f,
                ItemKind::Fn,
                &m.sig.ident,
                &m.attrs,
                is_trait_impl || is_public(&m.vis),
                Some(&m.sig),
            ),
            syn::ImplItem::Const(c) => emit_named(
                f,
                ItemKind::Const,
                &c.ident,
                &c.attrs,
                is_trait_impl || is_public(&c.vis),
                None,
            ),
            syn::ImplItem::Type(t) => emit_named(
                f,
                ItemKind::Type,
                &t.ident,
                &t.attrs,
                is_trait_impl || is_public(&t.vis),
                None,
            ),
            _ => {}
        }
    }
}

fn emit_named<'ast>(
    f: &mut impl FnMut(&ItemView<'ast>),
    kind: ItemKind,
    ident: &'ast syn::Ident,
    attrs: &'ast [syn::Attribute],
    is_pub: bool,
    sig: Option<&'ast syn::Signature>,
) {
    f(&ItemView {
        kind,
        name: ident.to_string(),
        ident: Some(ident),
        span: ident.span(),
        attrs,
        is_pub,
        sig,
    });
}

/// Renders a path as `::` separated segments (generic arguments dropped).
pub(crate) fn render_path(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Renders one attribute as its `::` separated path plus, for `#[derive(..)]`,
/// the paths inside the derive list.
pub(crate) fn render_attr(attr: &syn::Attribute) -> (String, Vec<String>) {
    let path = render_path(attr.path());
    let derives = if path == "derive" {
        attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        )
        .map(|paths| paths.iter().map(render_path).collect())
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    (path, derives)
}

/// Normalizes a `ToTokens` rendering: no spaces around `<`, `>` and `::`,
/// exactly one space after `,`.
pub(crate) fn normalize_rendered(raw: &str) -> String {
    let mut s = raw.replace(" ::", "::").replace(":: ", "::");
    s = s.replace(" <", "<").replace("< ", "<");
    s = s.replace(" >", ">").replace("> ", ">");
    s = s.replace(" ,", ",").replace(", ", ",").replace(',', ", ");
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim().to_string()
}

/// Renders a function's return type: `()` when there is none.
pub(crate) fn render_return_type(sig: &syn::Signature) -> String {
    match &sig.output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, ty) => normalize_rendered(&ty.to_token_stream().to_string()),
    }
}

fn build_violation(
    code: &'static str,
    rule_name: &str,
    severity: Severity,
    ctx: &FileContext,
    span: proc_macro2::Span,
    message: String,
    doc_ref: Option<&str>,
) -> Violation {
    let start = span.start();
    let location = Location::new(ctx.relative_path.clone(), start.line, start.column + 1);
    let mut violation = Violation::new(code, rule_name, severity, location, message);
    if let Some(doc) = doc_ref {
        violation = violation.with_doc_ref(doc);
    }
    violation
}

// ────────────────────────────────────────────
// DenyAttrRule
// ────────────────────────────────────────────

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

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut |view| {
            let rendered: Vec<(String, Vec<String>)> = view.attrs.iter().map(render_attr).collect();

            for rule in &applicable {
                if !rule.applies_to(view.kind) {
                    continue;
                }
                let hit = rule.attrs().iter().find(|pattern| {
                    rendered
                        .iter()
                        .any(|(path, derives)| pattern.matches(path, derives))
                });
                if let Some(pattern) = hit {
                    violations.push(build_violation(
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
                    ));
                }
            }
        });
        violations
    }
}

// ────────────────────────────────────────────
// RequireAttrRule
// ────────────────────────────────────────────

const REQUIRE_ATTR_NAME: &str = "require-attr";
const REQUIRE_ATTR_CODE: &str = "ALD006";

/// A per-file rule that enforces `[[require-attr]]` declarations.
///
/// Flags items of the configured kinds and visibility that carry none of the
/// required attributes.
pub struct RequireAttrRule {
    config: Arc<DeclarativeConfig>,
}

impl RequireAttrRule {
    /// Creates a new require-attr rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl Rule for RequireAttrRule {
    fn name(&self) -> &'static str {
        REQUIRE_ATTR_NAME
    }

    fn code(&self) -> &'static str {
        REQUIRE_ATTR_CODE
    }

    fn description(&self) -> &'static str {
        "Require specified attributes on items within a scope"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let applicable: Vec<&RequireAttr> = self
            .config
            .require_attrs()
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
            let rendered: Vec<(String, Vec<String>)> = view.attrs.iter().map(render_attr).collect();

            for rule in &applicable {
                if !rule.applies_to(view.kind) || !rule.visibility().matches(view.is_pub) {
                    continue;
                }
                let present = rule.attrs().iter().any(|pattern| {
                    rendered
                        .iter()
                        .any(|(path, derives)| pattern.matches(path, derives))
                });
                if !present {
                    let expected = rule
                        .attrs()
                        .iter()
                        .map(|p| format!("`{}`", p.as_str()))
                        .collect::<Vec<_>>()
                        .join(" or ");
                    violations.push(build_violation(
                        REQUIRE_ATTR_CODE,
                        rule.name(),
                        rule.severity(),
                        ctx,
                        view.span,
                        format!(
                            "{}: `{} {}` is missing {}",
                            rule.message(),
                            view.kind,
                            view.name,
                            expected,
                        ),
                        rule.doc_ref(),
                    ));
                }
            }
        });
        violations
    }
}

// ────────────────────────────────────────────
// NamingRule
// ────────────────────────────────────────────

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

// ────────────────────────────────────────────
// RequireReturnRule
// ────────────────────────────────────────────

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

        let mut violations = Vec::new();
        walk_items(&ast.items, &mut |view| {
            let Some(sig) = view.sig else {
                return;
            };
            let rendered = render_return_type(sig);
            let is_async = sig.asyncness.is_some();
            for rule in &applicable {
                if !rule.visibility().matches(view.is_pub) || !rule.matches_asyncness(is_async) {
                    continue;
                }
                if !rule.pattern().is_match(&rendered) {
                    violations.push(build_violation(
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
                    ));
                }
            }
        });
        violations
    }
}

// ────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::*;

    fn parse_file(code: &str) -> syn::File {
        syn::parse_file(code).expect("test code should parse")
    }

    fn make_ctx<'a>(path: &'a str, content: &'a str) -> FileContext<'a> {
        crate::test_utils::file_ctx(path, content, false)
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

    // ── inline_path_to_string ──

    fn parse_type_path(code: &str) -> syn::Path {
        match syn::parse_str::<syn::Type>(code).expect("test type should parse") {
            syn::Type::Path(tp) => tp.path,
            _ => panic!("expected a type path"),
        }
    }

    #[test]
    fn inline_path_multi_segment() {
        let path = parse_type_path("crate::infra::Db");
        assert_eq!(
            inline_path_to_string(&path).as_deref(),
            Some("crate::infra::Db")
        );
    }

    #[test]
    fn inline_path_drops_generic_arguments() {
        let path = parse_type_path("crate::infra::Db<T>");
        assert_eq!(
            inline_path_to_string(&path).as_deref(),
            Some("crate::infra::Db")
        );
    }

    #[test]
    fn inline_path_skips_single_segment() {
        let path = parse_type_path("Db");
        assert!(inline_path_to_string(&path).is_none());
    }

    #[test]
    fn inline_path_skips_self_and_super() {
        assert!(inline_path_to_string(&parse_type_path("self::Db")).is_none());
        assert!(inline_path_to_string(&parse_type_path("super::Db")).is_none());
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

    // ── LayeringRule ──

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

    // ── DenyAttrRule ──

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

    // ── RequireAttrRule ──

    fn require_attr_rule(visibility: VisibilityFilter, on: Vec<ItemKind>) -> RequireAttrRule {
        let config = DeclarativeConfig::builder()
            .require_attrs(vec![RequireAttr::new(
                "instrument-handlers".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("**/*.rs").unwrap()]),
                vec![AttrPattern::new("tracing::instrument").unwrap()],
                on,
                "Handlers must be instrumented.".to_string(),
                None,
                Severity::Warning,
            )
            .with_visibility(visibility)])
            .build()
            .unwrap();
        RequireAttrRule::new(Arc::new(config))
    }

    #[test]
    fn require_attr_flags_missing_attribute() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "pub fn handler() {}");

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, REQUIRE_ATTR_CODE);
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(violations[0].message.contains("`tracing::instrument`"));
    }

    #[test]
    fn require_attr_accepts_present_attribute() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations =
            crate::test_utils::check_rule(&rule, "#[tracing::instrument]\npub fn handler() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn require_attr_pub_visibility_skips_private_fn() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "fn helper() {}");
        assert!(violations.is_empty());
    }

    #[test]
    fn require_attr_any_visibility_flags_private_fn() {
        let rule = require_attr_rule(VisibilityFilter::Any, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(&rule, "fn helper() {}");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn require_attr_treats_trait_impl_methods_as_public() {
        let rule = require_attr_rule(VisibilityFilter::Pub, vec![ItemKind::Fn]);
        let violations = crate::test_utils::check_rule(
            &rule,
            "struct S;\ntrait T { fn go(); }\nimpl T for S {\n    fn go() {}\n}",
        );
        // the trait declaration is private (`trait T`), the impl method counts as public
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].location.line, 4);
    }

    // ── NamingRule ──

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

    // ── RequireReturnRule ──

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

    // ── rendering helpers ──

    #[test]
    fn normalize_rendered_collapses_token_spacing() {
        assert_eq!(normalize_rendered("Result < T , E >"), "Result<T, E>");
        assert_eq!(
            normalize_rendered("Result < Vec < u8 > , AppError >"),
            "Result<Vec<u8>, AppError>"
        );
        assert_eq!(
            normalize_rendered("std :: string :: String"),
            "std::string::String"
        );
    }

    #[test]
    fn render_return_type_normalizes_generic_args() {
        let item: syn::ItemFn =
            syn::parse_str("fn f() -> Result<Vec<u8>, AppError> { todo!() }").unwrap();
        assert_eq!(render_return_type(&item.sig), "Result<Vec<u8>, AppError>");

        let unit: syn::ItemFn = syn::parse_str("fn f() {}").unwrap();
        assert_eq!(render_return_type(&unit.sig), "()");
    }

    #[test]
    fn render_attr_extracts_derive_paths() {
        let item: syn::ItemStruct =
            syn::parse_str("#[derive(Debug, sqlx::FromRow)]\nstruct S;").unwrap();
        let (path, derives) = render_attr(&item.attrs[0]);
        assert_eq!(path, "derive");
        assert_eq!(
            derives,
            vec!["Debug".to_string(), "sqlx::FromRow".to_string()]
        );

        let f: syn::ItemFn = syn::parse_str("#[tracing::instrument]\nfn f() {}").unwrap();
        let (path, derives) = render_attr(&f.attrs[0]);
        assert_eq!(path, "tracing::instrument");
        assert!(derives.is_empty());
    }
}
