//! List rules command implementation.

use anyhow::{Context, Result};
use arch_lint_rules::{all_rules, rule_option_schemas};
use serde_json::{Map, Value};

/// One declarative rule (`ALD001`-`ALD011`), as shown by `list-rules`.
///
/// `table` is the TOML section name (`[[<table>]]`) and the matching key
/// under `properties` in the generated `arch-lint.toml` schema (see
/// `arch_lint::schema::config_json_schema`), so `options` can be pulled from
/// there rather than hand-written. `ALD009` (`acyclic`), `ALD010`
/// (`independent`) and `ALD011` (`allow-dep`) are the three project-wide
/// rules of the eleven — each computed once over the `ScopeGraph` rather
/// than once per file — which only affects their description, not how they
/// are presented here.
struct DeclarativeRuleInfo {
    code: &'static str,
    name: &'static str,
    description: &'static str,
    table: &'static str,
}

const DECLARATIVE_RULES: &[DeclarativeRuleInfo] = &[
    DeclarativeRuleInfo {
        code: "ALD001",
        name: "restrict-use",
        description: "Forbid use paths (and inline qualified paths) within a scope",
        table: "restrict-use",
    },
    DeclarativeRuleInfo {
        code: "ALD002",
        name: "require-use",
        description: "Require a preferred crate over alternatives within a file set",
        table: "require-use",
    },
    DeclarativeRuleInfo {
        code: "ALD003",
        name: "deny-scope-dep",
        description: "Forbid one scope from depending on another",
        table: "deny-scope-dep",
    },
    DeclarativeRuleInfo {
        code: "ALD004",
        name: "layering",
        description: "Forbid upward (and, with adjacent_only, layer-skipping) dependencies in an ordered layer stack",
        table: "layering",
    },
    DeclarativeRuleInfo {
        code: "ALD005",
        name: "deny-attr",
        description: "Forbid given attributes on items of a scope",
        table: "deny-attr",
    },
    DeclarativeRuleInfo {
        code: "ALD006",
        name: "require-attr",
        description: "Require at least one of the given attributes on items of a scope",
        table: "require-attr",
    },
    DeclarativeRuleInfo {
        code: "ALD007",
        name: "naming",
        description: "Require identifiers of one item kind to match a regex",
        table: "naming",
    },
    DeclarativeRuleInfo {
        code: "ALD008",
        name: "require-return",
        description: "Require functions to return a type matching a regex",
        table: "require-return",
    },
    DeclarativeRuleInfo {
        code: "ALD009",
        name: "acyclic",
        description: "Project-wide: forbid a cycle between scopes in the dependency graph",
        table: "acyclic",
    },
    DeclarativeRuleInfo {
        code: "ALD010",
        name: "independent",
        description: "Project-wide: forbid any dependency between a set of scopes, in either direction",
        table: "independent",
    },
    DeclarativeRuleInfo {
        code: "ALD011",
        name: "allow-dep",
        description: "Project-wide: forbid a scope from depending on anything outside an explicit allow-list",
        table: "allow-dep",
    },
];

/// Runs the list-rules command.
///
/// # Errors
///
/// Returns an error if the JSON form cannot be serialized.
pub fn run(json: bool) -> Result<()> {
    if json {
        return print_json();
    }
    print_table();
    print_options();
    Ok(())
}

/// Prints the code / name / description table and the preset summary.
fn print_table() {
    println!("Available rules:\n");
    println!("{:<10} {:<25} Description", "Code", "Name");
    println!("{}", "-".repeat(80));

    for rule in all_rules() {
        println!(
            "{:<10} {:<25} {}",
            rule.code(),
            rule.name(),
            rule.description()
        );
    }

    println!("\nPresets:");
    println!("  recommended  - AL001, AL002, AL003, AL005, AL006, AL007, AL013 (default)");
    println!("  strict       - All rules (AL001-AL013) with stricter settings");
    println!("  minimal      - AL001 only (for gradual adoption)");

    println!("\nDeclarative rules (loaded from arch-lint.toml, run alongside the preset):\n");
    println!("{:<10} {:<25} Description", "Code", "Name");
    println!("{}", "-".repeat(80));
    for rule in DECLARATIVE_RULES {
        println!(
            "{:<10} {:<25} {} ([[{}]])",
            rule.code, rule.name, rule.description, rule.table
        );
    }

    println!("\nUse --rules to filter specific rules, e.g.:");
    println!("  arch-lint check --rules no-unwrap-expect,no-sync-io");
    println!("  arch-lint check --rules AL001,AL002,AL003");
}

