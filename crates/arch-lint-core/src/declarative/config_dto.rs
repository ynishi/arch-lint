//! TOML deserialization types (DTO layer).
//!
//! These types exist solely for serde deserialization.
//! They are converted to domain model types via the loader.

use serde::Deserialize;

/// Raw TOML representation of declarative rules.
///
/// Extends the base `Config` with `[[scopes]]`, `[[restrict-use]]`,
/// `[[require-use]]`, and `[[deny-scope-dep]]` sections.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DeclarativeConfigDto {
    /// Named scope definitions.
    #[serde(rename = "scopes", default)]
    pub scopes: Vec<ScopeDto>,

    /// Use-restriction rules.
    #[serde(rename = "restrict-use", default)]
    pub restrict_use: Vec<RestrictUseDto>,

    /// Use-requirement rules.
    #[serde(rename = "require-use", default)]
    pub require_use: Vec<RequireUseDto>,

    /// Scope dependency constraints.
    #[serde(rename = "deny-scope-dep", default)]
    pub deny_scope_dep: Vec<ScopeDepDto>,
}

/// TOML representation of a named scope.
#[derive(Debug, Clone, Deserialize)]
pub struct ScopeDto {
    /// Scope name (e.g., "domain").
    pub name: String,
    /// Glob patterns for file paths.
    pub paths: Vec<String>,
}

/// TOML representation of a restrict-use rule.
#[derive(Debug, Clone, Deserialize)]
pub struct RestrictUseDto {
    /// Rule name (e.g., "no-sqlx-in-domain").
    pub name: String,
    /// Named scope reference (mutually exclusive with `files`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns (mutually exclusive with `scope`).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Denied use-path patterns.
    pub deny: Vec<String>,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
}

/// TOML representation of a require-use rule.
#[derive(Debug, Clone, Deserialize)]
pub struct RequireUseDto {
    /// Rule name (e.g., "require-tracing-over-log").
    pub name: String,
    /// Named scope reference.
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns.
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Preferred crate.
    pub prefer: String,
    /// Discouraged crates.
    pub over: Vec<String>,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "warning").
    #[serde(default = "default_severity_warning_str")]
    pub severity: String,
}

/// TOML representation of a scope dependency constraint.
#[derive(Debug, Clone, Deserialize)]
pub struct ScopeDepDto {
    /// Optional rule name (e.g., "no-domain-to-infra").
    #[serde(default)]
    pub name: Option<String>,
    /// Source scope name.
    pub from: String,
    /// Denied target scope names.
    pub to: Vec<String>,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
}

fn default_severity_str() -> String {
    "error".to_string()
}

fn default_severity_warning_str() -> String {
    "warning".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_empty() {
        let dto: DeclarativeConfigDto = toml::from_str("").unwrap();
        assert!(dto.scopes.is_empty());
        assert!(dto.restrict_use.is_empty());
        assert!(dto.deny_scope_dep.is_empty());
    }

    #[test]
    fn deserialize_full_config() {
        let toml_str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::*", "diesel::*"]
message = "Domain must be DB-agnostic."
doc = "ARCHITECTURE.md L85"

[[require-use]]
name = "require-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing, not log."

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert_eq!(dto.scopes.len(), 2);
        assert_eq!(dto.restrict_use.len(), 1);
        assert_eq!(dto.restrict_use[0].scope, Some("domain".to_string()));
        assert_eq!(dto.restrict_use[0].deny.len(), 2);
        assert_eq!(dto.require_use.len(), 1);
        assert_eq!(dto.deny_scope_dep.len(), 1);
        assert_eq!(dto.deny_scope_dep[0].severity, "error");
    }

    #[test]
    fn deserialize_inline_files() {
        let toml_str = r#"
[[restrict-use]]
name = "no-sqlx-in-handlers"
files = ["src/handlers/**"]
deny = ["sqlx::*"]
message = "Handlers must use repository."
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert_eq!(dto.restrict_use.len(), 1);
        assert!(dto.restrict_use[0].scope.is_none());
        assert_eq!(
            dto.restrict_use[0].files,
            Some(vec!["src/handlers/**".to_string()])
        );
    }
}
