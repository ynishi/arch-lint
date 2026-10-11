//! TOML deserialization types (DTO layer).
//!
//! These types exist solely for serde deserialization.
//! They are converted to domain model types via the loader.

use serde::Deserialize;

/// Raw TOML representation of declarative rules.
///
/// Extends the base `Config` with `[[scopes]]`, `[[restrict-use]]`,
/// `[[require-use]]`, `[[deny-scope-dep]]`, `[[layering]]`, `[[deny-attr]]`,
/// `[[require-attr]]`, `[[naming]]`, `[[require-return]]`, and `[[acyclic]]`
/// sections.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
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

    /// Ordered layer stacks.
    #[serde(rename = "layering", default)]
    pub layering: Vec<LayeringDto>,

    /// Attribute-denial rules.
    #[serde(rename = "deny-attr", default)]
    pub deny_attr: Vec<DenyAttrDto>,

    /// Attribute-requirement rules.
    #[serde(rename = "require-attr", default)]
    pub require_attr: Vec<RequireAttrDto>,

    /// Identifier naming rules.
    #[serde(rename = "naming", default)]
    pub naming: Vec<NamingDto>,

    /// Return-type rules.
    #[serde(rename = "require-return", default)]
    pub require_return: Vec<RequireReturnDto>,

    /// Project-wide cycle checks over the scope graph.
    #[serde(rename = "acyclic", default)]
    pub acyclic: Vec<AcyclicDto>,
}

/// TOML representation of a named scope.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScopeDto {
    /// Scope name (e.g., "domain").
    pub name: String,
    /// Glob patterns for file paths.
    pub paths: Vec<String>,
    /// Glob patterns subtracted from `paths`.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// TOML representation of a restrict-use rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// Exceptions to `deny`, same pattern syntax.
    #[serde(default)]
    pub except: Vec<String>,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
    /// Also check inline qualified paths like `sqlx::query(...)` (default: true).
    #[serde(rename = "check-inline", default = "default_true")]
    pub check_inline: bool,
}

/// TOML representation of a require-use rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// Also check inline qualified paths like `crate::infra::Db::new()` (default: true).
    #[serde(rename = "check-inline", default = "default_true")]
    pub check_inline: bool,
}

/// TOML representation of an ordered layer stack.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayeringDto {
    /// Optional rule name (e.g., "clean-arch").
    #[serde(default)]
    pub name: Option<String>,
    /// Scope names, top layer first.
    pub order: Vec<String>,
    /// Allow only the immediately-next layer (default: false).
    #[serde(rename = "adjacent_only", default)]
    pub adjacent_only: bool,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
    /// Also check inline qualified paths (default: true).
    #[serde(rename = "check-inline", default = "default_true")]
    pub check_inline: bool,
}

/// TOML representation of an attribute-denial rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DenyAttrDto {
    /// Rule name (e.g., "no-orm-derives-in-domain").
    pub name: String,
    /// Named scope reference (mutually exclusive with `files`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns (mutually exclusive with `scope`).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Denied attribute patterns.
    pub attrs: Vec<String>,
    /// Item kinds to check (default: all).
    #[serde(default)]
    pub on: Option<Vec<String>>,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
}

/// TOML representation of an attribute-requirement rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequireAttrDto {
    /// Rule name (e.g., "instrument-handlers").
    pub name: String,
    /// Named scope reference (mutually exclusive with `files`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns (mutually exclusive with `scope`).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Required attribute patterns (at least one must be present).
    pub attrs: Vec<String>,
    /// Item kinds to check (default: all).
    #[serde(default)]
    pub on: Option<Vec<String>>,
    /// Visibility filter: "pub" (default) or "any".
    #[serde(default = "default_visibility_str")]
    pub visibility: String,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "warning").
    #[serde(default = "default_severity_warning_str")]
    pub severity: String,
}

