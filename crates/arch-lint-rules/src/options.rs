//! Typed option structs for the `[rules.<name>]` tables of `arch-lint.toml`.
//!
//! Every built-in rule has exactly one options type here, even the rules that
//! take no options. The types are the single source of truth for the accepted
//! keys: [`presets::build_rule`](crate::Preset) deserializes into them, so an
//! unrecognized key is a configuration error, and
//! [`rule_option_schemas`] renders the same types as JSON Schema.
//!
//! Every field is an `Option<T>`. Unset means "fall through to the default",
//! and some defaults depend on the active preset — the per-field docs spell
//! that out.

use crate::RuntimeMode;
use arch_lint_core::RuleConfig;

/// A `[rules.<name>]` table carries an option the rule does not accept, or a
/// value of the wrong type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid options for rule `{rule}`: {message}")]
pub struct RuleOptionsError {
    /// Kebab-case name of the rule whose table failed to parse.
    pub rule: String,
    /// The underlying TOML deserialization message.
    pub message: String,
}

/// Deserializes the free-form half of a `[rules.<name>]` table into `T`.
///
/// `enabled`, `severity` and `exclude_files` are captured by [`RuleConfig`]
/// itself and never reach `T`.
///
/// # Errors
///
/// Returns [`RuleOptionsError`] when the table holds a key `T` does not
/// declare, or a value whose type does not match the field.
pub(crate) fn parse_options<T: serde::de::DeserializeOwned>(
    rule: &str,
    cfg: Option<&RuleConfig>,
) -> Result<T, RuleOptionsError> {
    let table: toml::map::Map<String, toml::Value> = cfg
        .map(|c| c.options.clone().into_iter().collect())
        .unwrap_or_default();

    toml::Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| RuleOptionsError {
            rule: rule.to_string(),
            message: e.to_string(),
        })
}

/// Options for `no-unwrap-expect` (AL001).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct NoUnwrapExpectOptions {
    /// Allow `.unwrap()` / `.expect()` inside `#[cfg(test)]` code.
    /// Default: `true` under `recommended` and `minimal`, `false` under `strict`.
    pub allow_in_tests: Option<bool>,

    /// Allow `.expect()` while still forbidding `.unwrap()`.
    /// Default: `true` under `minimal`, `false` under `recommended` and `strict`.
    pub allow_expect: Option<bool>,
}

/// Options for `no-sync-io` (AL002).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct NoSyncIoOptions {
    /// Extra call-path prefixes that count as non-blocking, added on top of the
    /// built-in `tokio::` and `async_std::`.
    /// Default: no extra prefixes, under every preset.
    pub allow_patterns: Option<Vec<String>>,
}

/// Options for `no-error-swallowing` (AL003). This rule takes no options.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct NoErrorSwallowingOptions {}

/// Options for `handler-complexity` (AL004).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct HandlerComplexityOptions {
    /// Maximum number of lines in a handler function body.
    /// Default: `150`, under every preset.
    pub max_handler_lines: Option<usize>,

    /// Maximum number of arms in a `match` expression.
    /// Default: `20`, under every preset.
    pub max_match_arms: Option<usize>,

    /// Maximum number of variants in an action-like enum.
    /// Default: `30`, under every preset.
    pub max_enum_variants: Option<usize>,
}

/// Options for `require-thiserror` (AL005).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct RequireThiserrorOptions {
    /// Extra type-name suffixes that mark a type as an error type and therefore
    /// require a `thiserror::Error` derive. Added on top of the built-in
    /// `Error` suffix; a type matches when its name ends with any suffix.
    /// Default: no extra suffixes, under every preset.
    pub patterns: Option<Vec<String>>,
}

/// Options for `require-tracing` (AL006). This rule takes no options.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct RequireTracingOptions {}

/// Options for `tracing-env-init` (AL007). This rule takes no options.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct TracingEnvInitOptions {}

/// Options for `async-trait-send-check` (AL009).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct AsyncTraitSendCheckOptions {
    /// Runtime the crate targets: `single-thread` or `multi-thread`.
    /// Default: `single-thread`, under every preset.
    pub runtime_mode: Option<RuntimeMode>,
}

/// Options for `prefer-from-over-into` (AL010). This rule takes no options.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct PreferFromOverIntoOptions {}

/// Options for `no-panic-in-lib` (AL011).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct NoPanicInLibOptions {
    /// Allow panic macros inside `#[cfg(test)]` code.
    /// Default: `true` under `recommended` and `minimal`, `false` under `strict`.
    pub allow_in_tests: Option<bool>,
}

/// Options for `require-doc-comments` (AL012).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct RequireDocCommentsOptions {
    /// Require a doc comment on public functions.
    /// Default: `true`, under every preset.
    pub require_fn_docs: Option<bool>,

    /// Require a doc comment on public structs.
    /// Default: `true`, under every preset.
    pub require_struct_docs: Option<bool>,

    /// Require a doc comment on public enums.
    /// Default: `true`, under every preset.
    pub require_enum_docs: Option<bool>,
}

