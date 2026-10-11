//! DTO → Domain model conversion with validation.

use crate::types::Severity;

use super::config_dto::{
    AcyclicDto, AllowDepDto, DeclarativeConfigDto, DenyAttrDto, IndependentDto, LayeringDto,
    NamingDto, RequireAttrDto, RequireReturnDto, RequireUseDto, RestrictUseDto, ScopeDepDto,
    ScopeDto,
};
use super::model::{
    Acyclic, AllowDep, AttrPattern, DeclarativeConfig, DenyAttr, GlobPattern, Independent,
    ItemKind, Layering, ModelError, Naming, RegexPattern, RequireAttr, RequireReturn, RequireUse,
    RestrictUse, Scope, ScopeDep, ScopeName, ScopeRef, UsePattern, VisibilityFilter,
};

/// Errors during DTO → Domain conversion.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// A field-level validation error.
    #[error("{context}: {source}")]
    Validation {
        /// Where the error occurred (e.g., `scopes[0].name`).
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

    let layerings = dto
        .layering
        .into_iter()
        .enumerate()
        .map(|(i, l)| convert_layering(l, i))
        .collect::<Result<Vec<_>, _>>()?;

    let deny_attrs = dto
        .deny_attr
        .into_iter()
        .map(convert_deny_attr)
        .collect::<Result<Vec<_>, _>>()?;

    let require_attrs = dto
        .require_attr
        .into_iter()
        .map(convert_require_attr)
        .collect::<Result<Vec<_>, _>>()?;

    let namings = dto
        .naming
        .into_iter()
        .map(convert_naming)
        .collect::<Result<Vec<_>, _>>()?;

    let require_returns = dto
        .require_return
        .into_iter()
        .map(convert_require_return)
        .collect::<Result<Vec<_>, _>>()?;

    let acyclics = dto
        .acyclic
        .into_iter()
        .enumerate()
        .map(|(i, a)| convert_acyclic(a, i))
        .collect::<Result<Vec<_>, _>>()?;

    let independents = dto
        .independent
        .into_iter()
        .enumerate()
        .map(|(i, ind)| convert_independent(ind, i))
        .collect::<Result<Vec<_>, _>>()?;

    let allow_deps = dto
        .allow_dep
        .into_iter()
        .enumerate()
        .map(|(i, a)| convert_allow_dep(a, i))
        .collect::<Result<Vec<_>, _>>()?;

    DeclarativeConfig::builder()
        .scopes(scopes)
        .restrict_uses(restrict_uses)
        .require_uses(require_uses)
        .scope_deps(scope_deps)
        .layerings(layerings)
        .deny_attrs(deny_attrs)
        .require_attrs(require_attrs)
        .namings(namings)
        .require_returns(require_returns)
        .acyclics(acyclics)
        .independents(independents)
        .allow_deps(allow_deps)
        .build()
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

    let excludes = dto
        .exclude
        .iter()
        .enumerate()
        .map(|(j, p)| {
            GlobPattern::new(p).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.exclude[{j}]"),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Scope::new(name, patterns).with_excludes(excludes))
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

    let except = dto
        .except
        .iter()
        .enumerate()
        .map(|(i, p)| {
            UsePattern::new(p).map_err(|e| LoadError::Validation {
                context: format!("restrict-use '{}' except[{i}]", dto.name),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let severity = parse_severity(&dto.severity, &format!("restrict-use '{}'", dto.name))?;

    Ok(
        RestrictUse::new(dto.name, scope, deny, dto.message, dto.doc, severity)
            .with_check_inline(dto.check_inline)
            .with_except(except),
    )
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

    Ok(
        ScopeDep::new(dto.name, from, to, dto.message, dto.doc, severity)
            .with_check_inline(dto.check_inline),
    )
}

fn convert_layering(dto: LayeringDto, index: usize) -> Result<Layering, LoadError> {
    let ctx = dto.name.clone().map_or_else(
        || format!("layering[{index}]"),
        |n| format!("layering '{n}'"),
    );

    let order = dto
        .order
        .iter()
        .enumerate()
        .map(|(i, name)| {
            ScopeName::new(name).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.order[{i}]"),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let severity = parse_severity(&dto.severity, &ctx)?;

    let layering = Layering::new(dto.name, order, dto.message, dto.doc, severity).map_err(|e| {
        LoadError::Validation {
            context: format!("{ctx}.order"),
            source: e,
        }
    })?;

    Ok(layering
        .with_adjacent_only(dto.adjacent_only)
        .with_check_inline(dto.check_inline))
}

/// Parses the optional `on` list, defaulting to every item kind.
fn parse_item_kinds(on: Option<Vec<String>>, ctx: &str) -> Result<Vec<ItemKind>, LoadError> {
    match on {
        None => Ok(ItemKind::ALL.to_vec()),
        Some(values) => values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                ItemKind::parse(v).map_err(|e| LoadError::Validation {
                    context: format!("{ctx}.on[{i}]"),
                    source: e,
                })
            })
            .collect(),
    }
}

fn parse_attr_patterns(attrs: &[String], ctx: &str) -> Result<Vec<AttrPattern>, LoadError> {
    attrs
        .iter()
        .enumerate()
        .map(|(i, a)| {
            AttrPattern::new(a).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.attrs[{i}]"),
                source: e,
            })
        })
        .collect()
}

fn parse_visibility(value: &str, ctx: &str) -> Result<VisibilityFilter, LoadError> {
    VisibilityFilter::parse(value).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.visibility"),
        source: e,
    })
}

fn parse_regex(pattern: &str, ctx: &str) -> Result<RegexPattern, LoadError> {
    RegexPattern::new(pattern).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.pattern"),
        source: e,
    })
}

