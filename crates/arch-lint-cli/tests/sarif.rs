//! CLI-level coverage of `--format sarif`: issue #11.
//!
//! Acceptance items 1-5 (the schema-validation item, 1, is covered
//! separately in the PR notes with the `curl`/`jsonschema` command run
//! against this binary's output — not re-run per `cargo test`, since it
//! needs network access).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

/// Repository root, derived from this crate's manifest directory — the same
/// pattern `commands::schema`'s tests use.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repository root resolves")
}

fn arch_lint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("run arch-lint")
}

fn json_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\n{output:?}"))
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

#[test]
fn sample_project_sarif_exits_1_and_is_valid_json() {
    let out = arch_lint(&["check", "examples/sample-project", "--format", "sarif"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));

    let doc = json_stdout(&out);
    assert_eq!(doc["version"], Value::String("2.1.0".to_string()));
    assert_eq!(doc["runs"].as_array().expect("array").len(), 1);
}

#[test]
fn sample_project_sarif_result_count_matches_json_violation_count() {
    let sarif = arch_lint(&["check", "examples/sample-project", "--format", "sarif"]);
    let json = arch_lint(&["check", "examples/sample-project", "--format", "json"]);

    let sarif_doc = json_stdout(&sarif);
    let json_doc = json_stdout(&json);

    let sarif_count = sarif_doc["runs"][0]["results"]
        .as_array()
        .expect("results array")
        .len();
    let json_count = json_doc["violations"]
        .as_array()
        .expect("violations array")
        .len();

    assert_eq!(sarif_count, json_count);
    // Sanity: the fixture is not clean, so this is a meaningful comparison,
    // not two empty arrays agreeing by accident.
    assert!(json_count > 0, "expected sample-project to have violations");
}

/// Two layers (`domain`, `infra`) where `domain` may not depend on `infra` —
/// same shape as `crates/arch-lint-ts/src/session.rs`'s own tests. Used for
/// both the violating and the clean fixture below; only the `.kt` file
/// content differs.
const TS_LAYERS_CONFIG: &str = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "infra"
packages = ["com.example.infra"]

[dependencies]
domain = []
infra = ["domain"]
"#;

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

fn arch_lint_in(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .args(args)
        .arg(root)
        .env("ARCH_LINT_CONFIG_DIR", root.join("no-global-config"))
        .output()
        .expect("run arch-lint")
}

#[test]
fn ts_engine_sarif_has_the_same_structure_with_a_violation() {
    let dir = project(
        TS_LAYERS_CONFIG,
        &[(
            "src/domain/Order.kt",
            "package com.example.domain\n\nimport com.example.infra.Database\n",
        )],
    );
    let out = arch_lint_in(
        dir.path(),
        &["check", "--engine", "ts", "--format", "sarif"],
    );
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));

    let doc = json_stdout(&out);
    assert_eq!(doc["version"], Value::String("2.1.0".to_string()));
    assert_eq!(doc["runs"].as_array().expect("array").len(), 1);

    let results = doc["runs"][0]["results"].as_array().expect("array");
    assert!(!results.is_empty(), "expected at least one violation");
    assert!(results[0]["ruleId"].is_string());
    assert!(results[0]["message"]["text"].is_string());
    assert!(results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"].is_string());
}

/// Three scopes (`domain`, `infra`, `web`) in a cycle
/// `domain -> infra -> web -> domain`: `ALD009` reports one violation,
/// attributed to the first edge, with the other two edges as `labels` —
/// which the SARIF renderer turns into `relatedLocations`.
const THREE_SCOPE_CYCLE_CONFIG: &str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "web"
paths = ["src/web/**"]

[[acyclic]]
"#;

#[test]
fn three_scope_cycle_has_two_related_locations() {
    let dir = project(
        THREE_SCOPE_CYCLE_CONFIG,
        &[
            ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
            ("src/infra/db.rs", "use crate::web::handler::Handler;\n"),
            (
                "src/web/handler.rs",
                "use crate::domain::service::Service;\n",
            ),
        ],
    );

    let out = arch_lint_in(dir.path(), &["check", "--format", "sarif"]);
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));

    let doc = json_stdout(&out);
    let results = doc["runs"][0]["results"].as_array().expect("array");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["ruleId"], Value::String("ALD009".to_string()));
    assert_eq!(
        results[0]["relatedLocations"]
            .as_array()
            .expect("relatedLocations array")
            .len(),
        2
    );
}

/// A baseline-able project (syn engine, `AL001`): bakes one `.unwrap()`
/// violation into the baseline, then adds a second one with a different
/// fingerprint (different file) and checks that `--baseline` +
/// `--format sarif` reports only the new one, while the
/// `baseline: N suppressed, M stale` summary still goes to stderr — the
/// same contract `--format json`/`text` have, just carried over to SARIF.
#[test]
fn sarif_with_baseline_reports_only_the_new_violation() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    fs::write(
        dir.path().join("src/a.rs"),
        "pub fn f(v: Option<i32>) -> i32 {\n    v.unwrap()\n}\n",
    )
    .expect("write src/a.rs");
    fs::write(
        dir.path().join("arch-lint.toml"),
        "preset = \"recommended\"\n",
    )
    .expect("write config");

    let baseline_path = dir.path().join("b.toml");
    let update = arch_lint_in(
        dir.path(),
        &[
            "check",
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--update-baseline",
        ],
    );
    assert_eq!(
        code(&update),
        0,
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    assert!(baseline_path.exists());

    // A second violation, in a different file — a different `(code, file,
    // message)` fingerprint, so it is not suppressed by the baseline above.
    fs::write(
        dir.path().join("src/b.rs"),
        "pub fn g(v: Option<i32>) -> i32 {\n    v.unwrap()\n}\n",
    )
    .expect("write src/b.rs");

    let out = arch_lint_in(
        dir.path(),
        &[
            "check",
            "--baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "sarif",
        ],
    );
    assert_eq!(code(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("baseline:"),
        "expected a `baseline:` summary line on stderr, got: {stderr}"
    );

    let doc = json_stdout(&out);
    let results = doc["runs"][0]["results"].as_array().expect("array");
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        Value::String("src/b.rs".to_string())
    );
}

#[test]
fn ts_engine_sarif_is_a_valid_empty_document_when_clean() {
    let dir = project(
        TS_LAYERS_CONFIG,
        &[(
            "src/domain/Order.kt",
            "package com.example.domain\n\nclass Order\n",
        )],
    );
    let out = arch_lint_in(
        dir.path(),
        &["check", "--engine", "ts", "--format", "sarif"],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    let doc = json_stdout(&out);
    assert_eq!(doc["version"], Value::String("2.1.0".to_string()));
    assert_eq!(doc["runs"].as_array().expect("array").len(), 1);
    assert_eq!(
        doc["runs"][0]["results"].as_array().expect("array").len(),
        0
    );
    assert_eq!(
        doc["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("array")
            .len(),
        0
    );
}
