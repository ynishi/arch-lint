//! Per-site suppression classification shared by the project-wide rules
//! built on the [`ScopeGraph`](crate::declarative::graph::ScopeGraph):
//! `[[acyclic]]` (ALD009) and `[[independent]]` (ALD010).
//!
//! Both rules decide, independently for each [`EdgeSite`] of an edge,
//! whether an `allow` directive suppresses it, and if so whether that
//! suppression needed a `reason`. The logic is identical for both rule
//! kinds — only the three names a directive may use to address the rule
//! (its kind name, its code, and its TOML `name` or fallback) differ — so it
//! lives here once rather than being copy-pasted.

use crate::declarative::graph::{DirectiveSource, EdgeSite};
use crate::utils::allowance::AllowDirective;

/// What a rule does with one site, given every directive naming it there.
pub(crate) enum SiteOutcome {
    /// No directive names the rule: the site stands, so the edge survives.
    Report,
    /// At least one matching directive is attribute-sourced (`Scope` or
    /// `Item`), which never requires a `reason` — silently suppressed,
    /// matching `Suppressions::decide`'s attribute path.
    Suppressed,
    /// Every matching directive is `Comment`-sourced and none carries a
    /// `reason`. The site is still dropped, but the caller may report a
    /// warning in its place, matching `Suppressions::decide`'s comment path.
    MissingReason,
}

/// Every directive on `site` naming `kind_name`, `code`, or `display_name`
/// (each normalized), or `"all"`, together with where it came from.
///
/// Unlike a single best match, this returns *every* one, source included,
/// because whether the site is suppressed silently or needs a `reason`
/// depends on whether *any* matching directive is attribute-sourced, not
/// just the first one found (R1: an attribute and a comment can both name
/// the rule on the same site).
fn matching_directives<'s>(
    site: &'s EdgeSite,
    kind_name: &str,
    code: &str,
    display_name: &str,
) -> Vec<(DirectiveSource, &'s AllowDirective)> {
    let names = [
        kind_name.replace('-', "_"),
        code.replace('-', "_"),
        display_name.replace('-', "_"),
    ];
    site.allow_directives()
        .iter()
        .filter(|(_, directive)| {
            directive.rules.contains("all")
                || names.iter().any(|name| directive.rules.contains(name))
        })
        .map(|(source, directive)| (*source, directive))
        .collect()
}

/// Classifies one site for the rule identified by `kind_name` / `code` /
/// `display_name` (see [`SiteOutcome`]).
pub(crate) fn classify_site(
    site: &EdgeSite,
    kind_name: &str,
    code: &str,
    display_name: &str,
) -> SiteOutcome {
    let matches = matching_directives(site, kind_name, code, display_name);
    if matches.is_empty() {
        return SiteOutcome::Report;
    }
    if matches
        .iter()
        .any(|(source, _)| !matches!(source, DirectiveSource::Comment))
    {
        return SiteOutcome::Suppressed;
    }
    if matches
        .iter()
        .any(|(_, directive)| directive.reason.is_some())
    {
        SiteOutcome::Suppressed
    } else {
        SiteOutcome::MissingReason
    }
}
