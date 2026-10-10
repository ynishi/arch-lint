//! Collects scope-crossing references into a [`ScopeGraph`].
//!
//! The collector is not a rule: it reports nothing. It runs on the same AST
//! the per-file rules see and records, for every `use` item and inline path
//! that resolves from the file's scope into another declared scope, one
//! [`EdgeSite`] on the edge between the two scopes.
//!
//! Path resolution is the one [`ScopeDepRule`](super::ScopeDepRule) (`ALD003`)
//! uses: `crate::`, `self::` and `super::` paths (the latter two rewritten
//! relative to the file's module, inline `mod` blocks included, exactly as
//! `ScopeDepVisitor` does), resolved against `src/x/y.rs` and
//! `src/x/y/mod.rs` candidates. Bare paths and `pub use` re-exports are not
//! followed. Unlike `ALD003`, the graph records every reference regardless
//! of `allow` suppressions and of `check_inline` — rules built on the graph
//! decide what to filter.

use syn::spanned::Spanned;
use syn::visit::Visit;

use super::scope_dep::{file_module_segments, is_relative_path, resolve_relative_target_scopes};
use super::{expand_use_tree, inline_path_to_string};
use crate::context::FileContext;
use crate::declarative::graph::{EdgeKind, EdgeSite, ScopeGraph};
use crate::declarative::model::{DeclarativeConfig, ScopeName};
use crate::types::Location;

/// Visitor that records scope-crossing references of one file.
pub(crate) struct ScopeEdgeCollector<'a> {
    ctx: &'a FileContext<'a>,
    config: &'a DeclarativeConfig,
    file_scopes: Vec<&'a ScopeName>,
    graph: &'a mut ScopeGraph,
    /// Module path of the file itself; see
    /// [`ScopeDepVisitor`](super::scope_dep::ScopeDepVisitor)'s field of the
    /// same name (`None` outside `src/`, or under `src/bin/`).
    file_module: Option<Vec<String>>,
    /// Names of the inline `mod` blocks currently being visited.
    mod_stack: Vec<String>,
}

impl<'a> ScopeEdgeCollector<'a> {
    /// Records every scope-crossing reference in `ast` into `graph`.
    ///
    /// A file that belongs to no declared scope contributes nothing.
    pub(crate) fn collect(
        config: &'a DeclarativeConfig,
        ctx: &'a FileContext<'a>,
        ast: &syn::File,
        graph: &'a mut ScopeGraph,
    ) {
        let file_scopes = config.scopes_for_path(&ctx.relative_path);
        if file_scopes.is_empty() {
            return;
        }

        let mut collector = Self {
            file_module: file_module_segments(&ctx.relative_path),
            ctx,
            config,
            file_scopes,
            graph,
            mod_stack: Vec::new(),
        };
        collector.visit_file(ast);
    }

    /// The module path at the visitor's current position: the file's own
    /// module path, plus the inline `mod` blocks currently entered. `None`
    /// when the file itself has no resolvable module path.
    fn current_module(&self) -> Option<Vec<String>> {
        let mut module = self.file_module.clone()?;
        module.extend(self.mod_stack.iter().cloned());
        Some(module)
    }

    fn record(&mut self, path: &str, span: proc_macro2::Span, kind: EdgeKind) {
        // Only `self::`/`super::` paths need the current module; a
        // `crate::`/bare/external path never pays for computing it.
        let current_module = is_relative_path(path)
            .then(|| self.current_module())
            .flatten();
        let targets = resolve_relative_target_scopes(self.config, current_module.as_deref(), path);
        if targets.is_empty() {
            return;
        }

        for from in &self.file_scopes {
            for to in &targets {
                // `add_edge` drops `from == to`; the check here only avoids
                // building a site for it.
                if from == to {
                    continue;
                }
                self.graph.add_edge(
                    (*from).clone(),
                    (*to).clone(),
                    EdgeSite {
                        location: Location::from_span(self.ctx.relative_path.clone(), span),
                        kind,
                        path: path.to_string(),
                    },
                );
            }
        }
    }
}