/// Prints the option keys of every rule, plus the keys shared by all of them.
fn print_options() {
    println!("\nOptions ([rules.<name>] in arch-lint.toml):\n");

    let common = arch_lint::schema::common_rule_schema();
    println!("Common keys, accepted by every rule:");
    print_properties(&common);

    for entry in rule_option_schemas() {
        let schema = entry.schema.to_value();
        println!("\n{} {}:", entry.code, entry.name);
        print_properties(&schema);
    }

    println!("\nRun `arch-lint schema > arch-lint.schema.json` for the machine-readable form.");
}

/// Prints one `key  type  description` line per property of `schema`.
fn print_properties(schema: &Value) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        println!("  (no options)");
        return;
    };
    if properties.is_empty() {
        println!("  (no options)");
        return;
    }

    for (key, property) in properties {
        println!(
            "  {:<22} {:<28} {}",
            key,
            type_of(schema, property),
            description_of(property)
        );
    }
}

/// Renders the accepted value type of one property, enum variants included.
fn type_of(root: &Value, property: &Value) -> String {
    let property = resolve(root, property);

    if let Some(values) = property.get("enum").and_then(Value::as_array) {
        let variants: Vec<String> = values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), String::from)
            })
            .collect();
        if !variants.is_empty() {
            return variants.join(" | ");
        }
    }

    if let Some(value) = property.get("const").and_then(Value::as_str) {
        return value.to_string();
    }

    if let Some(branches) = property
        .get("anyOf")
        .or_else(|| property.get("oneOf"))
        .and_then(Value::as_array)
    {
        // A `oneOf` of string constants is a closed set of values: list them.
        let variants: Vec<&str> = branches
            .iter()
            .filter_map(|branch| branch.get("const").and_then(Value::as_str))
            .collect();
        if !variants.is_empty() {
            return variants.join(" | ");
        }

        for branch in branches {
            let rendered = type_of(root, branch);
            if rendered != "null" && !rendered.is_empty() {
                return rendered;
            }
        }
    }

    let name = match property.get("type") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null")
            .unwrap_or("null")
            .to_string(),
        _ => String::new(),
    };

    if name == "array" {
        if let Some(items) = property.get("items") {
            let item = type_of(root, items);
            if !item.is_empty() {
                return format!("array of {item}");
            }
        }
    }
    name
}

/// Follows a `$ref` into the schema's own `$defs`.
fn resolve<'a>(root: &'a Value, property: &'a Value) -> &'a Value {
    property
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)))
        .unwrap_or(property)
}

