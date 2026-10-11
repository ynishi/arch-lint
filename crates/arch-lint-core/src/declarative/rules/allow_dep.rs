//! The `[[allow-dep]]` rule (ALD011): a project-wide allow-list on the
//! dependencies a scope may have.
//!
//! Like `[[acyclic]]` (ALD009) and `[[independent]]` (ALD010), this is a
//! [`ProjectRule`](crate::rule::ProjectRule) built on the
//! [`ScopeGraph`](crate::declarative::graph::ScopeGraph) the analyzer
//! collects while running the per-file rules
//! ([`ProjectContext::scope_graph`]), not a per-file `syn` AST visitor.
//!
//! For each `[[allow-dep]]` block (`from`, `to`): every edge `(from, X)` of
//! the graph where `X` is a declared scope not in `to` is an offending edge
//! (`X != from` always holds — [`ScopeGraph::add_edge`] drops self-loops, and
//! [`AllowDep::new`] rejects a `to` that contains `from`). A reference that
//! resolves to no declared scope (an external crate, an unscoped file) never
//! reaches the graph in the first place, so it is never a violation here
//! either. `to = []` forbids every declared-scope dependency from `from`.
//! One violation is reported per such edge, at its first non-suppressed
//! site, with the edge's other non-suppressed sites attached as labels.
//! Multiple `[[allow-dep]]` blocks with the same `from` are each checked
//! independently — the edges allowed by one do not widen another.
//!
//! Suppression reuses [`classify_site`](super::graph_suppress::classify_site),
//! the same per-site classification `[[acyclic]]` and `[[independent]]` use:
//! an `allow` directive suppresses a site silently when attribute-sourced
//! (`Scope` or `Item`), and needs a `reason` when comment-sourced on an
//! error-severity rule — a reason-less comment site is reported as the
//! shared [`missing_reason_violation`] warning instead. Like
//! `[[independent]]`, every edge in the rule's universe is offending by
//! definition, so the warning fires for every `MissingReason` site of an
//! error-severity rule unconditionally.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::context::ProjectContext;
use crate::declarative::graph::{EdgeSite, ScopeGraph};
use crate::declarative::model::{AllowDep, DeclarativeConfig, ScopeName};
use crate::rule::ProjectRule;
use crate::types::{Label, Severity, Violation};

use super::graph_suppress::{classify_site, SiteOutcome};
use super::missing_reason_violation;

/// Kind name used by `allow` directives and [`ProjectRule::name`].
pub(crate) const ALLOW_DEP_NAME: &str = "allow-dep";
/// Rule code used by `allow` directives and [`ProjectRule::code`].
pub(crate) const ALLOW_DEP_CODE: &str = "ALD011";

/// A project-wide rule that enforces every `[[allow-dep]]` declaration.
pub struct AllowDepRule {
    config: Arc<DeclarativeConfig>,
}

impl AllowDepRule {
    /// Creates a new allow-list rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl ProjectRule for AllowDepRule {
    fn name(&self) -> &'static str {
        ALLOW_DEP_NAME
    }

    fn code(&self) -> &'static str {
        ALLOW_DEP_CODE
    }

    fn description(&self) -> &'static str {
        "Detect dependencies from a scope that fall outside its declared allow-list"
    }

    fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation> {
        self.config
            .allow_deps()
            .iter()
            .flat_map(|rule| check_one(rule, ctx.scope_graph()))
            .collect()
    }
}

/// Runs one `[[allow-dep]]` block against `graph`: one violation per
/// offending edge out of `rule.from_scope()`, plus any missing-reason
/// warnings.
fn check_one(rule: &AllowDep, graph: &ScopeGraph) -> Vec<Violation> {
    let allowed: BTreeSet<&ScopeName> = rule.to().iter().collect();
    let mut violations = Vec::new();

    for (from, to, sites) in graph.edges() {
        if from != rule.from_scope() || allowed.contains(to) {
            continue;
        }

        let mut reporting_site: Option<&EdgeSite> = None;
        let mut label_sites: Vec<&EdgeSite> = Vec::new();

        for site in sites {
            match classify_site(site, ALLOW_DEP_NAME, ALLOW_DEP_CODE, &rule.display_name()) {
                SiteOutcome::Report => {
                    if reporting_site.is_none() {
                        reporting_site = Some(site);
                    } else {
                        label_sites.push(site);
                    }
                }
                SiteOutcome::Suppressed => {}
                SiteOutcome::MissingReason => {
                    if rule.severity() == Severity::Error {
                        violations.push(missing_reason_violation(
                            ALLOW_DEP_CODE,
                            &rule.display_name(),
                            site.location.clone(),
                        ));
                    }
                }
            }
        }

        if let Some(site) = reporting_site {
            violations.push(build_violation(rule, to, site, &label_sites));
        }
    }

    violations
}

