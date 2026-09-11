//! JSON Schema for `arch-lint.toml`.
//!
//! [`config_json_schema`] assembles one draft 2020-12 schema describing the
//! whole configuration file:
//!
//! * the top-level keys and `[analyzer]` table of [`Config`];
//! * one entry per built-in rule under `rules`, combining the common
//!   `enabled` / `severity` / `exclude_files` keys of [`RuleConfig`] with that
//!   rule's own option keys, closed against unknown keys;
//! * the declarative sections (`[[scopes]]`, `[[restrict-use]]`, ...) of
//!   [`DeclarativeConfigDto`], which are parsed from the same document.
//!
//! The `rules` map itself stays open so that tables for custom rules — rules
//! registered through the builder API, which arch-lint cannot know the option
//! keys of — still validate.
//!
//! ```
//! let schema = arch_lint::schema::config_json_schema();
//! assert_eq!(schema["title"], "arch-lint.toml");
//! assert!(schema["properties"]["rules"]["properties"]["no-unwrap-expect"].is_object());
//! ```

use arch_lint_core::declarative::config_dto::DeclarativeConfigDto;
use arch_lint_core::{Config, RuleConfig};
use arch_lint_rules::{all_rules, rule_option_schemas};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Stable identifier of the published schema.
pub const SCHEMA_ID: &str = "https://github.com/ynishi/arch-lint/schema/arch-lint.schema.json";

/// Title carried by the generated schema.
pub const SCHEMA_TITLE: &str = "arch-lint.toml";

/// Keys every `[rules.<name>]` table accepts, whatever the rule.
pub const COMMON_RULE_KEYS: &[&str] = &["enabled", "severity", "exclude_files"];

/// Prefix of a local JSON Schema reference.
const DEFS_PREFIX: &str = "#/$defs/";

/// Builds the JSON Schema of a complete `arch-lint.toml` document.
///
/// The result is a draft 2020-12 schema, as emitted by `schemars`. Write it
/// next to the configuration file and point an editor at it to get completion
/// and validation for every rule's option keys.
#[must_use]
pub fn config_json_schema() -> Value {
    let mut root = schemars::schema_for!(Config).to_value();
    let mut defs = take_defs(&mut root);

    let rules_property = build_rules_property(&mut defs);
    insert_property(&mut root, "rules".to_string(), rules_property);

    merge_declarative_sections(&mut root, &mut defs);

    if let Some(object) = root.as_object_mut() {
        object.insert("$id".to_string(), Value::String(SCHEMA_ID.to_string()));
        object.insert("title".to_string(), Value::String(SCHEMA_TITLE.to_string()));
        if defs.is_empty() {
            object.remove("$defs");
        } else {
            object.insert("$defs".to_string(), Value::Object(defs));
        }
    }

    root
}

/// Returns the schema of the keys every `[rules.<name>]` table accepts —
/// `enabled`, `severity` and `exclude_files` — as a self-contained object
/// schema carrying the `$defs` its properties reference.
///
/// Rule-specific keys live in `rule_option_schemas` of `arch-lint-rules`;
/// this is the half they all share.
#[must_use]
pub fn common_rule_schema() -> Value {
    let mut rule_config = schemars::schema_for!(RuleConfig).to_value();
    let defs = take_defs(&mut rule_config);
    let properties = common_rule_properties(&rule_config);

    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    if !defs.is_empty() {
        schema.insert("$defs".to_string(), Value::Object(defs));
    }
    Value::Object(schema)
}

/// Builds the schema of the `rules` map: one closed object per built-in rule,
/// with the map itself left open for custom rules.
fn build_rules_property(defs: &mut Map<String, Value>) -> Value {
    let mut rule_config = schemars::schema_for!(RuleConfig).to_value();
    let renames = merge_defs(defs, take_defs(&mut rule_config));
    rewrite_refs(&mut rule_config, &renames);
    let common = common_rule_properties(&rule_config);

    let descriptions: HashMap<String, String> = all_rules()
        .iter()
        .map(|rule| {
            (
                rule.name().to_string(),
                format!("{} — {}", rule.code(), rule.description()),
            )
        })
        .collect();

    let mut per_rule = Map::new();
    for entry in rule_option_schemas() {
        let mut options = entry.schema.to_value();
        let renames = merge_defs(defs, take_defs(&mut options));
        rewrite_refs(&mut options, &renames);

        let mut properties = common.clone();
        if let Some(option_properties) = options.get("properties").and_then(Value::as_object) {
            for (key, schema) in option_properties {
                properties.insert(key.clone(), schema.clone());
            }
        }

        let description = descriptions
            .get(entry.name)
            .cloned()
            .unwrap_or_else(|| entry.code.to_string());

        let mut schema = Map::new();
        schema.insert("type".to_string(), Value::String("object".to_string()));
        schema.insert("description".to_string(), Value::String(description));
        schema.insert("properties".to_string(), Value::Object(properties));
        schema.insert("additionalProperties".to_string(), Value::Bool(false));
        per_rule.insert(entry.name.to_string(), Value::Object(schema));
    }

    let mut rules = Map::new();
    rules.insert("type".to_string(), Value::String("object".to_string()));
    rules.insert(
        "description".to_string(),
        Value::String(
            "Per-rule settings, keyed by kebab-case rule name (`no-unwrap-expect`). \
             Tables naming a rule arch-lint does not ship belong to custom rules and \
             are not checked here."
                .to_string(),
        ),
    );
    rules.insert("properties".to_string(), Value::Object(per_rule));
    rules.insert("additionalProperties".to_string(), Value::Bool(true));
    Value::Object(rules)
}

