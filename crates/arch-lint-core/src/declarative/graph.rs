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
use crate::utils::allowance::AllowDirective;

/// How a reference crossed a scope boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EdgeKind {
    /// A `use` item: `use crate::infra::db::Pool;`.
    Use,
    /// An inline qualified path outside a `use`: `crate::infra::db::connect()`.
    Inline,
}

/// Where an `allow` directive recorded on an [`EdgeSite`] came from.
///
/// The three mechanisms the per-file rules' `Suppressions` also honor, in
/// the same priority order: `Scope` and `Item` are `#[arch_lint::allow(...)]`
/// attributes (no `reason` required), `Comment` is the inline
/// `// arch-lint: allow(...)` directive (a `reason` is required on an
/// error-severity rule — the warning reported in its place mirrors the one
/// `Suppressions::decide` reports for the per-file rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DirectiveSource {
    /// An enclosing `mod` (with a body) / `fn` / `impl` / `trait` attribute.
    Scope,
    /// The site's own item attribute (a `use` item; inline paths have no
    /// item of their own and so never produce this source).
    Item,
    /// The inline comment on the site's line or the line above it.
    Comment,
}

/// One place in the source that produced an edge.
///
/// Fields other than the allow directives are `pub` for read access, but
/// `#[non_exhaustive]` means a literal can't be built outside this crate;
/// use [`EdgeSite::new`], so a future field addition is not a breaking
/// change for crates outside this one. The allow directives are private:
/// read them through [`EdgeSite::allow_directive`] /
/// [`EdgeSite::allow_directives`].
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
    /// Every `allow` directive in scope at this site, each tagged with
    /// where it came from ([`DirectiveSource`]): the enclosing
    /// `mod`/`fn`/`impl`/`trait` attributes (outermost first), then the
    /// site's own item attributes (for a `use` item), then the inline
    /// comment directive(s) on its line — the same priority order the
    /// per-file rules' `Suppressions` consult the three mechanisms in.
    /// Recorded regardless of which rule (if any) the name belongs to —
    /// project-wide rules built on the graph (currently `[[acyclic]]`,
    /// `ALD009`) decide for themselves which directives apply to them, and
    /// whether their source matters (an attribute source never requires a
    /// `reason`; a comment source does, on an error-severity rule).
    allow_directives: Vec<(DirectiveSource, AllowDirective)>,
}

impl EdgeSite {
    /// Creates a site with no directives recorded.
    #[must_use]
    pub fn new(location: Location, kind: EdgeKind, path: impl Into<String>) -> Self {
        Self {
            location,
            kind,
            path: path.into(),
            allow_directives: Vec::new(),
        }
    }

    /// Records one more `allow` directive in scope at this site, from
    /// `source`.
    ///
    /// `directive`'s rule names are normalized to `snake_case` (`-` → `_`)
    /// before being stored — every caller gets this for free, not only this
    /// crate's own collector, so a lookup only needs to normalize the name
    /// it is searching for.
    ///
    /// Directives are searched in the order they are added, so callers
    /// should add the ones that take priority (enclosing-scope and
    /// item-level attributes) before the ones that don't (inline comments) —
    /// matching the order the per-file rules' `Suppressions` consult the
    /// same three mechanisms in.
    #[must_use]
    pub fn with_allow_directive(
        mut self,
        source: DirectiveSource,
        directive: AllowDirective,
    ) -> Self {
        let normalized = AllowDirective {
            rules: directive
                .rules
                .into_iter()
                .map(|rule| rule.replace('-', "_"))
                .collect(),
            reason: directive.reason,
        };
        self.allow_directives.push((source, normalized));
        self
    }

    /// Every `allow` directive in scope at this site, each with where it
    /// came from, in priority order (see
    /// [`EdgeSite::with_allow_directive`]).
    #[must_use]
    pub fn allow_directives(&self) -> &[(DirectiveSource, AllowDirective)] {
        &self.allow_directives
    }

