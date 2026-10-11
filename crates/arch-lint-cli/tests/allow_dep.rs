//! CLI-level coverage of `[[allow-dep]]` (ALD011): issue #8 acceptance item 1
//! (the exit code) and acceptance item 4 (`list-rules` and `schema`).

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

/// Acceptance 1: `domain -> shared` (allowed) and `domain -> infra` (not
/// allowed) with `to = ["shared"]` makes `arch-lint check` print one
/// `ALD011` violation naming `domain → infra` and exit `1`. This is also
/// the `docs/declarative-rules.md` example: its output is quoted
/// verbatim there.
#[test]
fn domain_to_infra_exits_1_with_ald011() {
    let config = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[allow-dep]]
from = "domain"
to = ["shared"]
"#;
    let dir = project(
        config,
        &[
            ("src/domain/a.rs", "use crate::shared::util::Util;\n"),
            ("src/domain/b.rs", "use crate::infra::db::Pool;\n"),
        ],
    );

    let out = arch_lint(Some(dir.path()), &["check"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD011"), "{stdout}");
    assert!(stdout.contains("domain \u{2192} infra"), "{stdout}");
}

/// Acceptance 4: `arch-lint list-rules` names `ALD011` and `allow-dep`, in
/// both the human-readable table and `--json`.
#[test]
fn list_rules_shows_ald011_allow_dep() {
    let out = arch_lint(None, &["list-rules"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ALD011"), "{stdout}");
    assert!(stdout.contains("allow-dep"), "{stdout}");
}

#[test]
fn list_rules_json_contains_ald011_allow_dep() {
    let out = arch_lint(None, &["list-rules", "--json"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let entries = value.as_array().expect("a JSON array");
    let allow_dep = entries
        .iter()
        .find(|entry| entry.get("code") == Some(&serde_json::Value::String("ALD011".into())))
        .unwrap_or_else(|| panic!("no ALD011 entry: {stdout}"));

    assert_eq!(
        allow_dep["name"],
        serde_json::Value::String("allow-dep".into())
    );
    assert_eq!(
        allow_dep["table"],
        serde_json::Value::String("allow-dep".into())
    );
    assert!(
        allow_dep["options"]["properties"]["to"].is_object(),
        "{stdout}"
    );
    assert!(allow_dep["options"].get("$ref").is_none(), "{stdout}");
}

/// Acceptance 4: `arch-lint schema` includes the `[[allow-dep]]` table.
#[test]
fn schema_includes_allow_dep_table() {
    let out = arch_lint(None, &["schema"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON schema");
    assert!(
        value["properties"]["allow-dep"].is_object(),
        "schema must have a top-level `allow-dep` property: {stdout}"
    );
}
