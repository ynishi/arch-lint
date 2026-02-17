//! DTO → Domain model conversion with validation.

use crate::types::Severity;

use super::config_dto::{
    DeclarativeConfigDto, RequireUseDto, RestrictUseDto, ScopeDepDto, ScopeDto,
};
use super::model::{
    DeclarativeConfig, GlobPattern, ModelError, RequireUse, RestrictUse, Scope, ScopeDep,
    ScopeName, ScopeRef, UsePattern,
};

/// Errors during DTO → Domain conversion.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// A field-level validation error.
    #[error("{context}: {source}")]
    Validation {
        /// Where the error occurred (e.g., "scopes[0].name").
        context: String,
        /// The underlying model error.
        source: ModelError,
    },

    /// The `scope` and `files` fields are both set or both missing.
    #[error("{rule_name}: exactly one of `scope` or `files` must be set")]
    AmbiguousScope {
        /// The rule that has the conflict.
        rule_name: String,
    },

    /// Unknown severity string.
    #[error("{context}: unknown severity `{value}`, expected: error, warning, info")]
    UnknownSeverity {
        /// Where the error occurred.
        context: String,
        /// The invalid value.
        value: String,
    },

    /// Cross-reference errors from aggregate root construction.
    #[error("configuration validation errors:\n{}", format_errors(.0))]
    CrossRef(Vec<ModelError>),
}

