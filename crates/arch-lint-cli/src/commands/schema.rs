//! Schema command implementation.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

/// Runs the schema command.
///
/// Prints the JSON Schema of `arch-lint.toml` to stdout, or writes it to
/// `out` when a path is given (creating or overwriting the file).
///
/// # Errors
///
/// Returns an error if the schema cannot be serialized, or if `out` cannot be
/// written.
pub fn run(out: Option<&Path>) -> Result<()> {
    let schema = arch_lint::schema::config_json_schema();
    let mut rendered =
        serde_json::to_string_pretty(&schema).context("failed to serialize the JSON Schema")?;
    rendered.push('\n');

    if let Some(path) = out {
        std::fs::write(path, &rendered)
            .with_context(|| format!("failed to write {}", path.display()))?;
        println!("Wrote {}", path.display());
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(rendered.as_bytes())?;
        stdout.flush()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::commands::{init::DEFAULT_CONFIG, init_ts::TS_CONFIG_TEMPLATE};
    use serde_json::Value;
    use std::path::{Path, PathBuf};

    /// The generated schema, built once per test.
    fn schema() -> Value {
        arch_lint::schema::config_json_schema()
    }

    /// Repository root, derived from this crate's manifest directory.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .expect("repository root resolves")
    }

    /// Every `arch-lint.toml` checked into the repository.
    fn fixture_files() -> Vec<PathBuf> {
        let root = repo_root();
        let mut files = vec![
            root.join("examples/sample-project/arch-lint.toml"),
            root.join("crates/arch-lint/tests/test-config.toml"),
        ];

        let fixtures = root.join("crates/arch-lint-core/tests/fixtures");
        let entries = std::fs::read_dir(&fixtures).expect("fixtures directory exists");
        for entry in entries.flatten() {
            let candidate = entry.path().join("arch-lint.toml");
            if candidate.is_file() {
                files.push(candidate);
            }
        }
        files.sort();
        files
    }

    /// Parses TOML text into its JSON representation.
    fn to_json(toml_str: &str) -> Value {
        let table: toml::Table = toml_str.parse().expect("fixture is valid TOML");
        serde_json::to_value(table).expect("TOML converts to JSON")
    }

    /// Follows a `$ref` into the schema's `$defs`.
    fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
        schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
            .map_or(schema, |name| &root["$defs"][name])
    }

    /// Property names accepted at one level of the schema.
    fn property_names(root: &Value, level: &Value) -> Vec<String> {
        resolve(root, level)
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| properties.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// `true` when the level rejects keys it does not declare.
    fn is_closed(root: &Value, level: &Value) -> bool {
        resolve(root, level).get("additionalProperties") == Some(&Value::Bool(false))
    }

    /// Structural check: every key of `document` is declared by the schema at
    /// its level, or sits at a level that accepts additional keys.
    fn check_document(schema: &Value, document: &Value, label: &str) {
        let root_keys = property_names(schema, schema);
        let root_open = !is_closed(schema, schema);

        let table = document.as_object().expect("a TOML document is a table");
        for key in table.keys() {
            assert!(
                root_keys.contains(key) || root_open,
                "{label}: root key `{key}` is not in the schema"
            );
        }

        if let Some(analyzer) = table.get("analyzer").and_then(Value::as_object) {
            let level = &schema["properties"]["analyzer"];
            let keys = property_names(schema, level);
            assert!(
                is_closed(schema, level),
                "the analyzer table must be closed"
            );
            for key in analyzer.keys() {
                assert!(
                    keys.contains(key),
                    "{label}: `[analyzer]` key `{key}` is not in the schema"
                );
            }
        }

        let Some(rules) = table.get("rules").and_then(Value::as_object) else {
            return;
        };
        for (name, rule_table) in rules {
            let level = &schema["properties"]["rules"]["properties"][name];
            if level.is_null() {
                // A custom rule: arch-lint does not know its option keys.
                continue;
            }
            assert!(
                is_closed(schema, level),
                "the `[rules.{name}]` table must be closed"
            );
            let keys = property_names(schema, level);
            let rule_table = rule_table
                .as_object()
                .expect("a rule configuration is a table");
            for key in rule_table.keys() {
                assert!(
                    keys.contains(key),
                    "{label}: `[rules.{name}]` key `{key}` is not in the schema"
                );
            }
        }
    }

    #[test]
    fn every_repository_fixture_matches_the_schema() {
        let schema = schema();
        let files = fixture_files();
        assert!(files.len() >= 5, "expected the known fixtures: {files:?}");

        for path in files {
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            check_document(&schema, &to_json(&content), &path.display().to_string());
        }
    }

    #[test]
    fn both_init_templates_match_the_schema() {
        let schema = schema();
        check_document(&schema, &to_json(DEFAULT_CONFIG), "init template");
        check_document(&schema, &to_json(TS_CONFIG_TEMPLATE), "init --ts template");
    }

    #[test]
    fn tree_sitter_keys_ride_on_the_open_root() {
        // The tree-sitter engine has its own configuration types, which are not
        // part of this schema; they are accepted because the root stays open.
        let schema = schema();
        assert!(!is_closed(&schema, &schema), "the root must stay open");

        let root_keys = property_names(&schema, &schema);
        for key in ["layers", "dependencies", "constraints"] {
            assert!(
                !root_keys.contains(&key.to_string()),
                "`{key}` is described by the tree-sitter config, not this schema"
            );
        }
    }

    #[test]
    fn typo_keys_are_absent_from_the_closed_levels() {
        let schema = schema();

        for (rule, typo) in [
            ("no-unwrap-expect", "allow_in_test"),
            ("handler-complexity", "max_match_arm"),
            ("no-sync-io", "allow_pattern"),
            ("require-tracing", "whatever"),
        ] {
            let level = &schema["properties"]["rules"]["properties"][rule];
            assert!(is_closed(&schema, level), "[rules.{rule}] must be closed");
            assert!(
                !property_names(&schema, level).contains(&typo.to_string()),
                "[rules.{rule}] must not accept `{typo}`"
            );
        }

        let analyzer = &schema["properties"]["analyzer"];
        assert!(
            !property_names(&schema, analyzer).contains(&"respect_gitignor".to_string()),
            "[analyzer] must not accept `respect_gitignor`"
        );
    }
}
