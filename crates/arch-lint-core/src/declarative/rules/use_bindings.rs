//! Name bindings of one file: what the head of an inline path stands for.
//!
//! `use uuid::Uuid;` binds `Uuid` to `uuid::Uuid`, so `Uuid::new_v7()` is the
//! path `uuid::Uuid::new_v7` written short. [`UseBindings`] keeps a stack of
//! scopes (frames) while a visitor walks a file and [`UseBindings::resolve`]
//! turns the head of a path into that canonical form, so a rule can match what
//! the code means rather than what it spells. Only the first segment of a path
//! of two or more segments is looked up, and Rust looks it up in the type
//! namespace, so that is the only namespace tracked here: `fn`, `const` and
//! `static` items (value namespace) neither bind nor hide a name.
//!
//! Scopes follow the Rust Reference: a `mod` (and the file itself) sees only
//! its own names, never those of the enclosing module; a block, a generic
//! parameter list and an `impl` see their own names and everything outside.
//! The names of a scope are collected when it is entered, so the order of
//! the items inside does not matter. Within one frame a local item wins over
//! an import of the same name (the type namespace cannot hold both).
//!
//! What it resolves: `use` (a name, a rename, `self` in a group),
//! `extern crate`, items of the type namespace (struct, enum, union, trait,
//! type alias, `mod`), type parameters, and `Self` inside an `impl`.
//!
//! What it does not follow, so that a name which depends on any of these
//! resolves to what is written, or to nothing:
//!
//! - glob imports, `use super::*;` in a nested `mod` included
//! - chains of `use super::X` / `use crate::X` / `use self::X`: the target
//!   stays as written (`super::Uuid`)
//! - `pub use` re-exports, type aliases (`type Id = uuid::Uuid;`) and
//!   bindings that cross files (`extern crate x as y;` in a crate root)
//! - qualified self types (`<Uuid>::new_v7()`), method call syntax, and the
//!   inside of macros
//! - a glob in an inner block hides an outer named import, but globs are not
//!   collected, so the outer import is still returned

use std::collections::HashMap;

use super::expand_use_tree;

/// What a name stands for in one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Binding {
    /// Made by `use`, `extern crate` or an `impl` (`Self`); `target` is the
    /// path as written. `decided` is the path a rule has already checked
    /// where the name was bound (the `use` leaf, `a::self` for `{self}`),
    /// or `None` when nothing checked it (`extern crate`).
    Import {
        target: String,
        decided: Option<String>,
    },
    /// An item of the type namespace declared in the scope, or a type
    /// parameter. It hides an import of the same name from outside.
    Local,
}

/// What the head of a path resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// The head is bound by an import. `full` is its target followed by the
    /// remaining segments of the path (`uuid::Uuid::new_v7`); `decided` is
    /// the path already checked where the name was bound, if any.
    Imported {
        decided: Option<String>,
        full: String,
    },
    /// The head is an item declared in the file, or a type parameter.
    Local,
    /// Nothing binds the head: an external crate, the prelude, a name from
    /// another file, or a path that does not start with a name.
    Unbound,
}

/// One scope: the names it declares, and whether lookup stops after it.
struct Frame {
    names: HashMap<String, Binding>,
    /// A module (or the file) does not see the names outside it.
    module: bool,
}

/// A position in the frame stack, returned by the `enter_*` methods and
/// handed back to [`UseBindings::exit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark(usize);

/// The stack of name scopes of the file being walked.
#[derive(Default)]
pub(crate) struct UseBindings {
    frames: Vec<Frame>,
}

impl UseBindings {
    /// Enters a module (or the file): its own names only, nothing from the
    /// enclosing module.
    pub(crate) fn enter_module(&mut self, items: &[syn::Item]) -> Mark {
        let mut names = HashMap::new();
        for item in items {
            collect_item(&mut names, item);
        }
        self.push(Frame {
            names,
            module: true,
        })
    }

    /// Enters a block: the items declared in it, on top of everything
    /// outside. No frame is made when the block declares nothing.
    pub(crate) fn enter_block(&mut self, stmts: &[syn::Stmt]) -> Mark {
        let mut names = HashMap::new();
        for stmt in stmts {
            if let syn::Stmt::Item(item) = stmt {
                collect_item(&mut names, item);
            }
        }
        self.push_if_any(names)
    }

