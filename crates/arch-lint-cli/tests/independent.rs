//! CLI-level coverage of `[[independent]]` (ALD010): issue #7 acceptance
//! item 1 (the exit code) and acceptance item 4 (`list-rules` and
//! `schema`).

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

/// Acceptance 1: a `billing -> shipping` reference makes `arch-lint check`
/// print one `ALD010` violation naming both scopes and exit `1`.
#[test]
fn billing_to_shipping_exits_1_with_ald010() {
    let config = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[scopes]]
name = "shipping"
paths = ["src/shipping/**"]

[[independent]]
scopes = ["billing", "shipping"]
"#;
    let dir = project(
        config,
        &[(
            "src/billing/service.rs",
            "use crate::shipping::order::Order;\n",
        )],
    );

    let out = arch_lint(Some(dir.path()), &["check"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD010"), "{stdout}");
    assert!(stdout.contains("billing \u{2192} shipping"), "{stdout}");
}

/// Acceptance 4: `arch-lint list-rules` names `ALD010` and `independent`,
/// in both the human-readable table and `--json`.
#[test]
fn list_rules_shows_ald010_independent() {
    let out = arch_lint(None, &["list-rules"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD010"), "{stdout}");
    assert!(stdout.contains("independent"), "{stdout}");
}

#[test]
fn list_rules_json_contains_ald010_independent() {
    let out = arch_lint(None, &["list-rules", "--json"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let entries = value.as_array().expect("a JSON array");
    let independent = entries
        .iter()
        .find(|entry| entry.get("code") == Some(&serde_json::Value::String("ALD010".into())))
        .unwrap_or_else(|| panic!("no ALD010 entry: {stdout}"));

    assert_eq!(
        independent["name"],
        serde_json::Value::String("independent".into())
    );
    assert_eq!(
        independent["table"],
        serde_json::Value::String("independent".into())
    );
    assert!(
        independent["options"]["properties"]["scopes"].is_object(),
        "{stdout}"
    );
    assert!(independent["options"].get("$ref").is_none(), "{stdout}");
}

/// Acceptance 4: `arch-lint schema` includes the `[[independent]]` table.
#[test]
fn schema_includes_independent_table() {
    let out = arch_lint(None, &["schema"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        value["properties"]["independent"].is_object(),
        "schema must have a top-level `independent` property: {stdout}"
    );
}
