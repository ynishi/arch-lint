//! Declarative architecture rules driven by TOML configuration.
//!
//! This module provides a scope-based model for defining architecture
//! constraints without writing Rust rule code.
//!
//! Sections: `[[scopes]]`, `[[restrict-use]]` (ALD001), `[[require-use]]`
//! (ALD002), `[[deny-scope-dep]]` (ALD003), `[[layering]]` (ALD004),
//! `[[deny-attr]]` (ALD005), `[[require-attr]]` (ALD006), `[[naming]]`
//! (ALD007), `[[require-return]]` (ALD008) and `[[acyclic]]` (ALD009).
//!
//! # Architecture
//!
//! ```text
//! TOML text
//!   ↓ serde (DTO layer)
//! config_dto types
//!   ↓ validate + convert                  load_config_from_toml()
//! DeclarativeConfig (pure domain model)
//!   ↓ create_rules()          ↓ create_project_rules()   ↓ Analyzer (per-file pass)
//! Vec<RuleBox>               Vec<ProjectRuleBox>        graph::ScopeGraph → ProjectContext
//! ```
//!
//! [`load_rules_from_toml`](crate::declarative::load_rules_from_toml) is the
//! one-call form of the left column. The middle column is `[[acyclic]]`
//! (ALD009), the one rule kind that runs once per project rather than once
//! per file —
//! [`create_project_rules_shared`](crate::declarative::create_project_rules_shared)
//! builds it the same way
//! [`create_rules_shared`](crate::declarative::create_rules_shared) builds
//! the per-file rules. The right column is the
//! scope dependency graph ([`graph`](crate::declarative::graph)): the
//! analyzer gets the same
//! [`model::DeclarativeConfig`](crate::declarative::model::DeclarativeConfig)
//! through [`AnalyzerBuilder::declarative`](crate::AnalyzerBuilder::declarative),
//! collects scope-crossing references while it runs the per-file rules, and
//! hands the graph to the project rules through
//! [`ProjectContext::scope_graph`](crate::ProjectContext::scope_graph) — which
//! is what the middle column reads.

use std::sync::Arc;

pub mod config_dto;
pub mod graph;
pub mod loader;
pub mod model;
pub mod rules;