fn convert_deny_attr(dto: DenyAttrDto) -> Result<DenyAttr, LoadError> {
    let ctx = format!("deny-attr '{}'", dto.name);
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;
    let attrs = parse_attr_patterns(&dto.attrs, &ctx)?;
    let on = parse_item_kinds(dto.on, &ctx)?;
    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(DenyAttr::new(
        dto.name,
        scope,
        attrs,
        on,
        dto.message,
        dto.doc,
        severity,
    ))
}

fn convert_require_attr(dto: RequireAttrDto) -> Result<RequireAttr, LoadError> {
    let ctx = format!("require-attr '{}'", dto.name);
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;
    let attrs = parse_attr_patterns(&dto.attrs, &ctx)?;
    let on = parse_item_kinds(dto.on, &ctx)?;
    let visibility = parse_visibility(&dto.visibility, &ctx)?;
    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(
        RequireAttr::new(dto.name, scope, attrs, on, dto.message, dto.doc, severity)
            .with_visibility(visibility),
    )
}

fn convert_naming(dto: NamingDto) -> Result<Naming, LoadError> {
    let ctx = format!("naming '{}'", dto.name);
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;
    let item = ItemKind::parse_named(&dto.item).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.item"),
        source: e,
    })?;
    let pattern = parse_regex(&dto.pattern, &ctx)?;
    let visibility = parse_visibility(&dto.visibility, &ctx)?;
    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(Naming::new(
        dto.name,
        scope,
        item,
        pattern,
        dto.message,
        dto.doc,
        severity,
    )
    .with_visibility(visibility))
}

fn convert_require_return(dto: RequireReturnDto) -> Result<RequireReturn, LoadError> {
    let ctx = format!("require-return '{}'", dto.name);
    let scope = resolve_scope_ref(dto.scope, dto.files, &dto.name)?;
    let visibility = parse_visibility(&dto.visibility, &ctx)?;
    let pattern = parse_regex(&dto.pattern, &ctx)?;
    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(
        RequireReturn::new(dto.name, scope, pattern, dto.message, dto.doc, severity)
            .with_visibility(visibility)
            .with_asyncness(dto.is_async),
    )
}

