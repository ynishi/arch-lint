//! Declarative architecture rules driven by TOML configuration.
//!
//! This module provides a scope-based model for defining architecture
//! constraints without writing Rust rule code.
//!
//! # Architecture
//!
//! ```text
//! TOML text
//!   ↓ serde (DTO layer)
//! config_dto types
//!   ↓ validate + convert
//! DeclarativeConfig (pure domain model)
//!   ↓ load_rules_from_toml()
//! Vec<RuleBox>
//! ```

use std::sync::Arc;

pub mod config_dto;
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

/// Parses TOML content and creates all applicable declarative rules.
///
/// Returns `Ok(vec![])` if no declarative sections are present.
///
/// # Errors
///
/// Returns an error if TOML parsing or model validation fails.
pub fn load_rules_from_toml(content: &str) -> Result<Vec<crate::rule::RuleBox>, LoadRulesError> {
    let dto: config_dto::DeclarativeConfigDto = toml::from_str(content)?;
    let config = loader::load(dto)?;
    Ok(create_rules(config))
}

/// Creates all declarative rules from a validated [`model::DeclarativeConfig`].
///
/// Returns an empty vec if no declarative rules are defined.
#[must_use]
pub fn create_rules(config: model::DeclarativeConfig) -> Vec<crate::rule::RuleBox> {
    if config.is_empty() {
        return vec![];
    }

    let config = Arc::new(config);
    let mut result: Vec<crate::rule::RuleBox> = Vec::new();

    if !config.restrict_uses().is_empty() {
        result.push(Box::new(rules::RestrictUseRule::new(Arc::clone(&config))));
    }
    if !config.require_uses().is_empty() {
        result.push(Box::new(rules::RequireUseRule::new(Arc::clone(&config))));
    }
    if !config.scope_deps().is_empty() {
        result.push(Box::new(rules::ScopeDepRule::new(config)));
    }

    result
}