fn format_errors(errors: &[ModelError]) -> String {
    errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Converts a `DeclarativeConfigDto` to a validated `DeclarativeConfig`.
///
/// # Errors
///
/// Returns the first error encountered during conversion.
pub fn load(dto: DeclarativeConfigDto) -> Result<DeclarativeConfig, LoadError> {
    let scopes = dto
        .scopes
        .iter()
        .enumerate()
        .map(|(i, s)| convert_scope(s, i))
        .collect::<Result<Vec<_>, _>>()?;

    let restrict_uses = dto
        .restrict_use
        .into_iter()
        .map(convert_restrict_use)
        .collect::<Result<Vec<_>, _>>()?;

    let require_uses = dto
        .require_use
        .into_iter()
        .map(convert_require_use)
        .collect::<Result<Vec<_>, _>>()?;

    let scope_deps = dto
        .deny_scope_dep
        .into_iter()
        .enumerate()
        .map(|(i, d)| convert_scope_dep(d, i))
        .collect::<Result<Vec<_>, _>>()?;

    DeclarativeConfig::new(scopes, restrict_uses, require_uses, scope_deps)
        .map_err(LoadError::CrossRef)
}

fn convert_scope(dto: &ScopeDto, index: usize) -> Result<Scope, LoadError> {
    let ctx = format!("scopes[{index}]");
    let name = ScopeName::new(&dto.name).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.name"),
        source: e,
    })?;

    let patterns = dto
        .paths
        .iter()
        .enumerate()
        .map(|(j, p)| {
            GlobPattern::new(p).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.paths[{j}]"),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Scope::new(name, patterns))
}

fn resolve_scope_ref(
    scope: Option<String>,
    files: Option<Vec<String>>,
    rule_name: &str,
) -> Result<ScopeRef, LoadError> {
    match (scope, files) {
        (Some(name), None) => {
            let scope_name = ScopeName::new(&name).map_err(|e| LoadError::Validation {
                context: format!("{rule_name}.scope"),
                source: e,
            })?;
            Ok(ScopeRef::Named(scope_name))
        }
        (None, Some(patterns)) => {
            let globs = patterns
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    GlobPattern::new(p).map_err(|e| LoadError::Validation {
                        context: format!("{rule_name}.files[{i}]"),
                        source: e,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ScopeRef::Inline(globs))
        }
        _ => Err(LoadError::AmbiguousScope {
            rule_name: rule_name.to_string(),
        }),
    }
}

fn convert_restrict_use(dto: RestrictUseDto) -> Result<RestrictUse, LoadError> {
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;

    let deny = dto
        .deny
        .iter()
        .enumerate()
        .map(|(i, p)| {
            UsePattern::new(p).map_err(|e| LoadError::Validation {
                context: format!("restrict-use '{}' deny[{i}]", dto.name),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let severity = parse_severity(&dto.severity, &format!("restrict-use '{}'", dto.name))?;

    Ok(RestrictUse::new(
        dto.name,
        scope,
        deny,
        dto.message,
        dto.doc,
        severity,
    ))
}

fn convert_require_use(dto: RequireUseDto) -> Result<RequireUse, LoadError> {
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;
    let severity = parse_severity(&dto.severity, &format!("require-use '{}'", dto.name))?;

    Ok(RequireUse::new(
        dto.name,
        scope,
        dto.prefer,
        dto.over,
        dto.message,
        dto.doc,
        severity,
    ))
}

fn convert_scope_dep(dto: ScopeDepDto, index: usize) -> Result<ScopeDep, LoadError> {
    let ctx = format!("deny-scope-dep[{index}]");
    let from = ScopeName::new(&dto.from).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.from"),
        source: e,
    })?;

    let to = dto
        .to
        .iter()
        .enumerate()
        .map(|(i, name)| {
            ScopeName::new(name).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.to[{i}]"),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(ScopeDep::new(from, to, dto.message, dto.doc, severity))
}

fn parse_severity(value: &str, context: &str) -> Result<Severity, LoadError> {
    match value {
        "error" => Ok(Severity::Error),
        "warning" => Ok(Severity::Warning),
        "info" => Ok(Severity::Info),
        _ => Err(LoadError::UnknownSeverity {
            context: context.to_string(),
            value: value.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_load(toml_str: &str) -> Result<DeclarativeConfig, LoadError> {
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        load(dto)
    }

    // -- Happy path --

    #[test]
    fn load_empty_config() {
        let config = parse_and_load("").unwrap();
        assert!(config.is_empty());
    }

    #[test]
    fn load_full_config() {
        let config = parse_and_load(
            r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::*"]
message = "No DB in domain."
doc = "ARCH.md L42"
severity = "error"

[[require-use]]
name = "require-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing."

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
"#,
        )
        .unwrap();

        assert_eq!(config.scopes().len(), 2);
        assert_eq!(config.restrict_uses().len(), 1);
        assert_eq!(config.require_uses().len(), 1);
        assert_eq!(config.scope_deps().len(), 1);
    }

    #[test]
    fn load_inline_files() {
        let config = parse_and_load(
            r#"
[[restrict-use]]
name = "no-sqlx"
files = ["src/handlers/**"]
deny = ["sqlx::*"]
message = "No direct DB."
"#,
        )
        .unwrap();

        assert_eq!(config.restrict_uses().len(), 1);
    }

    // -- Error cases --

    #[test]
    fn load_rejects_both_scope_and_files() {
        let result = parse_and_load(
            r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "bad"
scope = "domain"
files = ["src/**"]
deny = ["sqlx::*"]
message = "conflict"
"#,
        );
        assert!(matches!(result, Err(LoadError::AmbiguousScope { .. })));
    }

    #[test]
    fn load_rejects_neither_scope_nor_files() {
        let result = parse_and_load(
            r#"
[[restrict-use]]
name = "bad"
deny = ["sqlx::*"]
message = "missing scope"
"#,
        );
        assert!(matches!(result, Err(LoadError::AmbiguousScope { .. })));
    }

    #[test]
    fn load_rejects_invalid_scope_name() {
        let result = parse_and_load(
            r#"
[[scopes]]
name = "INVALID"
paths = ["src/**"]
"#,
        );
        assert!(matches!(result, Err(LoadError::Validation { .. })));
    }

    #[test]
    fn load_rejects_unknown_severity() {
        let result = parse_and_load(
            r#"
[[restrict-use]]
name = "bad"
files = ["src/**"]
deny = ["sqlx::*"]
message = "msg"
severity = "critical"
"#,
        );
        assert!(matches!(result, Err(LoadError::UnknownSeverity { .. })));
    }

    #[test]
    fn load_rejects_unknown_scope_ref() {
        let result = parse_and_load(
            r#"
[[restrict-use]]
name = "bad"
scope = "nonexistent"
deny = ["sqlx::*"]
message = "msg"
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }
}