fn convert_acyclic(dto: AcyclicDto, index: usize) -> Result<Acyclic, LoadError> {
    let ctx = dto
        .name
        .clone()
        .map_or_else(|| format!("acyclic[{index}]"), |n| format!("acyclic '{n}'"));

    let scopes = match dto.scopes {
        None => None,
        Some(names) => {
            let scopes = names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    ScopeName::new(name).map_err(|e| LoadError::Validation {
                        context: format!("{ctx}.scopes[{i}]"),
                        source: e,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Some(scopes)
        }
    };

    let severity = parse_severity(&dto.severity, &ctx)?;

    Ok(Acyclic::new(dto.name, scopes, dto.doc, severity))
}

fn convert_independent(dto: IndependentDto, index: usize) -> Result<Independent, LoadError> {
    let ctx = dto.name.clone().map_or_else(
        || format!("independent[{index}]"),
        |n| format!("independent '{n}'"),
    );

    let scopes = dto
        .scopes
        .iter()
        .enumerate()
        .map(|(i, name)| {
            ScopeName::new(name).map_err(|e| LoadError::Validation {
                context: format!("{ctx}.scopes[{i}]"),
                source: e,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let severity = parse_severity(&dto.severity, &ctx)?;

    Independent::new(dto.name, scopes, dto.doc, severity).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.scopes"),
        source: e,
    })
}

fn convert_allow_dep(dto: AllowDepDto, index: usize) -> Result<AllowDep, LoadError> {
    let ctx = dto.name.clone().map_or_else(
        || format!("allow-dep[{index}]"),
        |n| format!("allow-dep '{n}'"),
    );

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

    AllowDep::new(dto.name, from, to, dto.doc, severity).map_err(|e| LoadError::Validation {
        context: format!("{ctx}.to"),
        source: e,
    })
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

    // -- New rule kinds: happy path --

    const SCOPES: &str = r#"
[[scopes]]
name = "handler"
paths = ["src/handler/**"]

[[scopes]]
name = "domain"
paths = ["src/domain/**"]
exclude = ["src/domain/tests/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]
"#;

    fn load_with_scopes(extra: &str) -> Result<DeclarativeConfig, LoadError> {
        parse_and_load(&format!("{SCOPES}{extra}"))
    }

    #[test]
    fn load_all_new_rule_kinds() {
        let config = load_with_scopes(
            r#"
[[layering]]
name = "clean-arch"
order = ["handler", "domain", "infra"]
adjacent_only = true
message = "Layers may only depend downward."

[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"
attrs = ["derive(sqlx::FromRow)", "serde::Serialize"]
on = ["struct", "enum"]
message = "Domain types must stay ORM-agnostic."

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]
on = ["fn"]
severity = "warning"
message = "Handlers must be instrumented."

[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
severity = "warning"
message = "Domain traits are ports."

[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
async = true
pattern = "^Result<.*,\\s*AppError>$"
message = "Handlers must return Result<_, AppError>."
"#,
        )
        .unwrap();

        assert_eq!(config.layerings().len(), 1);
        assert!(config.layerings()[0].adjacent_only());
        assert_eq!(config.deny_attrs().len(), 1);
        assert_eq!(config.deny_attrs()[0].on().len(), 2);
        assert_eq!(config.require_attrs().len(), 1);
        assert_eq!(config.require_attrs()[0].severity(), Severity::Warning);
        assert_eq!(config.namings().len(), 1);
        assert_eq!(config.require_returns().len(), 1);
        assert_eq!(config.require_returns()[0].asyncness(), Some(true));
        assert!(!config.is_empty());
    }

    #[test]
    fn load_defaults_on_to_every_item_kind() {
        let config = load_with_scopes(
            r#"
[[deny-attr]]
name = "no-serde"
scope = "domain"
attrs = ["serde::Serialize"]
message = "msg"
"#,
        )
        .unwrap();
        assert_eq!(config.deny_attrs()[0].on().len(), 10);
    }

    #[test]
    fn load_scope_exclude_applies() {
        let config = load_with_scopes("").unwrap();
        let domain = config.scope(&ScopeName::new("domain").unwrap()).unwrap();
        assert!(domain.contains(std::path::Path::new("src/domain/entity.rs")));
        assert!(!domain.contains(std::path::Path::new("src/domain/tests/it.rs")));
    }

    // -- New rule kinds: validation errors --

    #[test]
    fn load_rejects_unknown_layer_scope() {
        let result = load_with_scopes(
            r#"
[[layering]]
order = ["handler", "nonexistent"]
message = "msg"
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }

    #[test]
    fn load_rejects_short_layer_order() {
        let result = load_with_scopes(
            r#"
[[layering]]
order = ["handler"]
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 2 scopes"), "{err}");
    }

    #[test]
    fn load_rejects_empty_layer_order() {
        let result = load_with_scopes(
            r#"
[[layering]]
order = []
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 2 scopes"), "{err}");
    }

    #[test]
    fn load_rejects_duplicate_layers() {
        let result = load_with_scopes(
            r#"
[[layering]]
name = "dup"
order = ["handler", "domain", "handler"]
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("more than once"), "{err}");
    }

    #[test]
    fn load_rejects_unknown_layering_severity() {
        let result = load_with_scopes(
            r#"
[[layering]]
order = ["handler", "domain"]
severity = "critical"
message = "msg"
"#,
        );
        assert!(matches!(result, Err(LoadError::UnknownSeverity { .. })));
    }

    #[test]
    fn load_rejects_invalid_item_kind_in_on() {
        let result = load_with_scopes(
            r#"
[[deny-attr]]
name = "bad"
scope = "domain"
attrs = ["serde::Serialize"]
on = ["struct", "macro"]
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown item kind `macro`"), "{err}");
        assert!(err.contains("deny-attr 'bad'.on[1]"), "{err}");
    }

    #[test]
    fn load_rejects_invalid_naming_item() {
        let result = load_with_scopes(
            r#"
[[naming]]
name = "bad"
scope = "domain"
item = "impl"
pattern = "^X$"
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown item kind `impl`"), "{err}");
    }

    #[test]
    fn load_rejects_invalid_visibility() {
        let result = load_with_scopes(
            r#"
[[require-attr]]
name = "bad"
scope = "handler"
attrs = ["tracing::instrument"]
visibility = "private"
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown visibility `private`"), "{err}");
    }

    #[test]
    fn load_rejects_invalid_naming_regex() {
        let result = load_with_scopes(
            r#"
[[naming]]
name = "bad"
scope = "domain"
item = "struct"
pattern = "([a-z"
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid regex"), "{err}");
        assert!(err.contains("naming 'bad'.pattern"), "{err}");
    }

    #[test]
    fn load_rejects_invalid_require_return_regex() {
        let result = load_with_scopes(
            r#"
[[require-return]]
name = "bad"
scope = "handler"
pattern = "Result<(.*"
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid regex"), "{err}");
    }

    #[test]
    fn load_rejects_empty_attr_pattern() {
        let result = load_with_scopes(
            r#"
[[deny-attr]]
name = "bad"
scope = "domain"
attrs = [""]
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("attribute pattern must not be empty"), "{err}");
    }

    #[test]
    fn load_rejects_empty_derive_attr_pattern() {
        let result = load_with_scopes(
            r#"
[[deny-attr]]
name = "bad"
scope = "domain"
attrs = ["derive()"]
message = "msg"
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid attribute pattern"), "{err}");
    }

    #[test]
    fn load_rejects_unknown_scope_for_new_kinds() {
        for section in [
            "[[deny-attr]]\nname = \"x\"\nscope = \"nope\"\nattrs = [\"serde::Serialize\"]\nmessage = \"m\"\n",
            "[[require-attr]]\nname = \"x\"\nscope = \"nope\"\nattrs = [\"tracing::instrument\"]\nmessage = \"m\"\n",
            "[[naming]]\nname = \"x\"\nscope = \"nope\"\nitem = \"struct\"\npattern = \"^A$\"\nmessage = \"m\"\n",
            "[[require-return]]\nname = \"x\"\nscope = \"nope\"\npattern = \"^A$\"\nmessage = \"m\"\n",
        ] {
            let result = load_with_scopes(section);
            assert!(
                matches!(result, Err(LoadError::CrossRef(_))),
                "expected CrossRef for {section}"
            );
        }
    }

    #[test]
    fn load_rejects_ambiguous_scope_for_new_kinds() {
        let result = load_with_scopes(
            r#"
[[naming]]
name = "bad"
scope = "domain"
files = ["src/**"]
item = "struct"
pattern = "^A$"
message = "msg"
"#,
        );
        assert!(matches!(result, Err(LoadError::AmbiguousScope { .. })));
    }

    #[test]
    fn load_rejects_invalid_exclude_glob() {
        let result = parse_and_load(
            r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]
exclude = [""]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("scopes[0].exclude[0]"), "{err}");
    }

    // -- [[acyclic]] --

    #[test]
    fn load_acyclic_with_scopes() {
        let config = load_with_scopes(
            r#"
[[acyclic]]
scopes = ["domain", "infra"]
doc = "ARCHITECTURE.md L10"
severity = "error"
"#,
        )
        .unwrap();

        assert_eq!(config.acyclics().len(), 1);
        let rule = &config.acyclics()[0];
        assert_eq!(rule.display_name(), "acyclic");
        let scopes: Vec<&str> = rule
            .scopes()
            .unwrap()
            .iter()
            .map(crate::declarative::model::ScopeName::as_str)
            .collect();
        assert_eq!(scopes, ["domain", "infra"]);
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L10"));
    }

    #[test]
    fn load_acyclic_without_scopes_means_every_scope() {
        let config = load_with_scopes("[[acyclic]]\n").unwrap();
        assert_eq!(config.acyclics().len(), 1);
        assert!(config.acyclics()[0].scopes().is_none());
    }

    #[test]
    fn load_acyclic_rejects_unknown_scope() {
        let result = load_with_scopes(
            r#"
[[acyclic]]
scopes = ["domain", "nope"]
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }

    #[test]
    fn load_acyclic_rejects_unknown_severity() {
        let result = load_with_scopes(
            r#"
[[acyclic]]
severity = "critical"
"#,
        );
        assert!(matches!(result, Err(LoadError::UnknownSeverity { .. })));
    }

    // -- [[independent]] --

    #[test]
    fn load_independent_with_scopes() {
        let config = load_with_scopes(
            r#"
[[independent]]
scopes = ["domain", "infra"]
doc = "ARCHITECTURE.md L20"
severity = "error"
"#,
        )
        .unwrap();

        assert_eq!(config.independents().len(), 1);
        let rule = &config.independents()[0];
        assert_eq!(rule.display_name(), "independent");
        let scopes: Vec<&str> = rule
            .scopes()
            .iter()
            .map(crate::declarative::model::ScopeName::as_str)
            .collect();
        assert_eq!(scopes, ["domain", "infra"]);
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L20"));
    }

    #[test]
    fn load_independent_rejects_unknown_scope() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = ["domain", "nope"]
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }

    #[test]
    fn load_independent_rejects_unknown_severity() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = ["domain", "infra"]
severity = "critical"
"#,
        );
        assert!(matches!(result, Err(LoadError::UnknownSeverity { .. })));
    }

    #[test]
    fn load_independent_rejects_a_single_scope() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = ["domain"]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 2 entries"), "{err}");
    }

    #[test]
    fn load_independent_rejects_no_scopes() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = []
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 2 entries"), "{err}");
    }

    #[test]
    fn load_independent_rejects_an_immediate_duplicate_scope() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = ["domain", "domain"]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("more than once"), "{err}");
        assert!(err.contains("domain"), "{err}");
    }

    #[test]
    fn load_independent_rejects_a_non_adjacent_duplicate_scope() {
        let result = load_with_scopes(
            r#"
[[independent]]
scopes = ["handler", "domain", "handler"]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("more than once"), "{err}");
        assert!(err.contains("handler"), "{err}");
    }

    // -- [[allow-dep]] --

    #[test]
    fn load_allow_dep_with_scopes() {
        let config = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = ["infra"]
doc = "ARCHITECTURE.md L30"
severity = "error"
"#,
        )
        .unwrap();

        assert_eq!(config.allow_deps().len(), 1);
        let rule = &config.allow_deps()[0];
        assert_eq!(rule.display_name(), "allow-dep:domain");
        assert_eq!(rule.from_scope().as_str(), "domain");
        let to: Vec<&str> = rule.to().iter().map(ScopeName::as_str).collect();
        assert_eq!(to, ["infra"]);
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L30"));
    }

    #[test]
    fn load_allow_dep_with_an_empty_to() {
        let config = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = []
"#,
        )
        .unwrap();

        assert_eq!(config.allow_deps().len(), 1);
        assert!(config.allow_deps()[0].to().is_empty());
    }

    #[test]
    fn load_allow_dep_rejects_unknown_from() {
        let result = load_with_scopes(
            r#"
[[allow-dep]]
from = "nope"
to = ["infra"]
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }

    #[test]
    fn load_allow_dep_rejects_unknown_to_entry() {
        let result = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = ["nope"]
"#,
        );
        assert!(matches!(result, Err(LoadError::CrossRef(_))));
    }

    #[test]
    fn load_allow_dep_rejects_unknown_severity() {
        let result = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = ["infra"]
severity = "critical"
"#,
        );
        assert!(matches!(result, Err(LoadError::UnknownSeverity { .. })));
    }

    #[test]
    fn load_allow_dep_rejects_to_containing_from() {
        let result = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = ["infra", "domain"]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("must not contain"), "{err}");
        assert!(err.contains("domain"), "{err}");
    }

    #[test]
    fn load_allow_dep_rejects_a_duplicate_to_entry() {
        let result = load_with_scopes(
            r#"
[[allow-dep]]
from = "domain"
to = ["infra", "infra"]
"#,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("more than once"), "{err}");
        assert!(err.contains("infra"), "{err}");
    }

    #[test]
    fn load_two_allow_dep_blocks_for_the_same_from_are_both_kept() {
        let config = load_with_scopes(
            r#"
[[allow-dep]]
name = "domain-allow-handler"
from = "domain"
to = ["handler"]

[[allow-dep]]
name = "domain-allow-infra"
from = "domain"
to = ["infra"]
"#,
        )
        .unwrap();
        assert_eq!(config.allow_deps().len(), 2);
    }
}
