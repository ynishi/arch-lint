//! Rendering a [`ScopeGraph`] as text a human (Mermaid) or `dot` (Graphviz
//! DOT) can draw, and finding which of its edges a lint run flagged.
//!
//! ```no_run
//! use arch_lint::graph::{render, violating_edges, GraphFormat};
//! use arch_lint::Session;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let session = Session::discover(".")?;
//! let scope_graph = session.scope_graph()?; // borrows the session
//! let result = session.run()?; // consumes it
//!
//! let violating = violating_edges(&scope_graph, &result);
//! print!("{}", render(&scope_graph, GraphFormat::Mermaid, &violating));
//! # Ok(())
//! # }
//! ```
//!
//! Nodes are every declared `[[scopes]]` entry, isolated ones included
//! ([`ScopeGraph::nodes`]); there is no node for unscoped files, because the
//! collector behind [`arch_lint_core::ScopeGraph`] never records an
//! unscoped reference target in the first place — there is nothing to draw.
//! Edges are [`ScopeGraph::edges`], labelled with their site count. Both
//! renderers are plain string writers with no external dependency, and
//! deterministic: [`ScopeGraph`] iterates nodes and edges in sorted order,
//! so rendering the same graph twice produces byte-identical output (usable
//! as a `cargo test` snapshot).
//!
//! [`violating_edges`] is the mapping from a [`arch_lint_core::LintResult`]
//! to the edges a renderer should style as violating: see its own docs for
//! the exact rule.

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;

use arch_lint_core::{LintResult, Location, ScopeGraph};

/// A validated scope name, re-exported here so callers of [`render`] and
/// [`violating_edges`] don't need to reach into
/// `arch_lint_core::declarative::model` themselves.
pub use arch_lint_core::declarative::model::ScopeName;

/// Rule codes whose violations retroactively mark a [`ScopeGraph`] edge as
/// violating, for [`violating_edges`]: the per-file rules that check a
/// scope-crossing reference against the graph (`ALD003` deny-scope-dep,
/// `ALD004` layering) and the project-wide rules built on the graph itself
/// (`ALD009` acyclic, `ALD010` independent, `ALD011` allow-dep).
const VIOLATING_CODES: [&str; 5] = ["ALD003", "ALD004", "ALD009", "ALD010", "ALD011"];

/// Output format for [`render`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphFormat {
    /// Graphviz DOT text: `digraph scopes { ... }`, accepted by `dot -Tsvg`.
    Dot,
    /// Mermaid flowchart text: `graph LR` plus its body, pasteable into a
    /// Markdown `mermaid` fenced code block (GitHub renders it as-is).
    Mermaid,
}

/// Renders `graph` as `format`, styling every edge in `violating` as a
/// violation.
///
/// `violating` is normally the result of [`violating_edges`], but any set of
/// `(from, to)` pairs works — a caller that only wants the structure, with
/// no styling, passes an empty set.
///
/// Deterministic: see the [module docs](self).
#[must_use]
pub fn render(
    graph: &ScopeGraph,
    format: GraphFormat,
    violating: &BTreeSet<(ScopeName, ScopeName)>,
) -> String {
    match format {
        GraphFormat::Dot => render_dot(graph, violating),
        GraphFormat::Mermaid => render_mermaid(graph, violating),
    }
}

/// The scopes that participate in at least one edge (either side), used by
/// [`render_dot`] to decide which nodes need an explicit declaration line
/// (an isolated node has no edge to be drawn by). Mermaid declares every
/// node unconditionally (see [`render_mermaid`]), so it has no use for this.
fn touched_nodes(graph: &ScopeGraph) -> HashSet<&ScopeName> {
    graph.edges().flat_map(|(from, to, _)| [from, to]).collect()
}