/// Extracts the `enabled` / `severity` / `exclude_files` property schemas,
/// descriptions included, from the generated [`RuleConfig`] schema.
fn common_rule_properties(rule_config: &Value) -> Map<String, Value> {
    let mut common = Map::new();
    if let Some(properties) = rule_config.get("properties").and_then(Value::as_object) {
        for key in COMMON_RULE_KEYS {
            if let Some(schema) = properties.get(*key) {
                common.insert((*key).to_string(), schema.clone());
            }
        }
    }
    common
}

/// Copies the top-level properties of the declarative configuration — the
/// `[[scopes]]`, `[[restrict-use]]`, ... sections — into the root schema.
fn merge_declarative_sections(root: &mut Value, defs: &mut Map<String, Value>) {
    let mut declarative = schemars::schema_for!(DeclarativeConfigDto).to_value();
    let renames = merge_defs(defs, take_defs(&mut declarative));
    rewrite_refs(&mut declarative, &renames);

    let Some(properties) = declarative.get("properties").and_then(Value::as_object) else {
        return;
    };
    for (key, schema) in properties.clone() {
        insert_property(root, key, schema);
    }
}

/// Removes and returns the `$defs` map of a generated schema.
fn take_defs(schema: &mut Value) -> Map<String, Value> {
    schema
        .as_object_mut()
        .and_then(|object| object.remove("$defs"))
        .and_then(|defs| match defs {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// Folds `incoming` definitions into `target`.
///
/// A definition whose name is already taken by an identical body is dropped;
/// one that clashes with a *different* body is inserted under a suffixed name.
/// The returned map lists those renames so that references to them can be
/// rewritten with [`rewrite_refs`].
fn merge_defs(
    target: &mut Map<String, Value>,
    incoming: Map<String, Value>,
) -> HashMap<String, String> {
    let mut renames = HashMap::new();
    let mut staged: Vec<(String, Value)> = Vec::new();

    for (name, body) in incoming {
        match target.get(&name) {
            Some(existing) if *existing == body => {}
            Some(_) => {
                let mut suffix = 1_u32;
                let mut candidate = format!("{name}_{suffix}");
                while target.contains_key(&candidate) {
                    suffix += 1;
                    candidate = format!("{name}_{suffix}");
                }
                renames.insert(name, candidate.clone());
                staged.push((candidate, body));
            }
            None => staged.push((name, body)),
        }
    }

    for (name, mut body) in staged {
        rewrite_refs(&mut body, &renames);
        target.insert(name, body);
    }

    renames
}

/// Retargets every `#/$defs/<name>` reference listed in `renames`.
fn rewrite_refs(value: &mut Value, renames: &HashMap<String, String>) {
    if renames.is_empty() {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some(renamed) = reference
                    .strip_prefix(DEFS_PREFIX)
                    .and_then(|name| renames.get(name))
                {
                    *reference = format!("{DEFS_PREFIX}{renamed}");
                }
            }
            for nested in map.values_mut() {
                rewrite_refs(nested, renames);
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_refs(item, renames);
            }
        }
        _ => {}
    }
}

/// Inserts (or replaces) one entry of the root `properties` object.
fn insert_property(root: &mut Value, key: String, schema: Value) {
    if let Some(properties) = root
        .as_object_mut()
        .and_then(|object| object.get_mut("properties"))
        .and_then(Value::as_object_mut)
    {
        properties.insert(key, schema);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Value {
        config_json_schema()
    }

    #[test]
    fn root_carries_identity_and_draft() {
        let schema = schema();
        assert_eq!(schema["$id"], SCHEMA_ID);
        assert_eq!(schema["title"], SCHEMA_TITLE);
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }

    #[test]
    fn root_properties_cover_config_and_declarative_sections() {
        let schema = schema();
        let properties = schema["properties"]
            .as_object()
            .expect("root has properties");
        for key in [
            "preset",
            "fail_on",
            "analyzer",
            "rules",
            "scopes",
            "restrict-use",
            "require-use",
            "deny-scope-dep",
            "layering",
            "deny-attr",
            "require-attr",
            "naming",
            "require-return",
        ] {
            assert!(properties.contains_key(key), "missing root key `{key}`");
        }
    }

    #[test]
    fn every_built_in_rule_has_a_closed_table_schema() {
        let schema = schema();
        let rules = &schema["properties"]["rules"];
        assert_eq!(rules["additionalProperties"], Value::Bool(true));

        for entry in rule_option_schemas() {
            let rule = &rules["properties"][entry.name];
            assert_eq!(
                rule["additionalProperties"],
                Value::Bool(false),
                "{} must reject unknown keys",
                entry.name
            );
            let description = rule["description"].as_str().unwrap_or_default();
            assert!(
                description.starts_with(entry.code),
                "{} description must start with its code: {description}",
                entry.name
            );

            let properties = rule["properties"]
                .as_object()
                .expect("rule table has properties");
            for key in COMMON_RULE_KEYS {
                assert!(
                    properties.contains_key(*key),
                    "{} is missing the common key `{key}`",
                    entry.name
                );
            }
            if let Some(options) = entry
                .schema
                .to_value()
                .get("properties")
                .and_then(Value::as_object)
            {
                for key in options.keys() {
                    assert!(
                        properties.contains_key(key),
                        "{} is missing its option `{key}`",
                        entry.name
                    );
                }
            }
        }
    }

    #[test]
    fn referenced_definitions_all_resolve() {
        let schema = schema();
        let defs = schema["$defs"].as_object().expect("schema has $defs");
        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);
        assert!(!refs.is_empty(), "expected at least one local reference");
        for reference in refs {
            let name = reference
                .strip_prefix(DEFS_PREFIX)
                .expect("only local $defs references are emitted");
            assert!(defs.contains_key(name), "dangling reference to `{name}`");
        }
    }

    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, nested) in map {
                    if key == "$ref" {
                        if let Some(reference) = nested.as_str() {
                            out.push(reference.to_string());
                        }
                    } else if key != "$defs" {
                        collect_refs(nested, out);
                    }
                }
                if let Some(defs) = map.get("$defs") {
                    collect_refs(defs, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_refs(item, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn analyzer_table_is_closed() {
        let schema = schema();
        let analyzer = resolve(&schema, &schema["properties"]["analyzer"]);
        assert_eq!(analyzer["additionalProperties"], Value::Bool(false));
        let properties = analyzer["properties"]
            .as_object()
            .expect("analyzer has properties");
        for key in [
            "root",
            "exclude",
            "include",
            "respect_gitignore",
            "parallelism",
        ] {
            assert!(properties.contains_key(key), "analyzer is missing `{key}`");
        }
    }

    /// Follows a single `$ref` into `$defs`, if the schema is a reference.
    fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
        schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix(DEFS_PREFIX))
            .map_or(schema, |name| &root["$defs"][name])
    }

    #[test]
    fn merge_defs_renames_only_conflicting_bodies() {
        let mut target = Map::new();
        target.insert("Same".to_string(), Value::Bool(true));
        target.insert("Other".to_string(), Value::Bool(true));

        let mut incoming = Map::new();
        incoming.insert("Same".to_string(), Value::Bool(true));
        incoming.insert("Other".to_string(), Value::Bool(false));
        incoming.insert("Fresh".to_string(), Value::Bool(false));

        let renames = merge_defs(&mut target, incoming);
        assert_eq!(renames.get("Other").map(String::as_str), Some("Other_1"));
        assert!(!renames.contains_key("Same"));
        assert!(!renames.contains_key("Fresh"));
        assert_eq!(target["Other"], Value::Bool(true));
        assert_eq!(target["Other_1"], Value::Bool(false));
        assert_eq!(target["Fresh"], Value::Bool(false));
    }

    #[test]
    fn rewrite_refs_retargets_nested_references() {
        let mut renames = HashMap::new();
        renames.insert("Old".to_string(), "New".to_string());
        let mut value = serde_json::json!({
            "items": [{ "$ref": "#/$defs/Old" }, { "$ref": "#/$defs/Kept" }]
        });
        rewrite_refs(&mut value, &renames);
        assert_eq!(value["items"][0]["$ref"], "#/$defs/New");
        assert_eq!(value["items"][1]["$ref"], "#/$defs/Kept");
    }
}