/// TOML representation of an identifier naming rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamingDto {
    /// Rule name (e.g., "domain-ports").
    pub name: String,
    /// Named scope reference (mutually exclusive with `files`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns (mutually exclusive with `scope`).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Item kind to check.
    pub item: String,
    /// Regex the identifier must match.
    pub pattern: String,
    /// Visibility filter: "pub" (default) or "any".
    #[serde(default = "default_visibility_str")]
    pub visibility: String,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "warning").
    #[serde(default = "default_severity_warning_str")]
    pub severity: String,
}

/// TOML representation of a return-type rule.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequireReturnDto {
    /// Rule name (e.g., "handlers-return-app-result").
    pub name: String,
    /// Named scope reference (mutually exclusive with `files`).
    #[serde(default)]
    pub scope: Option<String>,
    /// Inline file patterns (mutually exclusive with `scope`).
    #[serde(default)]
    pub files: Option<Vec<String>>,
    /// Visibility filter: "pub" (default) or "any".
    #[serde(default = "default_visibility_str")]
    pub visibility: String,
    /// When set, only functions with this asyncness are checked.
    #[serde(rename = "async", default)]
    pub is_async: Option<bool>,
    /// Regex the normalized return type must match.
    pub pattern: String,
    /// Violation message.
    pub message: String,
    /// Document reference.
    #[serde(default)]
    pub doc: Option<String>,
    /// Severity (default: "error").
    #[serde(default = "default_severity_str")]
    pub severity: String,
}

/// TOML representation of a project-wide cycle check (`ALD009`).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcyclicDto {
    /// Optional rule name, used by `allow` directives when several
    /// `[[acyclic]]` blocks are declared (default rule name: "acyclic").
    #[serde(default)]
    pub name: Option<String>,
    /// Scopes the cycle check is restricted to (default: every declared
    /// scope).
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
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

fn default_visibility_str() -> String {
    "pub".to_string()
}

