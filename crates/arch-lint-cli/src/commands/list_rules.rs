//! List rules command implementation.

use anyhow::{Context, Result};
use arch_lint_rules::{all_rules, rule_option_schemas};
use serde_json::{Map, Value};

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
    println!("\nDeclarative rules ([[restrict-use]], [[require-use]], [[deny-scope-dep]],");
    println!("[[layering]], [[deny-attr]], [[require-attr]], [[naming]], [[require-return]])");
    println!("are loaded from arch-lint.toml and always run alongside the preset.");

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

    let rules: Vec<Value> = rule_option_schemas()
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

    let rendered = serde_json::to_string_pretty(&Value::Array(rules))
        .context("failed to serialize the rule registry")?;
    println!("{rendered}");
    Ok(())
}
