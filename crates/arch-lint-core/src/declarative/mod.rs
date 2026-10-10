//! Declarative architecture rules driven by TOML configuration.
//!
//! This module provides a scope-based model for defining architecture
//! constraints without writing Rust rule code.
//!
//! Sections: `[[scopes]]`, `[[restrict-use]]` (ALD001), `[[require-use]]`
//! (ALD002), `[[deny-scope-dep]]` (ALD003), `[[layering]]` (ALD004),
//! `[[deny-attr]]` (ALD005), `[[require-attr]]` (ALD006), `[[naming]]`
//! (ALD007) and `[[require-return]]` (ALD008).
//!
//! # Architecture
//!
//! ```text
//! TOML text
//!   ↓ serde (DTO layer)
//! config_dto types
//!   ↓ validate + convert           load_config_from_toml()
//! DeclarativeConfig (pure domain model)
//!   ↓ create_rules()               ↓ Analyzer (per-file pass)
//! Vec<RuleBox>                   graph::ScopeGraph → ProjectContext
//! ```
//!
//! [`load_rules_from_toml`](crate::declarative::load_rules_from_toml) is the
//! one-call form of the left column. The right column is the scope
//! dependency graph ([`graph`](crate::declarative::graph)): the analyzer gets
//! the same [`model::DeclarativeConfig`](crate::declarative::model::DeclarativeConfig)
//! through [`AnalyzerBuilder::declarative`](crate::AnalyzerBuilder::declarative)
//! and collects scope-crossing references while it runs the per-file rules.

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
