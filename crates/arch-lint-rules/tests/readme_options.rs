//! Guards the README against option drift.
//!
//! Every option key a built-in rule accepts must be documented in that rule's
//! `[rules.<name>]` configuration block in `README.md`.

use arch_lint_rules::rule_option_schemas;

/// Reads the repository README.
fn readme() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("README.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Returns every fenced TOML block in `readme` whose first content line is the
/// `[rules.<name>]` header, one `String` per block.
fn config_blocks<'a>(readme: &'a str, rule: &str) -> Vec<&'a str> {
    let header = format!("[rules.{rule}]");
    let mut blocks = Vec::new();
    let mut rest = readme;

    while let Some(start) = rest.find(&header) {
        let block = &rest[start..];
        let end = block.find("```").unwrap_or(block.len());
        blocks.push(&block[..end]);
        rest = &rest[start + header.len()..];
    }
    blocks
}

/// Option keys of a rule, read from its generated schema.
fn option_keys(schema: schemars::Schema) -> Vec<String> {
    schema
        .to_value()
        .get("properties")
        .and_then(|properties| properties.as_object())
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn readme_documents_every_rule_option() {
    let readme = readme();

    for entry in rule_option_schemas() {
        let keys = option_keys(entry.schema);
        if keys.is_empty() {
            continue;
        }

        let blocks = config_blocks(&readme, entry.name);
        assert!(
            !blocks.is_empty(),
            "README has no `[rules.{}]` block, but {} accepts {:?}",
            entry.name,
            entry.code,
            keys
        );

        let documented = blocks
            .iter()
            .any(|block| keys.iter().all(|key| block.contains(key.as_str())));

        assert!(
            documented,
            "no `[rules.{}]` block in README documents every option of {}: {:?}",
            entry.name, entry.code, keys
        );
    }
}

#[test]
fn config_blocks_are_bounded_by_the_fence() {
    let readme = "```toml\n[rules.demo]\nkey = 1\n```\n\nprose\n";
    let blocks = config_blocks(readme, "demo");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].contains("key"));
    assert!(!blocks[0].contains("prose"));
}