/// The property's description, folded onto one line.
fn description_of(property: &Value) -> String {
    property
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Prints the rule registry as a JSON array.
fn print_json() -> Result<()> {
    let descriptions: std::collections::HashMap<String, String> = all_rules()
        .iter()
        .map(|rule| (rule.name().to_string(), rule.description().to_string()))
        .collect();

    let mut rules: Vec<Value> = rule_option_schemas()
        .into_iter()
        .map(|entry| {
            let mut object = Map::new();
            object.insert("code".to_string(), Value::String(entry.code.to_string()));
            object.insert("name".to_string(), Value::String(entry.name.to_string()));
            object.insert(
                "description".to_string(),
                Value::String(descriptions.get(entry.name).cloned().unwrap_or_default()),
            );
            object.insert("options".to_string(), entry.schema.to_value());
            Value::Object(object)
        })
        .collect();

    let config_schema = arch_lint::schema::config_json_schema();
    for rule in DECLARATIVE_RULES {
        rules.push(declarative_rule_entry(&config_schema, rule));
    }

    let rendered = serde_json::to_string_pretty(&Value::Array(rules))
        .context("failed to serialize the rule registry")?;
    println!("{rendered}");
    Ok(())
}

/// The JSON entry for one declarative rule (`ALD001`-`ALD011`), shaped like
/// the built-in rule entries: `code`, `name`, `description`, `options`, plus
/// `table` naming which `[[<table>]]` these are the fields of (so a reader
/// does not mistake them for `[rules.<name>]` keys, which is what `options`
/// means for every `AL*` entry above). `options` is the array item schema at
/// `config_schema.properties.<table>.items`, with every `$ref` inlined so it
/// is self-contained — the raw schema has it as a bare
/// `{"$ref": "#/$defs/..."}`.
fn declarative_rule_entry(config_schema: &Value, rule: &DeclarativeRuleInfo) -> Value {
    let mut object = Map::new();
    object.insert("code".to_string(), Value::String(rule.code.to_string()));
    object.insert("name".to_string(), Value::String(rule.name.to_string()));
    object.insert(
        "description".to_string(),
        Value::String(rule.description.to_string()),
    );
    object.insert("table".to_string(), Value::String(rule.table.to_string()));

    let items = config_schema
        .get("properties")
        .and_then(|properties| properties.get(rule.table))
        .and_then(|table| table.get("items"))
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    object.insert("options".to_string(), inline_schema(config_schema, &items));

    Value::Object(object)
}

/// Recursively replaces every `{"$ref": "#/$defs/Name"}` in `schema` with
/// the definition it points to (itself resolved the same way), so the
/// result is self-contained and does not need `config_schema`'s `$defs`
/// alongside it. `seen` guards against a reference cycle — rejoined as the
/// bare `$ref` if one is found, rather than looping forever.
fn inline_schema(config_schema: &Value, schema: &Value) -> Value {
    inline_schema_rec(config_schema, schema, &mut std::collections::HashSet::new())
}

fn inline_schema_rec(
    config_schema: &Value,
    schema: &Value,
    seen: &mut std::collections::HashSet<String>,
) -> Value {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
    {
        if seen.insert(name.to_string()) {
            if let Some(target) = config_schema.get("$defs").and_then(|defs| defs.get(name)) {
                let resolved = inline_schema_rec(config_schema, target, seen);
                seen.remove(name);
                return resolved;
            }
        }
        return schema.clone();
    }

    match schema {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), inline_schema_rec(config_schema, value, seen)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| inline_schema_rec(config_schema, item, seen))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarative_rule_entry_inlines_the_ref_and_names_its_table() {
        let config_schema = arch_lint::schema::config_json_schema();
        let acyclic = DECLARATIVE_RULES
            .iter()
            .find(|r| r.code == "ALD009")
            .expect("ALD009 is listed");

        let entry = declarative_rule_entry(&config_schema, acyclic);

        assert_eq!(entry["table"], Value::String("acyclic".to_string()));
        assert_eq!(entry["name"], Value::String("acyclic".to_string()));
        assert!(
            entry["options"].get("$ref").is_none(),
            "options must be self-contained, not an unresolved $ref: {entry:#?}"
        );
        assert!(
            entry["options"]["properties"]["scopes"].is_object(),
            "{entry:#?}"
        );
    }

    #[test]
    fn every_declarative_rule_resolves_to_a_concrete_options_object() {
        let config_schema = arch_lint::schema::config_json_schema();
        for rule in DECLARATIVE_RULES {
            let entry = declarative_rule_entry(&config_schema, rule);
            assert!(
                entry["options"]["properties"].is_object(),
                "{} ({}): {entry:#?}",
                rule.code,
                rule.name
            );
            assert!(entry["options"].get("$ref").is_none(), "{}", rule.code);
        }
    }

    /// N8: `DECLARATIVE_RULES` is hand-written, so a config enabling every
    /// table is built once here and fed through the real rule factories
    /// (`create_rules` for the eight per-file kinds, `create_project_rules`
    /// for `[[acyclic]]`) — if the two ever drift, this fails instead of
    /// `list-rules` silently going stale.
    #[test]
    fn declarative_rules_table_matches_the_real_rule_registry() {
        let toml = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[restrict-use]]
name = "r"
scope = "domain"
deny = ["sqlx::*"]
message = "m"

[[require-use]]
name = "ru"
scope = "domain"
prefer = "tracing"
over = ["log"]
message = "m"

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "m"

[[layering]]
order = ["domain", "infra"]
message = "m"

[[deny-attr]]
name = "da"
scope = "domain"
attrs = ["serde::Serialize"]
message = "m"

[[require-attr]]
name = "ra"
scope = "domain"
attrs = ["tracing::instrument"]
message = "m"

[[naming]]
name = "n"
scope = "domain"
item = "struct"
pattern = "^A"
message = "m"

[[require-return]]
name = "rr"
scope = "domain"
pattern = "^Result"
message = "m"

[[acyclic]]

[[independent]]
scopes = ["domain", "infra"]

[[allow-dep]]
from = "domain"
to = ["infra"]
"#;
        let config =
            arch_lint_core::declarative::load_config_from_toml(toml).expect("valid config");

        let mut built: Vec<(String, String)> =
            arch_lint_core::declarative::create_rules(config.clone())
                .iter()
                .map(|r| (r.code().to_string(), r.name().to_string()))
                .collect();
        built.extend(
            arch_lint_core::declarative::create_project_rules(config)
                .iter()
                .map(|r| (r.code().to_string(), r.name().to_string())),
        );
        built.sort();

        let mut expected: Vec<(String, String)> = DECLARATIVE_RULES
            .iter()
            .map(|r| (r.code.to_string(), r.name.to_string()))
            .collect();
        expected.sort();

        assert_eq!(
            built, expected,
            "DECLARATIVE_RULES has drifted from the real rule factories"
        );
    }
}