/// Options for `no-silent-result-drop` (AL013).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields, default)]
pub struct NoSilentResultDropOptions {
    /// Allow dropped `Result`s inside `#[cfg(test)]` code.
    /// Default: `true` under `recommended` and `minimal`, `false` under `strict`.
    pub allow_in_tests: Option<bool>,

    /// Allow discarding the error through `.ok()`.
    /// Default: `false`, under every preset.
    pub allow_ok: Option<bool>,

    /// Allow discarding the value through `let _ = ...`.
    /// Default: `false`, under every preset.
    pub allow_let_underscore: Option<bool>,
}

/// One rule's identity together with the JSON Schema of its options table.
#[derive(Debug, Clone)]
pub struct RuleOptionSchema {
    /// Rule code (`AL001`).
    pub code: &'static str,
    /// Kebab-case rule name (`no-unwrap-expect`).
    pub name: &'static str,
    /// JSON Schema of the rule's `[rules.<name>]` options.
    pub schema: schemars::Schema,
}

/// Returns the options schema of every built-in rule, in code order.
#[must_use]
pub fn rule_option_schemas() -> Vec<RuleOptionSchema> {
    vec![
        RuleOptionSchema {
            code: "AL001",
            name: "no-unwrap-expect",
            schema: schemars::schema_for!(NoUnwrapExpectOptions),
        },
        RuleOptionSchema {
            code: "AL002",
            name: "no-sync-io",
            schema: schemars::schema_for!(NoSyncIoOptions),
        },
        RuleOptionSchema {
            code: "AL003",
            name: "no-error-swallowing",
            schema: schemars::schema_for!(NoErrorSwallowingOptions),
        },
        RuleOptionSchema {
            code: "AL004",
            name: "handler-complexity",
            schema: schemars::schema_for!(HandlerComplexityOptions),
        },
        RuleOptionSchema {
            code: "AL005",
            name: "require-thiserror",
            schema: schemars::schema_for!(RequireThiserrorOptions),
        },
        RuleOptionSchema {
            code: "AL006",
            name: "require-tracing",
            schema: schemars::schema_for!(RequireTracingOptions),
        },
        RuleOptionSchema {
            code: "AL007",
            name: "tracing-env-init",
            schema: schemars::schema_for!(TracingEnvInitOptions),
        },
        RuleOptionSchema {
            code: "AL009",
            name: "async-trait-send-check",
            schema: schemars::schema_for!(AsyncTraitSendCheckOptions),
        },
        RuleOptionSchema {
            code: "AL010",
            name: "prefer-from-over-into",
            schema: schemars::schema_for!(PreferFromOverIntoOptions),
        },
        RuleOptionSchema {
            code: "AL011",
            name: "no-panic-in-lib",
            schema: schemars::schema_for!(NoPanicInLibOptions),
        },
        RuleOptionSchema {
            code: "AL012",
            name: "require-doc-comments",
            schema: schemars::schema_for!(RequireDocCommentsOptions),
        },
        RuleOptionSchema {
            code: "AL013",
            name: "no-silent-result-drop",
            schema: schemars::schema_for!(NoSilentResultDropOptions),
        },
    ]
}