fn default_severity_warning_str() -> String {
    "warning".to_string()
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_in_rule_table_is_rejected() {
        // A typo such as `denyy` must not be silently ignored.
        let toml_str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx"
scope = "domain"
denyy = ["sqlx::*"]
deny = ["sqlx::*"]
message = "No DB."
"#;
        let err = match toml::from_str::<DeclarativeConfigDto>(toml_str) {
            Ok(_) => panic!("typo key must be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("denyy"), "{err}");

        // Unknown keys in every other rule table are rejected too.
        for (section, body) in [
            ("scopes", "name = \"d\"\npaths = [\"src/**\"]\nexclud = []"),
            ("layering", "order = [\"a\", \"b\"]\nmessage = \"m\"\nadjacent = true"),
            ("deny-attr", "name = \"n\"\nscope = \"d\"\nattrs = [\"x\"]\nmessage = \"m\"\nons = []"),
            ("naming", "name = \"n\"\nscope = \"d\"\nitem = \"trait\"\npattern = \".\"\nmessage = \"m\"\nvis = \"pub\""),
        ] {
            let doc = format!("[[{section}]]\n{body}\n");
            assert!(
                toml::from_str::<DeclarativeConfigDto>(&doc).is_err(),
                "unknown key in [[{section}]] must be rejected"
            );
        }

        // Keys that belong to the base `Config` (parsed separately from the
        // same document) are still ignored at the top level.
        let doc = "preset = \"strict\"\n[analyzer]\nroot = \"src\"\n";
        assert!(toml::from_str::<DeclarativeConfigDto>(doc).is_ok());
    }

    #[test]
    fn deserialize_empty() {
        let dto: DeclarativeConfigDto = toml::from_str("").unwrap();
        assert!(dto.scopes.is_empty());
        assert!(dto.restrict_use.is_empty());
        assert!(dto.deny_scope_dep.is_empty());
        assert!(dto.acyclic.is_empty());
    }

    #[test]
    fn deserialize_acyclic_config() {
        let toml_str = r#"
[[acyclic]]
scopes = ["domain", "app", "infra"]
severity = "error"

[[acyclic]]
name = "no-cycles-anywhere"
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert_eq!(dto.acyclic.len(), 2);
        assert_eq!(
            dto.acyclic[0].scopes,
            Some(vec![
                "domain".to_string(),
                "app".to_string(),
                "infra".to_string()
            ])
        );
        assert_eq!(dto.acyclic[0].severity, "error");
        assert_eq!(dto.acyclic[0].name, None);
        assert_eq!(dto.acyclic[1].name, Some("no-cycles-anywhere".to_string()));
        assert_eq!(dto.acyclic[1].scopes, None);
        assert_eq!(dto.acyclic[1].severity, "error", "default severity");
    }

    #[test]
    fn acyclic_rejects_unknown_key() {
        let doc = "[[acyclic]]\nscoops = [\"domain\"]\n";
        assert!(toml::from_str::<DeclarativeConfigDto>(doc).is_err());
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
        assert!(dto.restrict_use[0].check_inline);
        assert_eq!(dto.require_use.len(), 1);
        assert_eq!(dto.deny_scope_dep.len(), 1);
        assert_eq!(dto.deny_scope_dep[0].severity, "error");
        assert!(dto.deny_scope_dep[0].check_inline);
    }

    #[test]
    fn deserialize_check_inline_false() {
        let toml_str = r#"
[[restrict-use]]
name = "no-sqlx-in-domain"
files = ["src/domain/**"]
deny = ["sqlx::*"]
message = "Domain must be DB-agnostic."
check-inline = false

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
check-inline = false
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert!(!dto.restrict_use[0].check_inline);
        assert!(!dto.deny_scope_dep[0].check_inline);
    }

    #[test]
    fn deserialize_new_rule_kinds() {
        let toml_str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]
exclude = ["src/**/tests/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::**"]
except = ["sqlx::types::*"]
message = "msg"

[[layering]]
name = "clean-arch"
order = ["handler", "domain", "infra"]
message = "Layers may only depend downward."

[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"
attrs = ["derive(sqlx::FromRow)"]
on = ["struct", "enum"]
message = "msg"

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]
on = ["fn"]
severity = "warning"
message = "msg"

[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
message = "msg"

[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
async = true
pattern = "^Result<.*, AppError>$"
message = "msg"
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert_eq!(dto.scopes[0].exclude, vec!["src/**/tests/**".to_string()]);
        assert_eq!(
            dto.restrict_use[0].except,
            vec!["sqlx::types::*".to_string()]
        );
        assert_eq!(dto.layering.len(), 1);
        assert_eq!(dto.layering[0].order.len(), 3);
        assert!(!dto.layering[0].adjacent_only);
        assert!(dto.layering[0].check_inline);
        assert_eq!(dto.layering[0].severity, "error");
        assert_eq!(
            dto.deny_attr[0].on,
            Some(vec!["struct".into(), "enum".into()])
        );
        assert_eq!(dto.require_attr[0].visibility, "pub");
        assert_eq!(dto.require_attr[0].severity, "warning");
        assert_eq!(dto.naming[0].item, "trait");
        assert_eq!(dto.require_return[0].is_async, Some(true));
        assert_eq!(dto.require_return[0].visibility, "pub");
    }

    #[test]
    fn deserialize_defaults_for_optional_fields() {
        let toml_str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx"
scope = "domain"
deny = ["sqlx::*"]
message = "msg"

[[layering]]
order = ["a", "b"]
adjacent_only = true
check-inline = false
message = "msg"

[[deny-attr]]
name = "d"
files = ["src/**"]
attrs = ["serde::Serialize"]
message = "msg"
"#;
        let dto: DeclarativeConfigDto = toml::from_str(toml_str).unwrap();
        assert!(dto.scopes[0].exclude.is_empty());
        assert!(dto.restrict_use[0].except.is_empty());
        assert!(dto.layering[0].name.is_none());
        assert!(dto.layering[0].adjacent_only);
        assert!(!dto.layering[0].check_inline);
        assert!(dto.deny_attr[0].on.is_none());
        assert_eq!(dto.deny_attr[0].severity, "error");
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
