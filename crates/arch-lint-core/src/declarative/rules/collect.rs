//! Collects scope-crossing references into a [`ScopeGraph`].
//!
//! The collector is not a rule: it reports nothing. It runs on the same AST
//! the per-file rules see and records, for every `use` item and inline path
//! that resolves from the file's scope into another declared scope, one
//! [`EdgeSite`] on the edge between the two scopes.
//!
//! Path resolution is the one [`ScopeDepRule`](super::ScopeDepRule) (`ALD003`)
//! uses: `crate::`-prefixed paths only, `src/x/y.rs` and `src/x/y/mod.rs`
//! candidates. Unlike `ALD003`, the graph records every reference regardless
//! of `allow` suppressions and of `check_inline` — rules built on the graph
//! decide what to filter.

use syn::spanned::Spanned;
use syn::visit::Visit;

use super::scope_dep::resolve_target_scopes;
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
            ctx,
            config,
            file_scopes,
            graph,
        };
        collector.visit_file(ast);
    }

    fn record(&mut self, path: &str, span: proc_macro2::Span, kind: EdgeKind) {
        let targets = resolve_target_scopes(self.config, path);
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
    fn external_crate_and_relative_paths_are_not_edges() {
        let code = "use tokio::sync::Mutex;\nuse super::infra::db::Pool;\nuse self::x::Y;\n";
        let graph = collect("src/domain/service.rs", code);
        assert_eq!(graph.edge_count(), 0);
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