/// Maps a scope name to a Mermaid node identifier.
///
/// Every id is prefixed `s_` ("scope"), unconditionally. `[[scopes]]` names
/// are `[a-z0-9-]` ([`ScopeName::new`]), so without a prefix a name that
/// happens to equal a Mermaid flowchart keyword (`end`, `style`, `graph`,
/// `subgraph`, `click`, `class`, ...) would be parsed as that keyword, not
/// as a node — breaking the diagram with no error (a scope literally named
/// `end` rendered as a bare `end -->|1| x` line, which closes the
/// surrounding block instead of drawing an edge). The prefix makes every id
/// a non-keyword unconditionally, so there is no keyword list to maintain.
/// `-` is also mapped to `_`: a bare `-` right before an edge arrow (`-->`)
/// is ambiguous with the arrow itself. Because the id never equals the
/// name once prefixed, [`render_mermaid`] always declares a label for it.
fn mermaid_id(name: &ScopeName) -> String {
    format!("s_{}", name.as_str().replace('-', "_"))
}

/// Renders `graph` as Mermaid flowchart text.
///
/// Every node gets its own declaration line (`id["name"]`) up front, in
/// name order, whether or not it has an edge — an isolated node has nothing
/// else to introduce it, and a participating node still needs the label
/// mapping the prefixed id back to the real name (see [`mermaid_id`]).
fn render_mermaid(graph: &ScopeGraph, violating: &BTreeSet<(ScopeName, ScopeName)>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "graph LR");

    for node in graph.nodes() {
        let _ = writeln!(out, "    {}[\"{node}\"]", mermaid_id(node));
    }

    let mut violating_indices = Vec::new();
    for (index, (from, to, sites)) in graph.edges().enumerate() {
        let _ = writeln!(
            out,
            "    {} -->|{}| {}",
            mermaid_id(from),
            sites.len(),
            mermaid_id(to)
        );
        if violating.contains(&(from.clone(), to.clone())) {
            violating_indices.push(index);
        }
    }
    for index in violating_indices {
        let _ = writeln!(out, "    linkStyle {index} stroke:#d33,stroke-width:2px");
    }

    out
}

/// Renders `graph` as Graphviz DOT text.
fn render_dot(graph: &ScopeGraph, violating: &BTreeSet<(ScopeName, ScopeName)>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "digraph scopes {{");
    let _ = writeln!(out, "    rankdir=LR;");

    let touched = touched_nodes(graph);
    for node in graph.nodes() {
        if !touched.contains(node) {
            let _ = writeln!(out, "    \"{node}\";");
        }
    }

    for (from, to, sites) in graph.edges() {
        if violating.contains(&(from.clone(), to.clone())) {
            let _ = writeln!(
                out,
                "    \"{from}\" -> \"{to}\" [label=\"{}\", color=\"red\", penwidth=2];",
                sites.len()
            );
        } else {
            let _ = writeln!(
                out,
                "    \"{from}\" -> \"{to}\" [label=\"{}\"];",
                sites.len()
            );
        }
    }

    let _ = writeln!(out, "}}");
    out
}

/// The `(file, line, column)` of a `Location`, the subset [`violating_edges`]
/// compares.
///
/// `offset`/`length` are deliberately excluded: the graph collector builds
/// an [`arch_lint_core::EdgeSite`]'s location with
/// [`arch_lint_core::Location::from_span`], and the per-file rules that
/// compare against it (`ALD003`, `ALD004`) build their violation's location
/// with [`arch_lint_core::Location::new`] — both leave `offset`/`length` at
/// `0`, so a rule that later starts filling them in on its own violation
/// (via [`arch_lint_core::Location::with_span`]) must not silently stop
/// matching that site — only `file`/`line`/`column` identify "this is the
/// same place in the source" here.
fn location_key(location: &Location) -> (&std::path::Path, usize, usize) {
    (location.file.as_path(), location.line, location.column)
}