    /// Enters a generic parameter list: each type parameter hides an outside
    /// name. Const generics are values and lifetimes are not paths, so
    /// neither is recorded. No frame is made without a type parameter.
    pub(crate) fn enter_generics(&mut self, generics: &syn::Generics) -> Mark {
        let names = generics
            .params
            .iter()
            .filter_map(|param| match param {
                syn::GenericParam::Type(t) => Some((t.ident.to_string(), Binding::Local)),
                _ => None,
            })
            .collect();
        self.push_if_any(names)
    }

    /// Enters an `impl`: binds `Self` to the self type.
    ///
    /// A path self type whose head is imported gives `Self` that import
    /// (`impl Tr for Uuid` with `use uuid::Uuid;` makes `Self` `uuid::Uuid`);
    /// one whose head is not bound gives `Self` the path as written
    /// (`impl Tr for uuid::Uuid`). A local head, or a self type that is not a
    /// plain path, makes `Self` local. Call it inside [`Self::enter_generics`]
    /// so that a type parameter as the self type is seen as local.
    pub(crate) fn enter_self(&mut self, self_ty: &syn::Type) -> Mark {
        let binding = self.self_binding(self_ty);
        self.push(Frame {
            names: HashMap::from([("Self".to_string(), binding)]),
            module: false,
        })
    }

    /// Enters a type item other than an `impl` (struct, enum, union, trait):
    /// `Self` inside it is that item, so it hides the `Self` of an enclosing
    /// `impl` (an item in a method body of `impl Tr for Uuid`).
    pub(crate) fn hide_self(&mut self) -> Mark {
        self.push(Frame {
            names: HashMap::from([("Self".to_string(), Binding::Local)]),
            module: false,
        })
    }

    /// Leaves the frames entered since `mark` was taken.
    pub(crate) fn exit(&mut self, mark: Mark) {
        self.frames.truncate(mark.0);
    }