/// Returns the options schema of one rule, by kebab-case name or by code.
///
/// Returns `None` for names that do not match a built-in rule.
#[must_use]
pub fn rule_option_schema(name_or_code: &str) -> Option<schemars::Schema> {
    let name = crate::canonical_rule_name(name_or_code)?;
    rule_option_schemas()
        .into_iter()
        .find(|entry| entry.name == name)
        .map(|entry| entry.schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arch_lint_core::Config;

    fn rule_config(toml_str: &str) -> RuleConfig {
        let config = Config::parse(toml_str).expect("config parses");
        config
            .rules
            .values()
            .next()
            .cloned()
            .expect("one rule table")
    }

    #[test]
    fn no_unwrap_expect_options_round_trip() {
        let cfg =
            rule_config("[rules.no-unwrap-expect]\nallow_in_tests = false\nallow_expect = true\n");
        let opts: NoUnwrapExpectOptions =
            parse_options("no-unwrap-expect", Some(&cfg)).expect("parses");
        assert_eq!(
            opts,
            NoUnwrapExpectOptions {
                allow_in_tests: Some(false),
                allow_expect: Some(true),
            }
        );
    }

    #[test]
    fn no_sync_io_options_round_trip() {
        let cfg = rule_config("[rules.no-sync-io]\nallow_patterns = [\"my_io::\", \"x::\"]\n");
        let opts: NoSyncIoOptions = parse_options("no-sync-io", Some(&cfg)).expect("parses");
        assert_eq!(
            opts.allow_patterns,
            Some(vec!["my_io::".to_string(), "x::".to_string()])
        );
    }

    #[test]
    fn handler_complexity_options_round_trip() {
        let cfg = rule_config(
            "[rules.handler-complexity]\nmax_handler_lines = 7\nmax_match_arms = 3\nmax_enum_variants = 5\n",
        );
        let opts: HandlerComplexityOptions =
            parse_options("handler-complexity", Some(&cfg)).expect("parses");
        assert_eq!(
            opts,
            HandlerComplexityOptions {
                max_handler_lines: Some(7),
                max_match_arms: Some(3),
                max_enum_variants: Some(5),
            }
        );
    }

    #[test]
    fn require_thiserror_options_round_trip() {
        let cfg = rule_config("[rules.require-thiserror]\npatterns = [\"Failure\"]\n");
        let opts: RequireThiserrorOptions =
            parse_options("require-thiserror", Some(&cfg)).expect("parses");
        assert_eq!(opts.patterns, Some(vec!["Failure".to_string()]));
    }

    #[test]
    fn async_trait_send_check_options_round_trip() {
        for (value, expected) in [
            ("single-thread", RuntimeMode::SingleThread),
            ("multi-thread", RuntimeMode::MultiThread),
        ] {
            let cfg = rule_config(&format!(
                "[rules.async-trait-send-check]\nruntime_mode = \"{value}\"\n"
            ));
            let opts: AsyncTraitSendCheckOptions =
                parse_options("async-trait-send-check", Some(&cfg)).expect("parses");
            assert_eq!(opts.runtime_mode, Some(expected));
        }
    }

    #[test]
    fn no_panic_in_lib_options_round_trip() {
        let cfg = rule_config("[rules.no-panic-in-lib]\nallow_in_tests = false\n");
        let opts: NoPanicInLibOptions =
            parse_options("no-panic-in-lib", Some(&cfg)).expect("parses");
        assert_eq!(opts.allow_in_tests, Some(false));
    }

    #[test]
    fn require_doc_comments_options_round_trip() {
        let cfg = rule_config(
            "[rules.require-doc-comments]\nrequire_fn_docs = false\nrequire_struct_docs = true\nrequire_enum_docs = false\n",
        );
        let opts: RequireDocCommentsOptions =
            parse_options("require-doc-comments", Some(&cfg)).expect("parses");
        assert_eq!(
            opts,
            RequireDocCommentsOptions {
                require_fn_docs: Some(false),
                require_struct_docs: Some(true),
                require_enum_docs: Some(false),
            }
        );
    }

    #[test]
    fn no_silent_result_drop_options_round_trip() {
        let cfg = rule_config(
            "[rules.no-silent-result-drop]\nallow_in_tests = false\nallow_ok = true\nallow_let_underscore = true\n",
        );
        let opts: NoSilentResultDropOptions =
            parse_options("no-silent-result-drop", Some(&cfg)).expect("parses");
        assert_eq!(
            opts,
            NoSilentResultDropOptions {
                allow_in_tests: Some(false),
                allow_ok: Some(true),
                allow_let_underscore: Some(true),
            }
        );
    }

    #[test]
    fn absent_table_yields_all_defaults() {
        let opts: NoSilentResultDropOptions =
            parse_options("no-silent-result-drop", None).expect("parses");
        assert_eq!(opts, NoSilentResultDropOptions::default());
        assert!(opts.allow_in_tests.is_none());
    }

    #[test]
    fn rules_without_options_reject_any_key() {
        let cfg = rule_config("[rules.require-tracing]\nwhatever = 1\n");
        let err = parse_options::<RequireTracingOptions>("require-tracing", Some(&cfg))
            .expect_err("unknown key must be rejected");
        assert!(err.message.contains("whatever"), "{err}");
    }

    #[test]
    fn enabled_severity_and_exclude_files_are_not_options() {
        let cfg = rule_config(
            "[rules.no-unwrap-expect]\nenabled = true\nseverity = \"warning\"\nexclude_files = [\"src/cli/**\"]\nallow_in_tests = true\n",
        );
        let opts: NoUnwrapExpectOptions =
            parse_options("no-unwrap-expect", Some(&cfg)).expect("parses");
        assert_eq!(opts.allow_in_tests, Some(true));
    }

    #[test]
    fn wrong_value_type_is_rejected() {
        let cfg = rule_config("[rules.no-unwrap-expect]\nallow_in_tests = \"yes\"\n");
        let err = parse_options::<NoUnwrapExpectOptions>("no-unwrap-expect", Some(&cfg))
            .expect_err("string for a bool must be rejected");
        assert_eq!(err.rule, "no-unwrap-expect");
    }

    #[test]
    fn schema_registry_covers_every_rule() {
        let entries = rule_option_schemas();
        for rule in crate::all_rules() {
            let entry = entries
                .iter()
                .find(|e| e.name == rule.name())
                .unwrap_or_else(|| panic!("no options schema registered for {}", rule.name()));
            assert_eq!(entry.code, rule.code(), "code mismatch for {}", rule.name());
        }
        assert_eq!(entries.len(), crate::all_rules().len());
    }

    #[test]
    fn schema_lookup_accepts_names_and_codes() {
        let by_code = rule_option_schema("AL001").expect("AL001 has a schema");
        let by_name = rule_option_schema("no-unwrap-expect").expect("name has a schema");
        assert_eq!(by_code.to_value(), by_name.clone().to_value());
        assert!(rule_option_schema("AL008").is_none());
        assert!(rule_option_schema("nope").is_none());

        let json = by_name.to_value().to_string();
        assert!(json.contains("allow_in_tests"), "{json}");
        assert!(json.contains("allow_expect"), "{json}");
    }
}