/// Errors from parsing TOML and loading declarative rules.
#[derive(Debug, thiserror::Error)]
pub enum LoadRulesError {
    /// TOML deserialization failed.
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    /// Domain model validation failed.
    #[error("{0}")]
    Load(#[from] loader::LoadError),
}

/// Parses TOML content into a validated [`model::DeclarativeConfig`].
///
/// A document without declarative sections yields an empty config (no
/// scopes, no rules). This is the entry point for callers that need the
/// config itself, to build rules with [`create_rules_shared`] and to hand the
/// same instance to [`AnalyzerBuilder::declarative`](crate::AnalyzerBuilder::declarative).
///
/// # Errors
///
/// Returns an error if TOML parsing or model validation fails.
pub fn load_config_from_toml(content: &str) -> Result<model::DeclarativeConfig, LoadRulesError> {
    let dto: config_dto::DeclarativeConfigDto = toml::from_str(content)?;
    Ok(loader::load(dto)?)
}

/// Parses TOML content and creates all applicable declarative rules.
///
/// Returns `Ok(vec![])` if no declarative sections are present.
///
/// # Errors
///
/// Returns an error if TOML parsing or model validation fails.
pub fn load_rules_from_toml(content: &str) -> Result<Vec<crate::rule::RuleBox>, LoadRulesError> {
    Ok(create_rules(load_config_from_toml(content)?))
}

/// Creates all declarative rules from a validated [`model::DeclarativeConfig`].
///
/// Returns an empty vec if no declarative rules are defined. Use
/// [`create_rules_shared`] when the config is also needed elsewhere.
#[must_use]
pub fn create_rules(config: model::DeclarativeConfig) -> Vec<crate::rule::RuleBox> {
    create_rules_shared(&Arc::new(config))
}

/// Creates all declarative rules from a shared [`model::DeclarativeConfig`].
///
/// The rules keep a clone of the `Arc`, so the caller can hand the same
/// config to the analyzer for scope-graph collection without copying it.
/// Returns an empty vec if no declarative rules are defined.
#[must_use]
pub fn create_rules_shared(config: &Arc<model::DeclarativeConfig>) -> Vec<crate::rule::RuleBox> {
    if config.is_empty() {
        return vec![];
    }

    let config = Arc::clone(config);
    let mut result: Vec<crate::rule::RuleBox> = Vec::new();

    if !config.restrict_uses().is_empty() {
        result.push(Box::new(rules::RestrictUseRule::new(Arc::clone(&config))));
    }
    if !config.require_uses().is_empty() {
        result.push(Box::new(rules::RequireUseRule::new(Arc::clone(&config))));
    }
    if !config.scope_deps().is_empty() {
        result.push(Box::new(rules::ScopeDepRule::new(Arc::clone(&config))));
    }
    if !config.layerings().is_empty() {
        result.push(Box::new(rules::LayeringRule::new(Arc::clone(&config))));
    }
    if !config.deny_attrs().is_empty() {
        result.push(Box::new(rules::DenyAttrRule::new(Arc::clone(&config))));
    }
    if !config.require_attrs().is_empty() {
        result.push(Box::new(rules::RequireAttrRule::new(Arc::clone(&config))));
    }
    if !config.namings().is_empty() {
        result.push(Box::new(rules::NamingRule::new(Arc::clone(&config))));
    }
    if !config.require_returns().is_empty() {
        result.push(Box::new(rules::RequireReturnRule::new(config)));
    }

    result
}

/// Creates all project-wide declarative rules from a validated
/// [`model::DeclarativeConfig`].
///
/// Returns an empty vec if no `[[acyclic]]` blocks are defined. Use
/// [`create_project_rules_shared`] when the config is also needed elsewhere
/// (for example, to hand the same instance to
/// [`AnalyzerBuilder::declarative`](crate::AnalyzerBuilder::declarative), so
/// the analyzer collects the [`graph::ScopeGraph`] these rules read).
#[must_use]
pub fn create_project_rules(config: model::DeclarativeConfig) -> Vec<crate::rule::ProjectRuleBox> {
    create_project_rules_shared(&Arc::new(config))
}

/// Creates all project-wide declarative rules from a shared
/// [`model::DeclarativeConfig`].
///
/// The rules keep a clone of the `Arc`, so the caller can hand the same
/// config to the analyzer for scope-graph collection without copying it.
/// Returns an empty vec if no `[[acyclic]]` blocks are defined.
#[must_use]
pub fn create_project_rules_shared(
    config: &Arc<model::DeclarativeConfig>,
) -> Vec<crate::rule::ProjectRuleBox> {
    if config.is_empty() {
        return vec![];
    }

    let mut result: Vec<crate::rule::ProjectRuleBox> = Vec::new();

    if !config.acyclics().is_empty() {
        result.push(Box::new(rules::AcyclicRule::new(Arc::clone(config))));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::{Acyclic, GlobPattern, Scope, ScopeName};
    use crate::types::Severity;

    #[test]
    fn create_project_rules_is_empty_without_acyclic() {
        let config = model::DeclarativeConfig::empty();
        assert!(create_project_rules(config).is_empty());
    }

    #[test]
    fn create_project_rules_shared_builds_the_acyclic_rule() {
        let config = Arc::new(
            model::DeclarativeConfig::builder()
                .scopes(vec![Scope::new(
                    ScopeName::new("domain").unwrap(),
                    vec![GlobPattern::new("src/domain/**").unwrap()],
                )])
                .acyclics(vec![Acyclic::new(None, None, None, Severity::Error)])
                .build()
                .unwrap(),
        );
        let rules = create_project_rules_shared(&config);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name(), "acyclic");
        assert_eq!(rules[0].code(), "ALD009");
    }
}