    /// Resolves the head of `path`.
    ///
    /// A path of one segment, one with a leading `::`, and one that starts
    /// with `crate`, `self` or `super` are [`Resolution::Unbound`]. Any other
    /// head is looked up from the innermost frame outwards, stopping after
    /// the first module frame. `Self` is looked up like any other name.
    pub(crate) fn resolve(&self, path: &syn::Path) -> Resolution {
        if path.segments.len() < 2 || path.leading_colon.is_some() {
            return Resolution::Unbound;
        }
        let head = path.segments[0].ident.to_string();
        if matches!(head.as_str(), "crate" | "self" | "super") {
            return Resolution::Unbound;
        }
        match self.lookup(&head) {
            Some(Binding::Import { target, decided }) => Resolution::Imported {
                decided: decided.clone(),
                full: join(target, path.segments.iter().skip(1)),
            },
            Some(Binding::Local) => Resolution::Local,
            None => Resolution::Unbound,
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        for frame in self.frames.iter().rev() {
            if let Some(binding) = frame.names.get(name) {
                return Some(binding);
            }
            if frame.module {
                break;
            }
        }
        None
    }

    fn self_binding(&self, self_ty: &syn::Type) -> Binding {
        let syn::Type::Path(type_path) = self_ty else {
            return Binding::Local;
        };
        let path = &type_path.path;
        let Some(head) = path.segments.first() else {
            return Binding::Local;
        };
        if type_path.qself.is_some() {
            return Binding::Local;
        }
        let head = head.ident.to_string();
        let rest = path.segments.iter().skip(1);
        let target = if path.leading_colon.is_some() {
            join(&head, rest)
        } else {
            match self.lookup(&head) {
                Some(Binding::Import { target, .. }) => join(target, rest),
                Some(Binding::Local) => return Binding::Local,
                None => join(&head, rest),
            }
        };
        // The self type was checked as a path where the `impl` names it.
        Binding::Import {
            decided: Some(target.clone()),
            target,
        }
    }

    fn push(&mut self, frame: Frame) -> Mark {
        let mark = Mark(self.frames.len());
        self.frames.push(frame);
        mark
    }

    fn push_if_any(&mut self, names: HashMap<String, Binding>) -> Mark {
        if names.is_empty() {
            return Mark(self.frames.len());
        }
        self.push(Frame {
            names,
            module: false,
        })
    }
}

/// Records the type-namespace names an item declares.
fn collect_item(names: &mut HashMap<String, Binding>, item: &syn::Item) {
    match item {
        syn::Item::Use(u) => {
            for resolved in expand_use_tree(&u.tree, "") {
                if let Some(binding) = resolved.binding {
                    // The first import of a name stands; a local item wins
                    // over any of them whatever the order.
                    names.entry(binding.name).or_insert(Binding::Import {
                        target: binding.target,
                        decided: Some(resolved.path),
                    });
                }
            }
        }
        syn::Item::ExternCrate(e) => {
            let name = e.rename.as_ref().map_or(&e.ident, |(_, alias)| alias);
            // restrict-use does not check an `extern crate` line, so
            // nothing has decided on the crate yet.
            names.entry(name.to_string()).or_insert(Binding::Import {
                target: e.ident.to_string(),
                decided: None,
            });
        }
        syn::Item::Struct(i) => declare(names, &i.ident),
        syn::Item::Enum(i) => declare(names, &i.ident),
        syn::Item::Union(i) => declare(names, &i.ident),
        syn::Item::Trait(i) => declare(names, &i.ident),
        syn::Item::Type(i) => declare(names, &i.ident),
        syn::Item::Mod(i) => declare(names, &i.ident),
        _ => {}
    }
}

fn declare(names: &mut HashMap<String, Binding>, ident: &syn::Ident) {
    names.insert(ident.to_string(), Binding::Local);
}

/// `base` followed by the identifiers of `rest`, `::` separated.
fn join<'a>(base: &str, rest: impl IntoIterator<Item = &'a syn::PathSegment>) -> String {
    let mut joined = base.to_string();
    for segment in rest {
        joined.push_str("::");
        joined.push_str(&segment.ident.to_string());
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(code: &str) -> Vec<syn::Item> {
        syn::parse_file(code).expect("test code should parse").items
    }

    fn stmts(code: &str) -> Vec<syn::Stmt> {
        let block: syn::Block = syn::parse_str(&format!("{{ {code} }}")).expect("block");
        block.stmts
    }

    fn path(code: &str) -> syn::Path {
        syn::parse_str(code).expect("test path should parse")
    }

    fn generics(code: &str) -> syn::Generics {
        let item: syn::ItemFn = syn::parse_str(&format!("fn f{code}() {{}}")).expect("fn");
        item.sig.generics
    }

    fn ty(code: &str) -> syn::Type {
        syn::parse_str(code).expect("test type should parse")
    }

    fn module(code: &str) -> UseBindings {
        let mut bindings = UseBindings::default();
        bindings.enter_module(&items(code));
        bindings
    }

    /// An import whose `use` leaf was checked as `decided`.
    fn imported(decided: &str, full: &str) -> Resolution {
        Resolution::Imported {
            decided: Some(decided.to_string()),
            full: full.to_string(),
        }
    }

    // ── what is collected ──

    #[test]
    fn name_import_binds_the_name() {
        let b = module("use uuid::Uuid;");
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn rename_binds_the_alias_not_the_original() {
        let b = module("use uuid::Uuid as U;");
        assert_eq!(
            b.resolve(&path("U::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Unbound);
    }

    #[test]
    fn self_in_a_group_binds_the_parent() {
        let b = module("use chrono::{self, Utc};");
        assert_eq!(
            b.resolve(&path("chrono::Utc")),
            imported("chrono::self", "chrono::Utc")
        );
        assert_eq!(
            b.resolve(&path("Utc::now")),
            imported("chrono::Utc", "chrono::Utc::now")
        );
    }

    #[test]
    fn self_with_rename_binds_the_alias_to_the_parent() {
        let b = module("use chrono::{self as c};");
        assert_eq!(
            b.resolve(&path("c::Utc")),
            imported("chrono::self", "chrono::Utc")
        );
    }

    #[test]
    fn nested_group_binds_each_leaf() {
        let b = module("use a::{b::{C, D as E}, F};");
        assert_eq!(b.resolve(&path("C::f")), imported("a::b::C", "a::b::C::f"));
        assert_eq!(b.resolve(&path("E::f")), imported("a::b::D", "a::b::D::f"));
        assert_eq!(b.resolve(&path("F::f")), imported("a::F", "a::F::f"));
    }

    #[test]
    fn glob_binds_nothing() {
        let b = module("use uuid::*;");
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Unbound);
    }

    #[test]
    fn underscore_rename_binds_nothing() {
        let b = module("use uuid::Uuid as _;");
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Unbound);
        assert!(b.frames[0].names.is_empty());
    }

    #[test]
    fn extern_crate_binds_the_crate_or_its_alias() {
        let b = module("extern crate uuid;\nextern crate chrono as time;");
        let unchecked = |full: &str| Resolution::Imported {
            decided: None,
            full: full.to_string(),
        };
        assert_eq!(b.resolve(&path("uuid::Uuid")), unchecked("uuid::Uuid"));
        assert_eq!(b.resolve(&path("time::Utc")), unchecked("chrono::Utc"));
    }

    #[test]
    fn type_namespace_items_are_local() {
        let b = module("struct S; enum E {} union U { a: u8 } trait T {} type A = u8; mod m {}");
        for name in ["S", "E", "U", "T", "A", "m"] {
            assert_eq!(
                b.resolve(&path(&format!("{name}::f"))),
                Resolution::Local,
                "{name}"
            );
        }
    }

    #[test]
    fn value_namespace_items_neither_bind_nor_hide() {
        let b = module("use uuid::Uuid;\nfn Uuid() {}\nconst Uuid: u8 = 0;\nstatic Uuid2: u8 = 0;");
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
        assert_eq!(b.resolve(&path("Uuid2::f")), Resolution::Unbound);
    }

    #[test]
    fn local_item_wins_over_an_import_whatever_the_order() {
        for code in ["use a::Foo;\nstruct Foo;", "struct Foo;\nuse a::Foo;"] {
            let b = module(code);
            assert_eq!(b.resolve(&path("Foo::f")), Resolution::Local, "{code}");
        }
    }

    #[test]
    fn names_are_collected_before_use_so_order_does_not_matter() {
        let b = module("fn f() {}\nuse uuid::Uuid;");
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    // ── what resolve refuses ──

    #[test]
    fn unresolvable_heads_are_unbound() {
        let b = module("use uuid::Uuid;\nuse crate::Foo;\nuse super::Bar;");
        for p in [
            "Uuid",
            "::Uuid::new_v7",
            "crate::Uuid::new_v7",
            "self::Uuid::new_v7",
            "super::Uuid::new_v7",
            "other::Uuid::new_v7",
        ] {
            assert_eq!(b.resolve(&path(p)), Resolution::Unbound, "{p}");
        }
    }

    #[test]
    fn crate_self_super_targets_stay_as_written() {
        let b = module("use crate::infra::Db;\nuse super::Helper;");
        assert_eq!(
            b.resolve(&path("Db::new")),
            imported("crate::infra::Db", "crate::infra::Db::new")
        );
        assert_eq!(
            b.resolve(&path("Helper::new")),
            imported("super::Helper", "super::Helper::new")
        );
    }

    #[test]
    fn generic_arguments_are_not_part_of_the_full_path() {
        let b = module("use uuid::Uuid;");
        assert_eq!(
            b.resolve(&path("Uuid::<u8>::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    // ── scopes ──

    #[test]
    fn module_does_not_inherit_the_enclosing_module() {
        let mut b = module("use uuid::Uuid;");
        let mark = b.enter_module(&items("fn f() {}"));
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Unbound);
        b.exit(mark);
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn module_frame_is_a_wall_even_for_a_block_inside_it() {
        let mut b = module("use uuid::Uuid;");
        b.enter_module(&items("use chrono::Utc;"));
        b.enter_block(&stmts("struct S;"));
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Unbound);
        assert_eq!(
            b.resolve(&path("Utc::now")),
            imported("chrono::Utc", "chrono::Utc::now")
        );
    }

    #[test]
    fn block_inherits_and_its_items_hide_outer_imports() {
        let mut b = module("use uuid::Uuid;\nuse chrono::Utc;");
        let mark = b.enter_block(&stmts("struct Uuid;"));
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Local);
        assert_eq!(
            b.resolve(&path("Utc::now")),
            imported("chrono::Utc", "chrono::Utc::now")
        );
        b.exit(mark);
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn block_collects_use_items() {
        let mut b = UseBindings::default();
        b.enter_module(&items(""));
        b.enter_block(&stmts("use uuid::Uuid; let _ = 1;"));
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn block_fn_and_const_do_not_hide() {
        let mut b = module("use uuid::Uuid;");
        b.enter_block(&stmts("const Uuid: u8 = 0; fn Uuid() {}"));
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn block_and_generics_without_names_make_no_frame() {
        let mut b = module("");
        let before = b.frames.len();
        let m1 = b.enter_block(&stmts("let x = 1; fn f() {} const C: u8 = 0;"));
        let m2 = b.enter_generics(&generics("<'a, const N: usize>"));
        let m3 = b.enter_generics(&generics(""));
        assert_eq!(b.frames.len(), before);
        b.exit(m3);
        b.exit(m2);
        b.exit(m1);
        assert_eq!(b.frames.len(), before);
    }

    #[test]
    fn type_parameter_hides_an_import() {
        let mut b = module("use uuid::Uuid;");
        let mark = b.enter_generics(&generics("<Uuid: Tr, 'a, const N: usize>"));
        assert_eq!(b.resolve(&path("Uuid::new_v7")), Resolution::Local);
        b.exit(mark);
        assert_eq!(
            b.resolve(&path("Uuid::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn exit_leaves_every_frame_entered_since_the_mark() {
        let mut b = module("");
        let before = b.frames.len();
        let mark = b.enter_generics(&generics("<T>"));
        b.enter_self(&ty("Foo"));
        b.enter_block(&stmts("struct S;"));
        assert_eq!(b.frames.len(), before + 3);
        b.exit(mark);
        assert_eq!(b.frames.len(), before);
    }

    // ── Self ──

    #[test]
    fn self_follows_an_imported_self_type() {
        let mut b = module("use uuid::Uuid;");
        b.enter_self(&ty("Uuid"));
        assert_eq!(
            b.resolve(&path("Self::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn self_follows_a_multi_segment_self_type_with_an_imported_head() {
        let mut b = module("use uuid::fmt;");
        b.enter_self(&ty("fmt::Hyphenated"));
        assert_eq!(
            b.resolve(&path("Self::f")),
            imported("uuid::fmt::Hyphenated", "uuid::fmt::Hyphenated::f")
        );
    }

    #[test]
    fn self_keeps_the_written_path_when_the_head_is_unbound() {
        let mut b = module("");
        b.enter_self(&ty("uuid::Uuid"));
        assert_eq!(
            b.resolve(&path("Self::new_v7")),
            imported("uuid::Uuid", "uuid::Uuid::new_v7")
        );
    }

    #[test]
    fn self_is_local_for_a_local_head_or_a_non_path_type() {
        for self_ty in [
            "Foo",
            "Foo<u8>",
            "&Foo",
            "(u8, u8)",
            "[u8; 2]",
            "<Foo as Tr>::Out",
        ] {
            let mut b = module("struct Foo;");
            b.enter_self(&ty(self_ty));
            assert_eq!(b.resolve(&path("Self::f")), Resolution::Local, "{self_ty}");
        }
    }

    #[test]
    fn self_is_local_for_a_type_parameter() {
        let mut b = module("use uuid::T;");
        b.enter_generics(&generics("<T>"));
        b.enter_self(&ty("T"));
        assert_eq!(b.resolve(&path("Self::f")), Resolution::Local);
    }

    #[test]
    fn hide_self_hides_the_self_of_an_enclosing_impl() {
        let mut b = module("use uuid::Uuid;");
        b.enter_self(&ty("Uuid"));
        let mark = b.hide_self();
        assert_eq!(b.resolve(&path("Self::f")), Resolution::Local);
        b.exit(mark);
        assert_eq!(
            b.resolve(&path("Self::f")),
            imported("uuid::Uuid", "uuid::Uuid::f")
        );
    }

    #[test]
    fn self_ignores_generic_arguments() {
        let mut b = module("use uuid::Uuid;");
        b.enter_self(&ty("Uuid<u8>"));
        assert_eq!(
            b.resolve(&path("Self::f")),
            imported("uuid::Uuid", "uuid::Uuid::f")
        );
    }
}
