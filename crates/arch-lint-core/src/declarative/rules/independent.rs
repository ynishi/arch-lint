//! The `[[independent]]` rule (ALD010): a project-wide check that no two of
//! the given scopes depend on each other, in either direction.
//!
//! Like `[[acyclic]]` (ALD009), this is a
//! [`ProjectRule`](crate::rule::ProjectRule) built on the
//! [`ScopeGraph`](crate::declarative::graph::ScopeGraph) the analyzer
//! collects while running the per-file rules
//! ([`ProjectContext::scope_graph`]), not a per-file `syn` AST visitor.
//!
//! For each `[[independent]]` block: every edge `(from, to)` of the graph
//! with both endpoints in the block's `scopes` is an offending edge
//! (`from != to` always holds — [`ScopeGraph::add_edge`] drops self-loops).
//! One violation is reported per such edge, at its first non-suppressed
//! site, with the edge's other non-suppressed sites attached as labels.
//!
//! Suppression reuses [`classify_site`](super::graph_suppress::classify_site),
//! the same per-site classification `[[acyclic]]` uses: an `allow`
//! directive suppresses a site silently when attribute-sourced (`Scope` or
//! `Item`), and needs a `reason` when comment-sourced on an error-severity
//! rule — a reason-less comment site is reported as the shared
//! [`missing_reason_violation`] warning instead, one per site. Unlike
//! `[[acyclic]]`, every edge in the rule's universe is offending by
//! definition (there is no "does this site actually matter" check like
//! `[[acyclic]]`'s cycle membership), so the warning fires for every
//! `MissingReason` site of an error-severity rule unconditionally.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::context::ProjectContext;
use crate::declarative::graph::{EdgeSite, ScopeGraph};
use crate::declarative::model::{DeclarativeConfig, Independent, ScopeName};
use crate::rule::ProjectRule;
use crate::types::{Label, Severity, Violation};

use super::graph_suppress::{classify_site, SiteOutcome};
use super::missing_reason_violation;

/// Kind name used by `allow` directives and [`ProjectRule::name`].
pub(crate) const INDEPENDENT_NAME: &str = "independent";
/// Rule code used by `allow` directives and [`ProjectRule::code`].
pub(crate) const INDEPENDENT_CODE: &str = "ALD010";

/// A project-wide rule that enforces every `[[independent]]` declaration.
pub struct IndependentRule {
    config: Arc<DeclarativeConfig>,
}

impl IndependentRule {
    /// Creates a new independent-scopes rule backed by the given config.
    #[must_use]
    pub fn new(config: Arc<DeclarativeConfig>) -> Self {
        Self { config }
    }
}

impl ProjectRule for IndependentRule {
    fn name(&self) -> &'static str {
        INDEPENDENT_NAME
    }

    fn code(&self) -> &'static str {
        INDEPENDENT_CODE
    }

    fn description(&self) -> &'static str {
        "Detect dependencies between scopes declared independent of each other"
    }

    fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation> {
        self.config
            .independents()
            .iter()
            .flat_map(|rule| check_one(rule, ctx.scope_graph()))
            .collect()
    }
}

/// The nodes `rule` restricts the check to: the rule's own `scopes`.
fn universe(rule: &Independent) -> BTreeSet<ScopeName> {
    rule.scopes().iter().cloned().collect()
}

/// Runs one `[[independent]]` block against `graph`: one violation per
/// offending edge, plus any missing-reason warnings.
fn check_one(rule: &Independent, graph: &ScopeGraph) -> Vec<Violation> {
    let nodes = universe(rule);
    let mut violations = Vec::new();

    for (from, to, sites) in graph.edges() {
        if !nodes.contains(from) || !nodes.contains(to) {
            continue;
        }

        let mut reporting_site: Option<&EdgeSite> = None;
        let mut label_sites: Vec<&EdgeSite> = Vec::new();

        for site in sites {
            match classify_site(
                site,
                INDEPENDENT_NAME,
                INDEPENDENT_CODE,
                &rule.display_name(),
            ) {
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
                            INDEPENDENT_CODE,
                            &rule.display_name(),
                            site.location.clone(),
                        ));
                    }
                }
            }
        }

        if let Some(site) = reporting_site {
            violations.push(build_violation(rule, from, to, site, &label_sites));
        }
    }

    violations
}