impl<'ast> Visit<'ast> for ScopeEdgeCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let pushed_mod = node.content.is_some();
        if pushed_mod {
            self.mod_stack.push(node.ident.to_string());
        }
        syn::visit::visit_item_mod(self, node);
        if pushed_mod {
            self.mod_stack.pop();
        }
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        for use_item in expand_use_tree(&node.tree, "") {
            self.record(&use_item.path, use_item.span, EdgeKind::Use);
        }
        syn::visit::visit_item_use(self, node);
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        // `use` items hold `UseTree`s, not `syn::Path`, so a `use` is never
        // recorded twice here.
        if let Some(path) = inline_path_to_string(node) {
            self.record(&path, node.span(), EdgeKind::Inline);
        }
        syn::visit::visit_path(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{make_ctx, parse_file};
    use super::*;
    use crate::declarative::model::{GlobPattern, Scope};

    fn name(s: &str) -> ScopeName {
        ScopeName::new(s).expect("valid scope name")
    }

    fn config() -> DeclarativeConfig {
        let scopes = vec![
            Scope::new(
                name("domain"),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            ),
            Scope::new(
                name("infra"),
                vec![GlobPattern::new("src/infra/**").unwrap()],
            ),
            Scope::new(name("app"), vec![GlobPattern::new("src/app/**").unwrap()]),
        ];
        DeclarativeConfig::new(scopes, vec![], vec![], vec![]).expect("valid config")
    }

    fn collect(path: &str, code: &str) -> ScopeGraph {
        let config = config();
        let ctx = make_ctx(path, code);
        let ast = parse_file(code);
        let mut graph = ScopeGraph::new();
        ScopeEdgeCollector::collect(&config, &ctx, &ast, &mut graph);
        graph
    }

    #[test]
    fn use_item_records_an_edge_at_the_leaf_identifier() {
        let code = "use crate::infra::db::Pool;\n\npub fn f() {}\n";
        let graph = collect("src/domain/service.rs", code);

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, EdgeKind::Use);
        assert_eq!(sites[0].path, "crate::infra::db::Pool");
        assert_eq!(sites[0].location.line, 1);
        assert_eq!(sites[0].location.column, 23, "column of `Pool`");
        assert_eq!(
            sites[0].location.file,
            std::path::PathBuf::from("src/domain/service.rs")
        );
    }

    #[test]
    fn grouped_use_records_one_site_per_leaf() {
        let code = "use crate::infra::{db::Pool, cache::Redis};\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.sites(&name("domain"), &name("infra")).len(), 2);
    }

    #[test]
    fn inline_path_records_an_inline_edge() {
        let code = "pub fn f() { crate::infra::db::connect(); }\n";
        let graph = collect("src/domain/service.rs", code);

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, EdgeKind::Inline);
        assert_eq!(sites[0].path, "crate::infra::db::connect");
    }

    #[test]
    fn same_scope_reference_is_not_an_edge() {
        let code = "use crate::domain::entity::User;\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn external_crate_and_bare_paths_are_not_edges() {
        // `tokio::sync::Mutex` is an external crate; `bare::path` has no
        // `crate::`/`self::`/`super::` prefix (and isn't `pub use` chased).
        // Neither is resolved to a scope at all.
        let code = "use tokio::sync::Mutex;\nuse bare::path;\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn single_super_from_domain_service_stays_in_domain_scope() {
        // `self::x::Y` and one `super::infra::db::Pool` from
        // `crate::domain::service` both resolve into `domain` (`self::` to
        // `crate::domain::service::x::Y`; one `super::` to
        // `crate::domain::infra::db::Pool` — still under `src/domain/**`).
        // They are resolved, but a same-scope reference is not an edge.
        let code = "use super::infra::db::Pool;\nuse self::x::Y;\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.edge_count(), 0);
    }

    /// Acceptance 3 (first case): `use super::super::infra::db::Pool;` in
    /// `src/domain/service.rs` is the same reference as acceptance 1 and
    /// produces the matching `domain -> infra` `Use` edge.
    #[test]
    fn double_super_use_item_records_domain_to_infra_edge() {
        let code = "use super::super::infra::db::Pool;\n";
        let graph = collect("src/domain/service.rs", code);

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, EdgeKind::Use);
        assert_eq!(sites[0].path, "super::super::infra::db::Pool");
    }

    /// Acceptance 3 (second case): `use self::helpers::f;` in
    /// `src/domain/mod.rs` resolves to `domain` (same as the file's own
    /// scope), so it produces no edge.
    #[test]
    fn self_use_item_in_domain_mod_rs_produces_no_edge() {
        let code = "use self::helpers::f;\n";
        let graph = collect("src/domain/mod.rs", code);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn inline_double_super_path_records_an_inline_edge() {
        let code = "pub fn f() { super::super::infra::db::connect(); }\n";
        let graph = collect("src/domain/service.rs", code);

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, EdgeKind::Inline);
        assert_eq!(sites[0].path, "super::super::infra::db::connect");
    }

    #[test]
    fn super_climbs_relative_to_inline_mod() {
        // Mirrors `scope_dep_super_climbs_relative_to_inline_mod`: three
        // `super`s are needed to reach the crate root from inside the
        // inline `wiring` module.
        let code = "mod wiring {\n    use super::super::super::infra::db::Pool;\n}\n";
        let graph = collect("src/domain/service.rs", code);

        assert_eq!(graph.sites(&name("domain"), &name("infra")).len(), 1);
    }

    /// A bodyless `mod helpers;` must not push onto the mod stack; a
    /// following sibling `use` at file level still resolves against the
    /// file's own module (mirrors `scope_dep`'s test of the same name).
    #[test]
    fn bodyless_mod_declaration_does_not_affect_following_use_edge() {
        let code = "mod helpers;\nuse super::super::infra::db::Pool;\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.sites(&name("domain"), &name("infra")).len(), 1);
    }

    /// A `super::` that would climb above the crate root produces no edge.
    #[test]
    fn super_above_crate_root_produces_no_edge() {
        let config = DeclarativeConfig::new(
            vec![
                Scope::new(name("root"), vec![GlobPattern::new("src/*.rs").unwrap()]),
                Scope::new(
                    name("infra"),
                    vec![GlobPattern::new("src/infra/**").unwrap()],
                ),
            ],
            vec![],
            vec![],
            vec![],
        )
        .expect("valid config");
        let code = "use super::infra::db::Pool;\n";
        let ctx = make_ctx("src/lib.rs", code);
        let ast = parse_file(code);
        let mut graph = ScopeGraph::new();
        ScopeEdgeCollector::collect(&config, &ctx, &ast, &mut graph);

        assert!(graph.is_empty(), "{graph:#?}");
    }

    #[test]
    fn file_outside_every_scope_contributes_nothing() {
        let code = "use crate::infra::db::Pool;\n";
        let graph = collect("src/other/thing.rs", code);
        assert!(graph.is_empty());
    }

    #[test]
    fn reverse_direction_is_a_distinct_edge() {
        let mut graph = collect("src/domain/service.rs", "use crate::infra::db::Pool;\n");
        let config = config();
        let code = "use crate::domain::entity::User;\n";
        let ctx = make_ctx("src/infra/db.rs", code);
        ScopeEdgeCollector::collect(&config, &ctx, &parse_file(code), &mut graph);

        assert!(graph.has_edge(&name("domain"), &name("infra")));
        assert!(graph.has_edge(&name("infra"), &name("domain")));
        assert_eq!(graph.edge_count(), 2);
    }
}
