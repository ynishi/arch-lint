//! `[[acyclic]]` (ALD009): a project-wide check that the scope graph has no
//! cycle.
//!
//! Unlike every other declarative rule, this one is a
//! [`ProjectRule`](crate::rule::ProjectRule): it does not look at one file's
//! AST, it looks at the [`ScopeGraph`] the analyzer collected while running
//! the per-file rules ([`ProjectContext::scope_graph`]).
//!
//! For each `[[acyclic]]` block: build the sub-graph induced by its `scopes`
//! (every declared scope when omitted), drop every edge whose sites are all
//! suppressed by an `allow` directive (see [`EdgeSite::allow_directive`]),
//! then run Tarjan's strongly-connected-components algorithm over what
//! remains. Every component of more than one node is one cycle, and
//! therefore one violation — a self-loop cannot occur, since
//! [`ScopeGraph::add_edge`] drops `from == to`.
//!
//! An `allow` directive suppresses a site, but whether it needs a `reason`
//! depends on where it came from — the same rule `Suppressions::decide`
//! applies to the per-file rules. An attribute (`Scope`, an enclosing
//! `mod`/`fn`/`impl`/`trait`, or `Item`, the site's own `use`) suppresses
//! silently, with or without a `reason`. A `Comment` (the inline
//! `// arch-lint: allow(...)`) needs one when the rule's severity is
//! `Error`: if a site is suppressed *solely* by comment directives and none
//! of them carries a reason, and that site actually sat on a cycle before
//! suppression, the dropped violation is replaced by the same
//! missing-reason warning [`Suppressions::decide`](super::Suppressions::decide)
//! reports for the per-file rules (via
//! [`missing_reason_violation`](super::missing_reason_violation)) — one
//! warning per offending site, not per edge.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::context::ProjectContext;
use crate::declarative::graph::{EdgeSite, ScopeGraph};
use crate::declarative::model::{Acyclic, DeclarativeConfig, ScopeName};
use crate::rule::ProjectRule;
use crate::types::{Label, Severity, Violation};

use super::graph_suppress::{classify_site, SiteOutcome};
use super::missing_reason_violation;

/// Kind name used by `allow` directives and [`ProjectRule::name`].
pub(crate) const ACYCLIC_NAME: &str = "acyclic";
/// Rule code used by `allow` directives and [`ProjectRule::code`].
pub(crate) const ACYCLIC_CODE: &str = "ALD009";

/// A project-wide rule that enforces every `[[acyclic]]` declaration.
pub struct AcyclicRule {
    config: Arc<DeclarativeConfig>,
}

impl AcyclicRule {
    /// Creates a new acyclic rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl ProjectRule for AcyclicRule {
    fn name(&self) -> &'static str {
        ACYCLIC_NAME
    }

    fn code(&self) -> &'static str {
        ACYCLIC_CODE
    }

    fn description(&self) -> &'static str {
        "Detect cycles between scopes"
    }

    fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation> {
        self.config
            .acyclics()
            .iter()
            .flat_map(|rule| check_one(rule, ctx.scope_graph()))
            .collect()
    }
}

/// The nodes `rule` restricts the cycle check to: the rule's own `scopes`
/// list, or every node of `graph` when it names none.
fn universe(rule: &Acyclic, graph: &ScopeGraph) -> BTreeSet<ScopeName> {
    match rule.scopes() {
        Some(scopes) => scopes.iter().cloned().collect(),
        None => graph.nodes().cloned().collect(),
    }
}

/// Adjacency restricted to a rule's `scopes`.
type Adjacency = BTreeMap<ScopeName, BTreeSet<ScopeName>>;

/// The reporting site of each surviving edge, keyed by `(from, to)` — what a
/// violation for that edge points at.
type RepresentativeSites<'g> = HashMap<(ScopeName, ScopeName), &'g EdgeSite>;

/// Adjacency over every edge in `nodes`, `allow` directives ignored
/// entirely. Used only to compute [`cycle_membership`]: whether suppressing
/// a site actually broke a cycle, so the missing-reason warning (B1) does
/// not fire on an `allow` that was never standing between anything and a
/// real violation.
fn raw_adjacency(graph: &ScopeGraph, nodes: &BTreeSet<ScopeName>) -> Adjacency {
    let mut adjacency: Adjacency = nodes
        .iter()
        .cloned()
        .map(|n| (n, BTreeSet::new()))
        .collect();
    for (from, to, _) in graph.edges() {
        if nodes.contains(from) && nodes.contains(to) {
            adjacency
                .entry(from.clone())
                .or_default()
                .insert(to.clone());
        }
    }
    adjacency
}

