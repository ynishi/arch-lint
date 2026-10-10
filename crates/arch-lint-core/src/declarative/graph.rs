//! The scope dependency graph collected during analysis.
//!
//! [`ScopeGraph`] is the data model behind the project-wide declarative rules
//! (cycle detection, independence, allow-lists) and the graph export. Nodes
//! are the names declared in `[[scopes]]`; an edge `from → to` exists when a
//! file of scope `from` references a path that resolves into scope `to`. Each
//! edge keeps every site that produced it, so a rule can point at the `use`
//! line and a renderer can count references.
//!
//! The graph is built once per [`Analyzer::analyze`](crate::Analyzer::analyze)
//! by a collector that runs inside the existing per-file pass (no second
//! parse), and is handed to project rules through
//! [`ProjectContext::scope_graph`](crate::ProjectContext::scope_graph).
//! [`Analyzer::scope_graph`](crate::Analyzer::scope_graph) builds it without
//! running any rule.
//!
//! Resolution follows the per-file scope rules (`[[deny-scope-dep]]`,
//! `[[layering]]`): `crate::`, `self::` and `super::` paths are resolved
//! (the latter two relative to the file's module, inline `mod` blocks
//! included); bare paths and `pub use` re-exports are not. A file that
//! belongs to no scope contributes nothing, and a reference that stays
//! inside one scope is not an edge. Every declared scope is a node even when
//! nothing points at or from it.

use std::collections::{BTreeMap, BTreeSet};

use super::model::ScopeName;
use crate::types::Location;

/// How a reference crossed a scope boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EdgeKind {
    /// A `use` item: `use crate::infra::db::Pool;`.
    Use,
    /// An inline qualified path outside a `use`: `crate::infra::db::connect()`.
    Inline,
}

/// One place in the source that produced an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EdgeSite {
    /// Where the reference is: file relative to the root, and a position
    /// within it. `Use` sites: the leaf identifier; `Inline` sites: the
    /// start of the path (both the positions `ALD003` reports).
    pub location: Location,
    /// Whether it was a `use` item or an inline path.
    pub kind: EdgeKind,
    /// The referenced path as written, e.g. `crate::infra::db::Pool`.
    pub path: String,
}

/// Dependency graph between declared scopes.
///
/// Iteration order is deterministic: nodes and edges come back sorted by
/// scope name, and the sites of an edge in the order they were recorded
/// (files are analyzed in sorted order, so that is file order, then source
/// order).
///
/// Limitation (v1): `crate::`, `self::` and `super::` paths are resolved to
/// a scope; bare paths and `pub use` re-export chains are not followed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeGraph {
    nodes: BTreeSet<ScopeName>,
    edges: BTreeMap<(ScopeName, ScopeName), Vec<EdgeSite>>,
}

impl ScopeGraph {
    /// Creates an empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a graph whose nodes are `scopes`, with no edges yet.
    pub fn with_nodes<I>(scopes: I) -> Self
    where
        I: IntoIterator<Item = ScopeName>,
    {
        Self {
            nodes: scopes.into_iter().collect(),
            edges: BTreeMap::new(),
        }
    }

    /// Adds a node. Adding an existing node is a no-op.
    pub fn add_node(&mut self, scope: ScopeName) {
        self.nodes.insert(scope);
    }

    /// Records one reference from `from` to `to`.
    ///
    /// Both endpoints become nodes. A reference inside one scope
    /// (`from == to`) is ignored: the graph describes dependencies between
    /// scopes, not within them.
    pub fn add_edge(&mut self, from: ScopeName, to: ScopeName, site: EdgeSite) {
        if from == to {
            return;
        }
        self.nodes.insert(from.clone());
        self.nodes.insert(to.clone());
        self.edges.entry((from, to)).or_default().push(site);
    }

