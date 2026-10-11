//! CLI-level coverage of `[[acyclic]]` (ALD009): issue #6 acceptance item 1
//! (the exit code) and acceptance item 6 (`list-rules` and `schema`).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn project(config: &str, files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for (relative, content) in files {
        let path = dir.path().join(relative);
        fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        fs::write(path, content).expect("write file");
    }
    fs::write(dir.path().join("arch-lint.toml"), config).expect("write config");
    dir
}

fn arch_lint(root: Option<&Path>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_arch-lint"));
    command.args(args);
    if let Some(root) = root {
        command
            .arg(root)
            // Keep the global config out of the picture.
            .env("ARCH_LINT_CONFIG_DIR", root.join("no-global-config"));
    }
    command.output().expect("run arch-lint")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

/// Acceptance 1: a `domain -> infra -> domain` cycle makes `arch-lint check`
/// print one `ALD009` violation naming the full path and exit `1`.
#[test]
fn two_scope_cycle_exits_1_with_ald009() {
    let config = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[acyclic]]
"#;
    let dir = project(
        config,
        &[
            ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
            ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
        ],
    );

    let out = arch_lint(Some(dir.path()), &["check"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD009"), "{stdout}");
    assert!(
        stdout.contains("domain \u{2192} infra \u{2192} domain"),
        "{stdout}"
    );
}

/// Acceptance 6: `arch-lint list-rules` names `ALD009` and `acyclic`, in
/// both the human-readable table and `--json`.
#[test]
fn list_rules_shows_ald009_acyclic() {
    let out = arch_lint(None, &["list-rules"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD009"), "{stdout}");
    assert!(stdout.contains("acyclic"), "{stdout}");
}

#[test]
fn list_rules_json_contains_ald009_acyclic() {
    let out = arch_lint(None, &["list-rules", "--json"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let entries = value.as_array().expect("a JSON array");
    let acyclic = entries
        .iter()
        .find(|entry| entry.get("code") == Some(&serde_json::Value::String("ALD009".into())))
        .unwrap_or_else(|| panic!("no ALD009 entry: {stdout}"));

    assert_eq!(acyclic["name"], serde_json::Value::String("acyclic".into()));
    // S2: `table` names the `[[acyclic]]` section these are the fields of
    // (not `[rules.acyclic]` keys), and `options` is a self-contained,
    // already-inlined schema (no bare `$ref`).
    assert_eq!(
        acyclic["table"],
        serde_json::Value::String("acyclic".into())
    );
    assert!(
        acyclic["options"]["properties"]["scopes"].is_object(),
        "{stdout}"
    );
    assert!(acyclic["options"].get("$ref").is_none(), "{stdout}");
}

#[test]
fn list_rules_json_contains_every_declarative_rule() {
    let out = arch_lint(None, &["list-rules", "--json"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let entries = value.as_array().expect("a JSON array");

    for ald_code in [
        "ALD001", "ALD002", "ALD003", "ALD004", "ALD005", "ALD006", "ALD007", "ALD008", "ALD009",
    ] {
        let entry = entries
            .iter()
            .find(|entry| entry.get("code") == Some(&serde_json::Value::String(ald_code.into())))
            .unwrap_or_else(|| panic!("no {ald_code} entry: {stdout}"));
        assert!(entry.get("table").is_some(), "{ald_code}: {stdout}");
        assert!(
            entry["options"]["properties"].is_object(),
            "{ald_code}: {stdout}"
        );
    }
}

/// Acceptance 6: `arch-lint schema` includes the `[[acyclic]]` table.
#[test]
fn schema_includes_acyclic_table() {
    let out = arch_lint(None, &["schema"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        value["properties"]["acyclic"].is_object(),
        "schema must have a top-level `acyclic` property: {stdout}"
    );
}