/// Maps every node that sits in a cycle (a strongly connected component of
/// more than one node) of `adjacency` to that component's id. A node absent
/// from the map is not part of any cycle.
fn cycle_membership(adjacency: &Adjacency) -> HashMap<ScopeName, usize> {
    let mut membership = HashMap::new();
    for (id, scc) in strongly_connected_components(adjacency)
        .into_iter()
        .enumerate()
    {
        if scc.len() > 1 {
            for node in scc {
                membership.insert(node, id);
            }
        }
    }
    membership
}

/// Whether the edge `from -> to` sits on some cycle of the *unsuppressed*
/// graph (both endpoints belong to the same component in `membership`).
fn edge_in_a_cycle(
    membership: &HashMap<ScopeName, usize>,
    from: &ScopeName,
    to: &ScopeName,
) -> bool {
    matches!((membership.get(from), membership.get(to)), (Some(a), Some(b)) if a == b)
}

/// Builds the working adjacency (edges with at least one site that stands —
/// no directive names `rule` there — restricted to `nodes`) and, for each
/// surviving edge, the reporting site. Also collects the missing-reason
/// warnings (B1, S6): every individual site classified as
/// [`SiteOutcome::MissingReason`] gets its own warning (not one per edge),
/// and only when `rule`'s severity is `Error` and the edge sits on a real
/// cycle in the unsuppressed graph (`membership`) — so a reason-less
/// `allow` that was never standing between anything and a real violation
/// does not warn. This is independent of whether the edge ultimately
/// survives through a different site: each site's own suppression attempt
/// is judged on its own, the same way `Suppressions::decide` judges each
/// per-file occurrence on its own.
fn working_graph<'g>(
    rule: &Acyclic,
    graph: &'g ScopeGraph,
    nodes: &BTreeSet<ScopeName>,
    membership: &HashMap<ScopeName, usize>,
) -> (Adjacency, RepresentativeSites<'g>, Vec<Violation>) {
    let mut adjacency: Adjacency = nodes
        .iter()
        .cloned()
        .map(|n| (n, BTreeSet::new()))
        .collect();
    let mut representative: RepresentativeSites<'g> = HashMap::new();
    let mut warnings = Vec::new();

    for (from, to, sites) in graph.edges() {
        if !nodes.contains(from) || !nodes.contains(to) {
            continue;
        }

        let mut reporting_site: Option<&'g EdgeSite> = None;
        for site in sites {
            match classify_site(site, ACYCLIC_NAME, ACYCLIC_CODE, &rule.display_name()) {
                SiteOutcome::Report => {
                    if reporting_site.is_none() {
                        reporting_site = Some(site);
                    }
                }
                SiteOutcome::Suppressed => {}
                SiteOutcome::MissingReason => {
                    if rule.severity() == Severity::Error && edge_in_a_cycle(membership, from, to) {
                        warnings.push(missing_reason_violation(
                            ACYCLIC_CODE,
                            &rule.display_name(),
                            site.location.clone(),
                        ));
                    }
                }
            }
        }

        if let Some(site) = reporting_site {
            adjacency
                .entry(from.clone())
                .or_default()
                .insert(to.clone());
            representative.insert((from.clone(), to.clone()), site);
        }
    }

    (adjacency, representative, warnings)
}

/// Runs one `[[acyclic]]` block against `graph`: one violation per strongly
/// connected component of more than one node, plus any missing-reason
/// warnings (see [`working_graph`]).
fn check_one(rule: &Acyclic, graph: &ScopeGraph) -> Vec<Violation> {
    let nodes = universe(rule, graph);
    let membership = cycle_membership(&raw_adjacency(graph, &nodes));
    let (adjacency, representative, mut violations) =
        working_graph(rule, graph, &nodes, &membership);

    violations.extend(
        strongly_connected_components(&adjacency)
            .into_iter()
            .filter(|scc| scc.len() > 1)
            .filter_map(|scc| build_violation(rule, &adjacency, &representative, &scc)),
    );
    violations
}