/// The edges of `graph` that a violation in `result` points at.
///
/// A violation counts when its `code` is `ALD003`, `ALD004`, `ALD009`,
/// `ALD010` or `ALD011`, and either its primary `location` or one of its
/// `labels`' locations names the same `file`/`line`/`column` as one of the
/// edge's `EdgeSite` locations
/// (`arch_lint_core::Violation`, `arch_lint_core::Label`,
/// `arch_lint_core::EdgeSite`). That match is exact, not a heuristic: the
/// per-file rules (`ALD003`, `ALD004`) build their violation's location from
/// the same source span the graph collector built the site's location from,
/// and the project-wide rules (`ALD009`, `ALD010`, `ALD011`) clone a site's
/// location directly into the violation they report.
///
/// Run [`arch_lint::Session::scope_graph`](crate::Session::scope_graph) and
/// [`arch_lint::Session::run`](crate::Session::run) on the same session
/// configuration before calling this, so `graph` and `result` describe the
/// same analysis.
#[must_use]
pub fn violating_edges(
    graph: &ScopeGraph,
    result: &LintResult,
) -> BTreeSet<(ScopeName, ScopeName)> {
    let locations: HashSet<(&std::path::Path, usize, usize)> = result
        .violations
        .iter()
        .filter(|violation| VIOLATING_CODES.contains(&violation.code.as_str()))
        .flat_map(|violation| {
            std::iter::once(&violation.location)
                .chain(violation.labels.iter().map(|label| &label.location))
        })
        .map(location_key)
        .collect();

    graph
        .edges()
        .filter(|(_, _, sites)| {
            sites
                .iter()
                .any(|site| locations.contains(&location_key(&site.location)))
        })
        .map(|(from, to, _)| (from.clone(), to.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arch_lint_core::{EdgeKind, EdgeSite, Label, Severity, Violation};
    use std::path::PathBuf;

    fn name(s: &str) -> ScopeName {
        ScopeName::new(s).expect("valid scope name")
    }

    fn site(line: usize) -> EdgeSite {
        EdgeSite::new(
            Location::new(PathBuf::from("src/domain/service.rs"), line, 1),
            EdgeKind::Use,
            format!("crate::infra::item{line}"),
        )
    }

    /// 3 nodes (`api-gateway` isolated, `domain`, `infra`), 2 edges, one
    /// violating (`domain -> infra`).
    fn build_graph() -> ScopeGraph {
        let mut g = ScopeGraph::with_nodes([name("api-gateway"), name("domain"), name("infra")]);
        g.add_edge(name("domain"), name("infra"), site(1));
        g.add_edge(name("domain"), name("infra"), site(2));
        g.add_edge(name("infra"), name("domain"), site(3));
        g
    }

    fn violating_domain_to_infra() -> BTreeSet<(ScopeName, ScopeName)> {
        [(name("domain"), name("infra"))].into_iter().collect()
    }

    // ── render: Mermaid ──

    #[test]
    fn mermaid_declares_every_node_with_a_prefixed_id_and_marks_the_violating_edge() {
        let graph = build_graph();
        let rendered = render(&graph, GraphFormat::Mermaid, &violating_domain_to_infra());
        let expected = [
            "graph LR",
            "    s_api_gateway[\"api-gateway\"]",
            "    s_domain[\"domain\"]",
            "    s_infra[\"infra\"]",
            "    s_domain -->|2| s_infra",
            "    s_infra -->|1| s_domain",
            "    linkStyle 0 stroke:#d33,stroke-width:2px",
            "",
        ]
        .join("\n");
        assert_eq!(rendered, expected);
    }

    #[test]
    fn mermaid_maps_a_hyphen_to_an_underscore_in_the_id_but_not_the_label() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("app-core"), name("infra"), site(1));
        let rendered = render(&g, GraphFormat::Mermaid, &BTreeSet::new());
        let expected = [
            "graph LR",
            "    s_app_core[\"app-core\"]",
            "    s_infra[\"infra\"]",
            "    s_app_core -->|1| s_infra",
            "",
        ]
        .join("\n");
        assert_eq!(rendered, expected);
    }

    /// `end` and `style` are Mermaid flowchart keywords; without the `s_`
    /// prefix an unquoted `end -->|1| s_style` line would close the
    /// surrounding block instead of drawing an edge (reproduced in review:
    /// a project with a scope literally named `end` broke the diagram with
    /// no error). The prefix makes every id a non-keyword unconditionally.
    #[test]
    fn mermaid_prefixes_ids_that_collide_with_mermaid_keywords() {
        let mut g = ScopeGraph::new();
        g.add_edge(name("end"), name("style"), site(1));
        let rendered = render(&g, GraphFormat::Mermaid, &BTreeSet::new());
        let expected = [
            "graph LR",
            "    s_end[\"end\"]",
            "    s_style[\"style\"]",
            "    s_end -->|1| s_style",
            "",
        ]
        .join("\n");
        assert_eq!(rendered, expected);
    }

    #[test]
    fn mermaid_empty_graph_is_just_the_header() {
        let graph = ScopeGraph::new();
        assert_eq!(
            render(&graph, GraphFormat::Mermaid, &BTreeSet::new()),
            "graph LR\n"
        );
    }

    // ── render: DOT ──

    #[test]
    fn dot_renders_isolated_node_and_violating_style() {
        let graph = build_graph();
        let rendered = render(&graph, GraphFormat::Dot, &violating_domain_to_infra());
        let expected = [
            "digraph scopes {",
            "    rankdir=LR;",
            "    \"api-gateway\";",
            "    \"domain\" -> \"infra\" [label=\"2\", color=\"red\", penwidth=2];",
            "    \"infra\" -> \"domain\" [label=\"1\"];",
            "}",
            "",
        ]
        .join("\n");
        assert_eq!(rendered, expected);
    }

    #[test]
    fn dot_empty_graph_has_no_node_or_edge_lines() {
        let graph = ScopeGraph::new();
        assert_eq!(
            render(&graph, GraphFormat::Dot, &BTreeSet::new()),
            "digraph scopes {\n    rankdir=LR;\n}\n"
        );
    }

    // ── violating_edges ──

    #[test]
    fn violating_edges_matches_primary_location() {
        let mut graph = ScopeGraph::new();
        let s1 = site(1);
        graph.add_edge(name("domain"), name("infra"), s1.clone());
        graph.add_edge(name("infra"), name("domain"), site(3));

        let mut result = LintResult::new();
        result.violations.push(Violation::new(
            "ALD003",
            "deny-scope-dep",
            Severity::Error,
            s1.location.clone(),
            "test",
        ));

        let violating = violating_edges(&graph, &result);
        assert!(violating.contains(&(name("domain"), name("infra"))));
        assert!(!violating.contains(&(name("infra"), name("domain"))));
    }

    /// The primary location names a file with no edge at all (`src/other
    /// /elsewhere.rs`, which no `site()` ever builds) — only the label,
    /// pointing at the edge's own site, can make this match. A primary
    /// location that happened to sit on the same edge would pass even if
    /// the label path were deleted; this one does not.
    #[test]
    fn violating_edges_matches_a_label_location_too() {
        let mut graph = ScopeGraph::new();
        let s1 = site(1);
        graph.add_edge(name("domain"), name("infra"), s1.clone());

        let primary_on_no_edge = Location::new(PathBuf::from("src/other/elsewhere.rs"), 99, 1);

        let mut result = LintResult::new();
        result.violations.push(
            Violation::new(
                "ALD009",
                "acyclic",
                Severity::Error,
                primary_on_no_edge,
                "test",
            )
            .with_label(Label::new(s1.location.clone(), "other edge")),
        );

        let violating = violating_edges(&graph, &result);
        assert!(violating.contains(&(name("domain"), name("infra"))));
    }

    /// `offset`/`length` (set here via `with_span`, as a future rule might)
    /// must not stop the match: only `file`/`line`/`column` are compared.
    #[test]
    fn violating_edges_matches_despite_a_nonzero_offset() {
        let mut graph = ScopeGraph::new();
        let s1 = site(1);
        graph.add_edge(name("domain"), name("infra"), s1.clone());

        let mut result = LintResult::new();
        result.violations.push(Violation::new(
            "ALD003",
            "deny-scope-dep",
            Severity::Error,
            s1.location.clone().with_span(42, 5),
            "test",
        ));

        let violating = violating_edges(&graph, &result);
        assert!(violating.contains(&(name("domain"), name("infra"))));
    }

    #[test]
    fn violating_edges_ignores_codes_outside_the_graph_rules() {
        let mut graph = ScopeGraph::new();
        let s1 = site(1);
        graph.add_edge(name("domain"), name("infra"), s1.clone());

        let mut result = LintResult::new();
        result.violations.push(Violation::new(
            "AL001",
            "no-unwrap-expect",
            Severity::Error,
            s1.location.clone(),
            "test",
        ));

        let violating = violating_edges(&graph, &result);
        assert!(violating.is_empty());
    }
}