/// Builds the violation for one offending edge `from -> to`, reported at
/// `site`, with `label_sites` (the edge's other non-suppressed sites)
/// attached as labels.
fn build_violation(
    rule: &Independent,
    from: &ScopeName,
    to: &ScopeName,
    site: &EdgeSite,
    label_sites: &[&EdgeSite],
) -> Violation {
    let mut violation = Violation::new(
        INDEPENDENT_CODE,
        rule.display_name(),
        rule.severity(),
        site.location.clone(),
        format!(
            "Independent scopes {from} and {to} must not depend on each other: {from} \u{2192} {to}"
        ),
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

    fn independent(scopes: Vec<ScopeName>, severity: Severity) -> Independent {
        Independent::new(None, scopes, None, severity).expect("at least 2 scopes")
    }

    // ── Acceptance 1 / 3: offending edges ──

    #[test]
    fn one_directional_edge_is_one_violation_naming_both_scopes() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.code, "ALD010");
        assert_eq!(v.rule, "independent");
        assert_eq!(v.severity, Severity::Error);
        assert!(
            v.message.contains("billing \u{2192} shipping"),
            "{}",
            v.message
        );
        assert_eq!(v.location.file, PathBuf::from("src/billing/a.rs"));
        assert!(v.labels.is_empty());
    }

    #[test]
    fn both_directions_are_two_separate_violations() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );
        graph.add_edge(
            name("shipping"),
            name("billing"),
            site("src/shipping/b.rs", 2),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 2, "{violations:#?}");
    }

    #[test]
    fn doc_ref_carries_through_to_the_violation() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );

        let rule = Independent::new(
            None,
            vec![name("billing"), name("shipping")],
            Some("ARCHITECTURE.md L20".to_string()),
            Severity::Error,
        )
        .unwrap();
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].doc_ref.as_deref(),
            Some("ARCHITECTURE.md L20")
        );
    }

    #[test]
    fn third_scope_added_to_universe_reports_a_second_violation() {
        // Acceptance 3, as a real before/after over the same graph: with
        // `scopes = ["billing", "shipping"]`, the `catalog -> billing` edge
        // sits outside the universe and is ignored (1 violation). Adding
        // `catalog` to `scopes`, with no other change, reports a second
        // violation for that edge too.
        let mut graph =
            ScopeGraph::with_nodes([name("billing"), name("shipping"), name("catalog")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );
        graph.add_edge(
            name("catalog"),
            name("billing"),
            site("src/catalog/c.rs", 1),
        );

        let before = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let before_violations = check_one(&before, &graph);
        assert_eq!(
            before_violations.len(),
            1,
            "catalog -> billing must be ignored before catalog joins scopes: {before_violations:#?}"
        );

        let after = independent(
            vec![name("billing"), name("shipping"), name("catalog")],
            Severity::Error,
        );
        let after_violations = check_one(&after, &graph);
        assert_eq!(
            after_violations.len(),
            2,
            "catalog -> billing must be reported once catalog joins scopes: {after_violations:#?}"
        );
    }

    #[test]
    fn edge_outside_the_universe_is_ignored() {
        // Acceptance 2: `billing -> shared`, with `shared` not in `scopes`,
        // is not reported.
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping"), name("shared")]);
        graph.add_edge(name("billing"), name("shared"), site("src/billing/a.rs", 1));

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn no_edge_between_the_scopes_has_no_violation() {
        let graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    // ── Suppression ──

    #[test]
    fn allow_on_the_only_site_of_an_edge_drops_it() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("independent", Some("bootstrap")),
            ),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn allow_is_scoped_to_the_rule_it_names() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1).with_allow_directive(
                DirectiveSource::Comment,
                allow("some-other-rule", Some("x")),
            ),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert_eq!(check_one(&rule, &graph).len(), 1);
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns_instead() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("independent", None)),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.code, "ALD010");
        assert_eq!(v.severity, Severity::Warning);
        assert_eq!(
            v.message,
            "Allow directive for 'independent' is missing required reason"
        );
        assert_eq!(v.location.file, PathBuf::from("src/billing/a.rs"));
    }

    #[test]
    fn comment_without_reason_on_warning_rule_suppresses_silently() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("independent", None)),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Warning);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "a non-error rule does not require a reason"
        );
    }

    #[test]
    fn item_attribute_without_reason_suppresses_silently() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1)
                .with_allow_directive(DirectiveSource::Item, allow("independent", None)),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert!(
            check_one(&rule, &graph).is_empty(),
            "an attribute allow never requires a reason"
        );
    }

    #[test]
    fn enclosing_scope_attribute_without_reason_wins_over_a_reasoned_comment() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        let site_with_both = site("src/billing/a.rs", 1)
            .with_allow_directive(DirectiveSource::Scope, allow("independent", None))
            .with_allow_directive(
                DirectiveSource::Comment,
                allow("independent", Some("reasoned")),
            );
        graph.add_edge(name("billing"), name("shipping"), site_with_both);

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        assert!(check_one(&rule, &graph).is_empty());
    }

    #[test]
    fn two_reason_less_comment_sites_on_one_edge_each_warn_and_the_edge_is_dropped() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("independent", None)),
        );
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/c.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("independent", None)),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let violations = check_one(&rule, &graph);
        assert_eq!(
            violations.len(),
            2,
            "no edge violation: both sites of the one edge are suppressed: {violations:#?}"
        );
        assert!(violations.iter().all(|v| v.severity == Severity::Warning));
        let mut files: Vec<_> = violations.iter().map(|v| v.location.file.clone()).collect();
        files.sort();
        assert_eq!(
            files,
            [
                PathBuf::from("src/billing/a.rs"),
                PathBuf::from("src/billing/c.rs"),
            ]
        );
    }

    #[test]
    fn remaining_sites_of_the_same_edge_are_attached_as_labels() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/c.rs", 1),
        );

        let rule = independent(vec![name("billing"), name("shipping")], Severity::Error);
        let violations = check_one(&rule, &graph);

        assert_eq!(
            violations.len(),
            1,
            "one violation per edge: {violations:#?}"
        );
        assert_eq!(
            violations[0].location.file,
            PathBuf::from("src/billing/a.rs")
        );
        assert_eq!(violations[0].labels.len(), 1);
        assert_eq!(
            violations[0].labels[0].location.file,
            PathBuf::from("src/billing/c.rs")
        );
    }

    #[test]
    fn user_named_rule_is_suppressed_by_either_spelling() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1)
                .with_allow_directive(DirectiveSource::Comment, allow("no-shared-deps", Some("x"))),
        );

        let rule = Independent::new(
            Some("no_shared_deps".to_string()),
            vec![name("billing"), name("shipping")],
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
    fn check_project_runs_every_independent_block() {
        let mut graph = ScopeGraph::with_nodes([name("billing"), name("shipping")]);
        graph.add_edge(
            name("billing"),
            name("shipping"),
            site("src/billing/a.rs", 1),
        );

        let config = Arc::new(
            DeclarativeConfig::builder()
                .scopes(vec![
                    Scope::new(
                        name("billing"),
                        vec![GlobPattern::new("src/billing/**").unwrap()],
                    ),
                    Scope::new(
                        name("shipping"),
                        vec![GlobPattern::new("src/shipping/**").unwrap()],
                    ),
                ])
                .independents(vec![independent(
                    vec![name("billing"), name("shipping")],
                    Severity::Error,
                )])
                .build()
                .unwrap(),
        );
        let rule = IndependentRule::new(config);
        let root = PathBuf::from(".");
        let ctx = ProjectContext::new(&root).with_scope_graph(graph);
        let violations = rule.check_project(&ctx);
        assert_eq!(violations.len(), 1);
        assert_eq!(rule.name(), "independent");
        assert_eq!(rule.code(), "ALD010");
    }
}