/// Builds the violation for one cycle, or `None` if (defensively) no cycle
/// path could be derived — unreachable for a genuine SCC of more than one
/// node, since every member then has an outgoing edge to another member.
fn build_violation(
    rule: &Acyclic,
    adjacency: &Adjacency,
    representative: &RepresentativeSites<'_>,
    scc: &BTreeSet<ScopeName>,
) -> Option<Violation> {
    let path = find_cycle_path(scc, adjacency);
    if path.len() < 2 {
        return None;
    }

    // One (from, to, site) triple per edge of the cycle, in path order.
    let edges: Vec<(ScopeName, ScopeName, &EdgeSite)> = path
        .windows(2)
        .map(|pair| {
            let (from, to) = (pair[0].clone(), pair[1].clone());
            let site = *representative.get(&(from.clone(), to.clone()))?;
            Some((from, to, site))
        })
        .collect::<Option<_>>()?;

    let path_str = path
        .iter()
        .map(ScopeName::as_str)
        .collect::<Vec<_>>()
        .join(" \u{2192} ");
    let members_str = scc
        .iter()
        .map(ScopeName::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    let (first, rest) = edges.split_first()?;
    let (_, _, first_site) = first;

    let mut violation = Violation::new(
        ACYCLIC_CODE,
        rule.display_name(),
        rule.severity(),
        first_site.location.clone(),
        format!("Cycle between scopes {members_str}: {path_str}"),
    );

    for (from, to, site) in rest {
        violation = violation.with_label(Label::new(
            site.location.clone(),
            format!("{from} \u{2192} {to}"),
        ));
    }

    if let Some(doc) = rule.doc_ref() {
        violation = violation.with_doc_ref(doc);
    }

    Some(violation)
}

/// Derives the shortest cycle through `scc`'s smallest node: breadth-first
/// search from that node, successors visited in name order, stopping at the
/// first edge found back to the start (BFS visits nodes in non-decreasing
/// distance order, so this is a shortest cycle through the start, and
/// deterministic since both the queue order and each node's successor order
/// are). Returns the path start ... closing node, with the start appended
/// again to close it (so a 2-node cycle is `[a, b, a]`). Returns an empty
/// vec if `scc` is empty.
fn find_cycle_path(scc: &BTreeSet<ScopeName>, adjacency: &Adjacency) -> Vec<ScopeName> {
    let Some(start) = scc.iter().next() else {
        return Vec::new();
    };

    let mut queue: VecDeque<ScopeName> = VecDeque::new();
    let mut predecessor: HashMap<ScopeName, ScopeName> = HashMap::new();
    let mut visited: HashSet<ScopeName> = HashSet::new();
    visited.insert(start.clone());
    queue.push_back(start.clone());

    let mut closing_at: Option<ScopeName> = None;
    'bfs: while let Some(current) = queue.pop_front() {
        let Some(successors) = adjacency.get(&current) else {
            continue;
        };
        for next in successors {
            if !scc.contains(next) {
                continue;
            }
            if *next == *start {
                // `current != start`: a self-loop cannot exist in `scc`
                // (`ScopeGraph::add_edge` drops `from == to`), so reaching
                // `start` again always closes a real cycle through it.
                closing_at = Some(current);
                break 'bfs;
            }
            if visited.insert(next.clone()) {
                predecessor.insert(next.clone(), current.clone());
                queue.push_back(next.clone());
            }
        }
    }

    let Some(mut node) = closing_at else {
        // Unreachable for a genuine SCC of size > 1: every member then has a
        // path back to every other member, `start` included.
        return Vec::new();
    };

    let mut path = vec![node.clone()];
    while node != *start {
        let Some(prev) = predecessor.get(&node) else {
            return Vec::new();
        };
        path.push(prev.clone());
        node = prev.clone();
    }
    path.reverse();
    path.push(start.clone());
    path
}

/// Tarjan's strongly-connected-components algorithm over `adjacency`.
///
/// Deterministic: nodes are visited in `adjacency`'s key order (sorted scope
/// names) and each node's successors in their own sorted order (the one
/// `BTreeSet` gives), and the returned components are sorted by their
/// smallest member.
fn strongly_connected_components(adjacency: &Adjacency) -> Vec<BTreeSet<ScopeName>> {
    let mut state = Tarjan {
        adjacency,
        index_counter: 0,
        indices: HashMap::new(),
        low_links: HashMap::new(),
        on_stack: HashSet::new(),
        stack: Vec::new(),
        sccs: Vec::new(),
    };

    for node in adjacency.keys() {
        if !state.indices.contains_key(node) {
            state.strong_connect(node);
        }
    }

    state
        .sccs
        .sort_by(|a, b| a.iter().next().cmp(&b.iter().next()));
    state.sccs
}

