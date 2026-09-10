//! Declarative rule implementations (syn visitors).
//!
//! Converts domain model rules into [`Rule`](crate::rule::Rule) trait
//! implementations that analyze `syn` ASTs. One submodule per rule kind; this
//! module holds the re-exports and the helpers shared by two or more of them.

use quote::ToTokens;
use syn::spanned::Spanned;

use crate::context::FileContext;
use crate::declarative::model::ItemKind;
use crate::types::{Location, Severity, Violation};

mod deny_attr;
mod layering;
mod naming;
mod require_attr;
mod require_return;
mod require_use;
mod restrict_use;
mod scope_dep;

pub use deny_attr::DenyAttrRule;
pub use layering::LayeringRule;
pub use naming::NamingRule;
pub use require_attr::RequireAttrRule;
pub use require_return::RequireReturnRule;
pub use require_use::RequireUseRule;
pub use restrict_use::RestrictUseRule;
pub use scope_dep::ScopeDepRule;

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
// Test support
// ────────────────────────────────────────────

#[cfg(test)]
mod test_support {
    use crate::context::FileContext;

    pub(super) fn parse_file(code: &str) -> syn::File {
        syn::parse_file(code).expect("test code should parse")
    }

    pub(super) fn make_ctx<'a>(path: &'a str, content: &'a str) -> FileContext<'a> {
        crate::test_utils::file_ctx(path, content, false)
    }
}

// ────────────────────────────────────────────
// Tests (shared helpers)
// ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::test_support::parse_file;
    use super::*;

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