/// Builds the violation for one offending edge `rule.from_scope() -> to`,
/// reported at `site`, with `label_sites` (the edge's other non-suppressed
/// sites) attached as labels.
fn build_violation(
    rule: &AllowDep,
    to: &ScopeName,
    site: &EdgeSite,
    label_sites: &[&EdgeSite],
) -> Violation {
    let from = rule.from_scope();
    let allow_list = rule
        .to()
        .iter()
        .map(ScopeName::as_str)
        .collect::<Vec<_>>()
        .join(", ");

    let mut violation = Violation::new(
        ALLOW_DEP_CODE,
        rule.display_name(),
        rule.severity(),
        site.location.clone(),
        format!("Scope `{from}` may only depend on [{allow_list}]: {from} \u{2192} {to}"),
    );

    for label_site in label_sites {
        violation = violation.with_label(Label::new(
            label_site.location.clone(),
            format!("{from} \u{2192} {to}"),
        ));
    }

    if let Some(doc) = rule.doc_ref() {
        violation = violation.with_doc_ref(doc);
    }

    violation
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

    fn allow_dep(from: ScopeName, to: Vec<ScopeName>, severity: Severity) -> AllowDep {
        AllowDep::new(None, from, to, None, severity).expect("valid allow-dep")
    }

    // ── Acceptance 1: an edge outside `to` is a violation ──

    #[test]
    fn edge_outside_to_is_one_violation_naming_the_allow_list() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("shared"), site("src/domain/a.rs", 1));
        graph.add_edge(name("domain"), name("infra"), site("src/domain/b.rs", 1));

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.code, "ALD011");
        assert_eq!(v.rule, "allow-dep:domain");
        assert_eq!(v.severity, Severity::Error);
        assert_eq!(
            v.message,
            "Scope `domain` may only depend on [shared]: domain \u{2192} infra"
        );
        assert_eq!(v.location.file, PathBuf::from("src/domain/b.rs"));
        assert!(v.labels.is_empty());
    }

    #[test]
    fn edge_inside_to_has_no_violation() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared")]);
        graph.add_edge(name("domain"), name("shared"), site("src/domain/a.rs", 1));

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    // ── Acceptance 2: no edge at all for a path outside every scope ──

    #[test]
    fn no_edge_between_the_scopes_has_no_violation() {
        let graph = ScopeGraph::with_nodes([name("domain"), name("shared")]);
        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn edges_from_a_different_scope_are_ignored() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("infra"), name("shared")]);
        graph.add_edge(name("infra"), name("domain"), site("src/infra/a.rs", 1));

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    // ── Acceptance 3: `to = []` forbids every declared-scope dependency ──

    #[test]
    fn empty_to_forbids_every_declared_scope_dependency() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("shared"), site("src/domain/a.rs", 1));
        graph.add_edge(name("domain"), name("infra"), site("src/domain/b.rs", 1));

        let rule = allow_dep(name("domain"), vec![], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 2, "{violations:#?}");
        for v in &violations {
            assert!(v.message.contains("may only depend on []"), "{}", v.message);
        }
    }

    // ── Multiple blocks for the same `from` are checked independently ──

    #[test]
    fn two_blocks_for_the_same_from_are_each_checked_against_their_own_to() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("shared"), site("src/domain/a.rs", 1));
        graph.add_edge(name("domain"), name("infra"), site("src/domain/b.rs", 1));

        let allows_shared = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        let allows_infra = allow_dep(name("domain"), vec![name("infra")], Severity::Error);

        // `domain -> infra` is reported by the block that only allows
        // `shared`, and `domain -> shared` by the block that only allows
        // `infra` — neither block's `to` widens the other's.
        let v1 = check_one(&allows_shared, &graph);
        assert_eq!(v1.len(), 1, "{v1:#?}");
        assert!(v1[0].message.contains("domain \u{2192} infra"));

        let v2 = check_one(&allows_infra, &graph);
        assert_eq!(v2.len(), 1, "{v2:#?}");
        assert!(v2[0].message.contains("domain \u{2192} shared"));
    }

    #[test]
    fn doc_ref_carries_through_to_the_violation() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));

        let rule = AllowDep::new(
            None,
            name("domain"),
            vec![name("shared")],
            Some("ARCHITECTURE.md L30".to_string()),
            Severity::Error,
        )
        .unwrap();
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L30")
        );
    }

    // ── Suppression ──

    #[test]
    fn allow_on_the_only_site_of_an_edge_drops_it() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("allow-dep", Some("bootstrap")),
            ),
        );

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn allow_is_scoped_to_the_rule_it_names() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("some-other-rule", Some("x")),
            ),
        );

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert_eq!(check_one(&rule, &graph).len(), 1);
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("allow-dep", None)),
        );

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.code, "ALD011");
        assert_eq!(v.severity, Severity::Warning);
        assert_eq!(
            v.message,
            "Allow directive for 'allow-dep:domain' is missing required reason"
        );
        assert_eq!(v.location.file, PathBuf::from("src/domain/a.rs"));
    }

    #[test]
    fn comment_without_reason_on_warning_rule_suppresses_silently() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("allow-dep", None)),
        );

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Warning);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "a non-error rule does not require a reason"
        );
    }

    #[test]
    fn item_attribute_without_reason_suppresses_silently() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Item, allow("allow-dep", None)),
        );

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "an attribute allow never requires a reason"
        );
    }

    #[test]
    fn remaining_sites_of_the_same_edge_are_attached_as_labels() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));
        graph.add_edge(name("domain"), name("infra"), site("src/domain/c.rs", 1));

        let rule = allow_dep(name("domain"), vec![name("shared")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(
            violations.len(),
            1,
            "one violation per edge: {violations:#?}"
        );
        assert_eq!(
            violations[0].location.file,
            PathBuf::from("src/domain/a.rs")
        );
        assert_eq!(violations[0].labels.len(), 1);
        assert_eq!(
            violations[0].labels[0].location.file,
            PathBuf::from("src/domain/c.rs")
        );
    }

    #[test]
    fn user_named_rule_is_suppressed_by_either_spelling() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(
            name("domain"),
            name("infra"),
            site("src/domain/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("domain-allow", Some("x"))),
        );

        let rule = AllowDep::new(
            Some("domain_allow".to_string()),
            name("domain"),
            vec![name("shared")],
            None,
            Severity::Error,
        )
        .unwrap();
        assert!(
            check_one(&rule, &graph).is_empty(),
            "comment used kebab-case, TOML `name` used snake_case: still suppressed"
        );
    }

    #[test]
    fn check_project_runs_every_allow_dep_block() {
        let mut graph = ScopeGraph::with_nodes([name("domain"), name("shared"), name("infra")]);
        graph.add_edge(name("domain"), name("infra"), site("src/domain/a.rs", 1));

        let config = Arc::new(
            DeclarativeConfig::builder()
                .scopes(vec![
                    Scope::new(
                        name("domain"),
                        vec![GlobPattern::new("src/domain/**").unwrap()],
                    ),
                    Scope::new(
                        name("shared"),
                        vec![GlobPattern::new("src/shared/**").unwrap()],
                    ),
                    Scope::new(
                        name("infra"),
                        vec![GlobPattern::new("src/infra/**").unwrap()],
                    ),
                ])
                .allow_deps(vec![allow_dep(
                    name("domain"),
                    vec![name("shared")],
                    Severity::Error,
                )])
                .build()
                .unwrap(),
        );
        let rule = AllowDepRule::new(config);
        let root = PathBuf::from(".");
        let ctx = ProjectContext::new(&root).with_scope_graph(graph);
        let violations = rule.check_project(&ctx);
        assert_eq!(violations.len(), 1);
        assert_eq!(rule.name(), "allow-dep");
        assert_eq!(rule.code(), "ALD011");
    }
}