/// Mutable state threaded through one Tarjan run.
struct Tarjan<'a> {
    adjacency: &'a Adjacency,
    index_counter: usize,
    indices: HashMap<ScopeName, usize>,
    low_links: HashMap<ScopeName, usize>,
    on_stack: HashSet<ScopeName>,
    stack: Vec<ScopeName>,
    sccs: Vec<BTreeSet<ScopeName>>,
}

impl Tarjan<'_> {
    /// Iterative-recursion step of Tarjan's algorithm for node `v`.
    ///
    /// Scope graphs are small (one node per declared scope), so plain
    /// recursion is used rather than an explicit work-stack.
    fn strong_connect(&mut self, v: &ScopeName) {
        let v_index = self.index_counter;
        self.index_counter += 1;
        self.indices.insert(v.clone(), v_index);
        self.low_links.insert(v.clone(), v_index);
        self.stack.push(v.clone());
        self.on_stack.insert(v.clone());

        let successors: Vec<ScopeName> = self
            .adjacency
            .get(v)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();

        for w in &successors {
            if !self.indices.contains_key(w) {
                self.strong_connect(w);
                let w_low = self.low_links.get(w).copied().unwrap_or(usize::MAX);
                let v_low = self.low_links.get(v).copied().unwrap_or(usize::MAX);
                self.low_links.insert(v.clone(), v_low.min(w_low));
            } else if self.on_stack.contains(w) {
                let w_index = self.indices.get(w).copied().unwrap_or(usize::MAX);
                let v_low = self.low_links.get(v).copied().unwrap_or(usize::MAX);
                self.low_links.insert(v.clone(), v_low.min(w_index));
            }
        }

        if self.low_links.get(v) == self.indices.get(v) {
            let mut scc = BTreeSet::new();
            while let Some(w) = self.stack.pop() {
                self.on_stack.remove(&w);
                let is_v = w == *v;
                scc.insert(w);
                if is_v {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::graph::{DirectiveSource, EdgeKind};
    use crate::declarative::model::{DeclarativeConfig, GlobPattern, Scope};
    use crate::types::{Location, Severity};
    use crate::utils::allowance::AllowDirective;
    use std::path::PathBuf;

    fn name(s: &str) -> ScopeName {
        ScopeName::new(s).expect("valid scope name")
    }

    fn site(path: &str, line: usize) -> EdgeSite {
        EdgeSite::new(
            Location::new(PathBuf::from(path), line, 1),
            EdgeKind::Use,
            format!("crate::item{line}"),
        )
    }

    fn allow(rule: &str, reason: Option<&str>) -> AllowDirective {
        AllowDirective {
            rules: [rule.replace('-', "_")].into_iter().collect(),
            reason: reason.map(str::to_string),
        }
    }

    // ── Tarjan SCC ──

    #[test]
    fn scc_finds_two_components_and_isolated_nodes() {
        let mut adjacency: BTreeMap<ScopeName, BTreeSet<ScopeName>> = BTreeMap::new();
        for n in ["a", "b", "c", "d", "e"] {
            adjacency.insert(name(n), BTreeSet::new());
        }
        // a <-> b (one SCC), c -> d -> c (another SCC), e isolated.
        adjacency.get_mut(&name("a")).unwrap().insert(name("b"));
        adjacency.get_mut(&name("b")).unwrap().insert(name("a"));
        adjacency.get_mut(&name("c")).unwrap().insert(name("d"));
        adjacency.get_mut(&name("d")).unwrap().insert(name("c"));

        let sccs = strongly_connected_components(&adjacency);
        // Exactly one component per strongly connected group: {a,b}, {c,d}
        // and the isolated {e}, sorted by each component's smallest member.
        let sizes: Vec<usize> = sccs.iter().map(BTreeSet::len).collect();
        assert_eq!(sizes, [2, 2, 1]);

        let first: Vec<&str> = sccs[0].iter().map(ScopeName::as_str).collect();
        assert_eq!(first, ["a", "b"]);
        let second: Vec<&str> = sccs[1].iter().map(ScopeName::as_str).collect();
        assert_eq!(second, ["c", "d"]);
        let third: Vec<&str> = sccs[2].iter().map(ScopeName::as_str).collect();
        assert_eq!(third, ["e"]);
    }

    #[test]
    fn scc_deterministic_ordering_regardless_of_insertion_order() {
        let mut adjacency: BTreeMap<ScopeName, BTreeSet<ScopeName>> = BTreeMap::new();
        for n in ["z", "a"] {
            adjacency.insert(name(n), BTreeSet::new());
        }
        adjacency.get_mut(&name("z")).unwrap().insert(name("a"));
        adjacency.get_mut(&name("a")).unwrap().insert(name("z"));

        let sccs = strongly_connected_components(&adjacency);
        assert_eq!(sccs.len(), 1);
        let members: Vec<&str> = sccs[0].iter().map(ScopeName::as_str).collect();
        assert_eq!(members, ["a", "z"]);
    }

    // ── check_one / check_project ──

    #[test]
    fn two_scope_cycle_is_one_violation_naming_the_full_path() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1);
        let v = &violations[0];
        assert_eq!(v.code, "ALD009");
        assert_eq!(v.rule, "acyclic");
        assert_eq!(v.severity, Severity::Error);
        assert!(
            v.message.contains("domain \u{2192} infra \u{2192} domain"),
            "{}",
            v.message
        );
        assert_eq!(v.location.file, PathBuf::from("src/domain/a.rs"));
        assert_eq!(v.labels.len(), 1);
        assert_eq!(v.labels[0].location.file, PathBuf::from("src/infra/b.rs"));
    }

    #[test]
    fn doc_ref_carries_through_to_the_violation() {
        // N9: `[[acyclic]]` now accepts `doc`, mirroring the other eight
        // declarative rule kinds.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(
            None,
            None,
            Some("ARCHITECTURE.md L10".to_string()),
            Severity::Error,
        );
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L10")
        );
    }

    #[test]
    fn three_scope_cycle_lists_all_three_in_order() {
        let mut graph = ScopeGraph::with_nodes([name("a"), name("b"), name("c")]);
        graph.add_edge(name("a"), name("b"), site("src/a.rs", 1));
        graph.add_edge(name("b"), name("c"), site("src/b.rs", 1));
        graph.add_edge(name("c"), name("a"), site("src/c.rs", 1));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1);
        assert!(
            violations[0]
                .message
                .contains("a \u{2192} b \u{2192} c \u{2192} a"),
            "{}",
            violations[0].message
        );
        assert_eq!(violations[0].labels.len(), 2);
    }

    #[test]
    fn acyclic_graph_has_no_violation() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn scopes_filter_ignores_a_cycle_through_an_excluded_scope() {
        // domain -> infra -> app -> domain is a cycle, but restricting to
        // ["domain", "app"] removes `infra` from the universe, so the
        // remaining edges (domain -> infra, infra -> app are both dropped;
        // only app -> domain would remain) cannot form a cycle.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra"), name("app")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));
        graph.add_edge(name("infra"), name("app"), site("src/infra/a.rs", 1));
        graph.add_edge(name("app"), name("domain"), site("src/app/a.rs", 1));

        let rule = Acyclic::new(
            None,
            Some(vec![name("domain"), name("app")]),
            None,
            Severity::Error,
        );
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn allow_on_the_only_site_of_an_edge_drops_it_from_the_cycle() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("acyclic", Some("bootstrap")),
            ),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "the suppressed edge must not be part of any cycle"
        );
    }

    #[test]
    fn allow_is_scoped_to_the_rule_it_names() {
        // A directive naming a different rule's display name must not
        // suppress this one.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("some-other-rule", Some("x")),
            ),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert_eq!(check_one(&rule, &graph).len(), 1);
    }

    // ── B1: reason-less `allow` on an error-severity rule ──

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("acyclic", None)),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.code, "ALD009");
        assert_eq!(v.severity, Severity::Warning);
        assert_eq!(
            v.message,
            "Allow directive for 'acyclic' is missing required reason"
        );
        assert_eq!(v.location.file, PathBuf::from("src/domain/a.rs"));
        assert!(
            v.suggestion.is_some(),
            "mirrors Suppressions::decide's MissingReason construction"
        );
    }

    #[test]
    fn comment_without_reason_on_warning_rule_suppresses_silently() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("acyclic", None)),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Warning);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "a non-error rule does not require a reason"
        );
    }

    #[test]
    fn missing_reason_does_not_fire_when_nothing_was_actually_suppressed() {
        // The `allow` sits on a `use` that is not part of any cycle (no
        // `infra -> domain` edge exists), so dropping it changes nothing:
        // no warning should fire.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("acyclic", None)),
        );

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    // ── R1: an attribute directive never needs a reason, even when a
    // comment on the same site or a sibling site does ──

    #[test]
    fn item_attribute_without_reason_suppresses_silently() {
        // F6: `#[arch_lint::allow(acyclic)]` on the `use` item itself, no
        // `reason` — must suppress silently, not warn, because the
        // directive is attribute-sourced (`Item`).
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Item, allow("acyclic", None)),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "an attribute allow never requires a reason"
        );
    }

    #[test]
    fn enclosing_scope_attribute_without_reason_wins_over_a_reasoned_comment() {
        // F9: an enclosing `fn`/`mod`/`impl`/`trait` attribute without a
        // reason, AND a reasoned comment, both name the rule on the same
        // site. Since at least one matching directive is attribute-sourced
        // (`Scope`), the site is suppressed silently regardless of the
        // comment's reason.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        let site_with_both = site("src/domain/a.rs", 1)
            .with_allow_directive(DirectiveSource::Scope, allow("acyclic", None))
            .with_allow_directive(DirectiveSource::Comment, allow("acyclic", Some("reasoned")));
        graph.add_edge(name("domain"), name("infra"), site_with_both);
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn two_reason_less_comment_sites_on_one_edge_each_warn() {
        // F10 / S6: both sites of the same edge are suppressed by a
        // reason-less comment directive — one warning per site, not one
        // per edge.
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("acyclic", None)),
        );
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/c.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("acyclic", None)),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        let violations = check_one(&rule, &graph);
        assert_eq!(
            violations.len(),
            2,
            "no cycle violation: the edge is fully suppressed: {violations:#?}"
        );

        let warnings: Vec<_> = violations
            .iter()
            .filter(|v| v.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 2, "{violations:#?}");
        let mut files: Vec<_> = warnings.iter().map(|v| v.location.file.clone()).collect();
        files.sort();
        assert_eq!(
            files,
            [
                PathBuf::from("src/domain/a.rs"),
                PathBuf::from("src/domain/c.rs"),
            ]
        );
    }

    // ── B2: a `name = "..."` TOML rule answers to kebab or snake case ──

    #[test]
    fn user_named_rule_is_suppressed_by_either_spelling() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("no-cycles", Some("x"))),
        );
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let rule = Acyclic::new(Some("no_cycles".to_string()), None, None, Severity::Error);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "comment used kebab-case, TOML `name` used snake_case: still suppressed"
        );
    }

    // ── S1: cycle path always closes on the SCC's smallest node ──

    #[test]
    fn shortest_cycle_through_the_smallest_node_is_reported_not_a_shortcut() {
        // a -> b -> c -> d -> a, plus a shortcut c -> b. The whole SCC is
        // {a, b, c, d}; the old "stop at the first repeat" walk found
        // b -> c -> b via the shortcut and never reported a or d. BFS from
        // the smallest node (a) must still find the full cycle back to a.
        let mut graph = ScopeGraph::with_nodes([name("a"), name("b"), name("c"), name("d")]);
        graph.add_edge(name("a"), name("b"), site("src/a.rs", 1));
        graph.add_edge(name("b"), name("c"), site("src/b.rs", 1));
        graph.add_edge(name("c"), name("b"), site("src/c.rs", 1));
        graph.add_edge(name("c"), name("d"), site("src/c.rs", 2));
        graph.add_edge(name("d"), name("a"), site("src/d.rs", 1));

        let rule = Acyclic::new(None, None, None, Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(
            violations[0].message,
            "Cycle between scopes a, b, c, d: a \u{2192} b \u{2192} c \u{2192} d \u{2192} a"
        );
        assert_eq!(violations[0].labels.len(), 3);
    }

    #[test]
    fn check_project_runs_every_acyclic_block() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));
        graph.add_edge(name("infra"), name("domain"), site("src/infra/b.rs", 2));

        let config = Arc::new(
            DeclarativeConfig::builder()
                .scopes(vec![
                    Scope::new(
                        name("domain"),
                        vec![GlobPattern::new("src/domain/**").unwrap()],
                    ),
                    Scope::new(
                        name("infra"),
                        vec![GlobPattern::new("src/infra/**").unwrap()],
                    ),
                ])
                .acyclics(vec![Acyclic::new(None, None, None, Severity::Error)])
                .build()
                .unwrap(),
        );
        let rule = AcyclicRule::new(config);
        let root = PathBuf::from(".");
        let ctx = ProjectContext::new(&root).with_scope_graph(graph);
        let violations = rule.check_project(&ctx);
        assert_eq!(violations.len(), 1);
        assert_eq!(rule.name(), "acyclic");
        assert_eq!(rule.code(), "ALD009");
    }
}