    /// Returns `true` when the graph has no nodes and no edges.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// Number of nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of distinct edges (not sites).
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Nodes in name order.
    pub fn nodes(&self) -> impl Iterator<Item = &ScopeName> + '_ {
        self.nodes.iter()
    }

    /// Edges as `(from, to, sites)`, ordered by `from` then `to`.
    pub fn edges(&self) -> impl Iterator<Item = (&ScopeName, &ScopeName, &[EdgeSite])> + '_ {
        self.edges
            .iter()
            .map(|((from, to), sites)| (from, to, sites.as_slice()))
    }

    /// Returns `true` when at least one reference goes from `from` to `to`.
    #[must_use]
    pub fn has_edge(&self, from: &ScopeName, to: &ScopeName) -> bool {
        self.edges.contains_key(&(from.clone(), to.clone()))
    }

    /// The sites that produced the edge `from → to` (empty when there is none).
    #[must_use]
    pub fn sites(&self, from: &ScopeName, to: &ScopeName) -> &[EdgeSite] {
        self.edges
            .get(&(from.clone(), to.clone()))
            .map_or(&[], Vec::as_slice)
    }

    /// Scopes that `from` depends on, in name order.
    ///
    /// `from` is cloned into the filter closure, so the returned iterator
    /// captures only `self`'s lifetime (`use<'a>` makes that precise) and a
    /// temporary (`g.successors(&name("a"))`) borrows nothing past this call.
    pub fn successors<'a>(
        &'a self,
        from: &ScopeName,
    ) -> impl Iterator<Item = &'a ScopeName> + use<'a> {
        let from = from.clone();
        self.edges
            .keys()
            .filter(move |(f, _)| *f == from)
            .map(|(_, to)| to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn name(s: &str) -> ScopeName {
        ScopeName::new(s).expect("valid scope name")
    }

    fn site(line: usize, kind: EdgeKind) -> EdgeSite {
        EdgeSite {
            location: Location::new(PathBuf::from("src/a/x.rs"), line, 1),
            kind,
            path: format!("crate::b::item{line}"),
        }
    }

    #[test]
    fn add_edge_aggregates_sites_per_pair() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("a"), name("b"), site(1, EdgeKind::Use));
        g.add_edge(name("a"), name("b"), site(7, EdgeKind::Inline));
        g.add_edge(name("b"), name("a"), site(3, EdgeKind::Use));

        assert_eq!(g.node_count(), 2);
        assert_eq!(g.edge_count(), 2);
        assert_eq!(g.sites(&name("a"), &name("b")).len(), 2);
        assert_eq!(g.sites(&name("a"), &name("b"))[1].kind, EdgeKind::Inline);
        assert!(g.has_edge(&name("b"), &name("a")));
        assert!(!g.has_edge(&name("a"), &name("a")));
    }

    #[test]
    fn self_reference_is_not_an_edge() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("a"), name("a"), site(1, EdgeKind::Use));
        assert_eq!(g.edge_count(), 0);
        assert!(g.is_empty(), "a dropped self-edge adds no node either");
    }

    #[test]
    fn declared_nodes_survive_without_edges() {
        let g = ScopeGraph::with_nodes([name("infra"), name("domain"), name("app")]);
        let nodes: Vec<&str> = g.nodes().map(ScopeName::as_str).collect();
        assert_eq!(nodes, ["app", "domain", "infra"], "sorted by name");
        assert!(!g.is_empty());
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn successors_are_only_the_out_edges_of_that_node() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("a"), name("c"), site(1, EdgeKind::Use));
        g.add_edge(name("a"), name("b"), site(2, EdgeKind::Use));
        g.add_edge(name("b"), name("c"), site(3, EdgeKind::Use));
        g.add_edge(name("aa"), name("c"), site(4, EdgeKind::Use));

        let succ: Vec<&str> = g.successors(&name("a")).map(ScopeName::as_str).collect();
        assert_eq!(succ, ["b", "c"]);
        assert_eq!(g.successors(&name("c")).count(), 0);
    }

    #[test]
    fn edges_iterate_in_name_order() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("z"), name("a"), site(1, EdgeKind::Use));
        g.add_edge(name("a"), name("z"), site(2, EdgeKind::Use));
        let pairs: Vec<(&str, &str)> = g
            .edges()
            .map(|(f, t, _)| (f.as_str(), t.as_str()))
            .collect();
        assert_eq!(pairs, [("a", "z"), ("z", "a")]);
    }
}