    /// The first directive (in priority order) naming `rule` or `all`,
    /// together with where it came from.
    ///
    /// `rule` is normalized to `snake_case` before comparison, so either
    /// spelling (`"acyclic"`, a kebab-case TOML `name`, or a code like
    /// `"ALD009"`) matches regardless of which spelling the directive used.
    /// A caller that needs *every* matching directive (not just the first —
    /// for example to tell whether an edge is suppressed solely by
    /// reason-less comments, when some other directive on the same site
    /// also matches) should scan [`EdgeSite::allow_directives`] directly.
    #[must_use]
    pub fn allow_directive(&self, rule: &str) -> Option<(DirectiveSource, &AllowDirective)> {
        let normalized = rule.replace('-', "_");
        self.allow_directives
            .iter()
            .find(|(_, directive)| {
                directive.rules.contains("all") || directive.rules.contains(&normalized)
            })
            .map(|(source, directive)| (*source, directive))
    }
}

/// Dependency graph between declared scopes.
///
/// Iteration order is deterministic: nodes and edges come back sorted by
/// scope name, and the sites of an edge in the order they were recorded
/// (files are analyzed in sorted order, so that is file order, then source
/// order).
///
/// Limitation (v1): `crate::`, `self::` and `super::` paths are resolved to
/// a scope, against the package the referencing file belongs to (the nearest
/// `Cargo.toml` with a `[package]` table; see
/// [`FileContext::crate_root`](crate::FileContext::crate_root)); bare paths,
/// cross-crate paths and `pub use` re-export chains are not followed.
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
        EdgeSite::new(
            Location::new(PathBuf::from("src/a/x.rs"), line, 1),
            kind,
            format!("crate::b::item{line}"),
        )
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

    fn directive(rule: &str) -> crate::utils::allowance::AllowDirective {
        crate::utils::allowance::AllowDirective {
            rules: [rule.to_string()].into_iter().collect(),
            reason: Some("because".to_string()),
        }
    }

    #[test]
    fn edge_site_allow_directive_tracks_named_rules() {
        let plain = site(1, EdgeKind::Use);
        assert!(plain.allow_directive("acyclic").is_none());

        let suppressed = site(1, EdgeKind::Use)
            .with_allow_directive(DirectiveSource::Comment, directive("acyclic"));
        assert!(suppressed.allow_directive("acyclic").is_some());
        assert!(suppressed.allow_directive("ALD009").is_none());
    }

    #[test]
    fn edge_site_allow_directive_normalizes_the_query_side() {
        // The collector normalizes stored rule names to `snake_case`; a
        // caller may still ask using kebab-case (e.g. a TOML `name` or
        // `Acyclic::display_name()`), so the query side is normalized too.
        let stored = site(1, EdgeKind::Use)
            .with_allow_directive(DirectiveSource::Comment, directive("no_cycles"));
        assert!(stored.allow_directive("no-cycles").is_some());
        assert!(stored.allow_directive("no_cycles").is_some());
    }

    #[test]
    fn with_allow_directive_normalizes_the_stored_side_too() {
        // N5: normalization happens inside `with_allow_directive` itself, so
        // it holds for every caller, not only a collector that happens to
        // pre-normalize before calling it.
        let stored = site(1, EdgeKind::Use)
            .with_allow_directive(DirectiveSource::Comment, directive("no-cycles"));
        assert!(stored.allow_directive("no_cycles").is_some());
        assert!(stored.allow_directive("no-cycles").is_some());
    }

    #[test]
    fn edge_site_allow_directive_honors_all() {
        let site =
            site(1, EdgeKind::Use).with_allow_directive(DirectiveSource::Comment, directive("all"));
        assert!(site.allow_directive("acyclic").is_some());
    }

    #[test]
    fn edge_site_allow_directives_returns_every_recorded_directive() {
        let site = site(1, EdgeKind::Use)
            .with_allow_directive(DirectiveSource::Item, directive("acyclic"))
            .with_allow_directive(DirectiveSource::Comment, directive("ALD009"));
        assert_eq!(site.allow_directives().len(), 2);
    }

    #[test]
    fn edge_site_allow_directive_reports_its_source() {
        let site = site(1, EdgeKind::Use)
            .with_allow_directive(DirectiveSource::Scope, directive("acyclic"));
        let (source, _) = site.allow_directive("acyclic").expect("recorded");
        assert_eq!(source, DirectiveSource::Scope);
    }
}
