//! CLI-level coverage of `arch-lint graph`: issue #9.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

/// Two scopes (`domain`, `infra`) and one `domain -> infra` reference that
/// `[[deny-scope-dep]]` (ALD003) flags — the same shape as
/// `crates/arch-lint-core/tests/fixtures/declarative`, built here as a
/// throwaway project so this test does not depend on the test binary's
/// working directory.
const DOMAIN_INFRA_CONFIG: &str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
severity = "error"
"#;

const DOMAIN_INFRA_FILES: &[(&str, &str)] = &[
    (
        "src/domain/service.rs",
        "use crate::infra::db::Connection;\n\npub fn run() {}\n",
    ),
    ("src/infra/db.rs", "pub struct Connection;\n"),
];

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

/// Acceptance 1 (Mermaid): nodes `domain`/`infra`, edge `domain -->|1|
/// infra`, and (ALD003 fires on that edge) a `linkStyle 0` line.
#[test]
fn mermaid_lists_scopes_edge_and_marks_the_violating_edge() {
    let dir = project(DOMAIN_INFRA_CONFIG, DOMAIN_INFRA_FILES);

    let out = arch_lint(Some(dir.path()), &["graph", "--format", "mermaid"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("graph LR\n"), "{stdout}");
    assert!(stdout.contains("s_domain[\"domain\"]"), "{stdout}");
    assert!(stdout.contains("s_infra[\"infra\"]"), "{stdout}");
    assert!(stdout.contains("s_domain -->|1| s_infra"), "{stdout}");
    assert!(stdout.contains("linkStyle 0"), "{stdout}");
}

/// Acceptance 3: `--format dot` output contains the quoted edge plus the
/// red styling attribute on the violating edge.
#[test]
fn dot_format_contains_quoted_edge_and_red_color() {
    let dir = project(DOMAIN_INFRA_CONFIG, DOMAIN_INFRA_FILES);

    let out = arch_lint(Some(dir.path()), &["graph", "--format", "dot"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("digraph scopes {\n"), "{stdout}");
    assert!(stdout.contains("\"domain\" -> \"infra\""), "{stdout}");
    assert!(stdout.contains("color=\"red\""), "{stdout}");
}

/// Acceptance 4: a project without `[[scopes]]` prints an empty graph and a
/// hint on stderr, exit 0.
#[test]
fn no_scopes_prints_empty_graph_and_stderr_hint() {
    let dir = project("", &[("src/lib.rs", "pub fn f() {}\n")]);

    let out = arch_lint(Some(dir.path()), &["graph"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "graph LR\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no [[scopes]]"), "{stderr}");
}

/// A project with `[[layers]]` (the tree-sitter engine) is rejected: `graph`
/// supports the `syn` engine only.
#[test]
fn layers_config_exits_2() {
    let config = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]
"#;
    let dir = project(config, &[("src/lib.rs", "pub fn f() {}\n")]);

    let out = arch_lint(Some(dir.path()), &["graph"]);
    assert_eq!(code(&out), 2, "{}", String::from_utf8_lossy(&out.stdout));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("syn engine only"), "{stderr}");
    assert!(stderr.contains("[[layers]]"), "{stderr}");
}

/// Acceptance 5: the subcommand is listed in `arch-lint --help`, and its own
/// `--help` lists both output formats.
#[test]
fn top_level_help_lists_graph() {
    let out = arch_lint(None, &["--help"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("graph"), "{stdout}");
}

#[test]
fn graph_help_lists_both_formats() {
    let out = arch_lint(None, &["graph", "--help"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("mermaid"), "{stdout}");
    assert!(stdout.contains("dot"), "{stdout}");
}

/// `--config` applies to `graph` the same way it does to `check`: an empty
/// default `arch-lint.toml` sits next to an explicit one, and only the
/// explicit one supplies `[[scopes]]`.
#[test]
fn explicit_config_flag_is_honored() {
    let dir = project(
        "",
        &[
            (
                "real.toml",
                "[[scopes]]\nname = \"domain\"\npaths = [\"src/domain/**\"]\n",
            ),
            ("src/domain/service.rs", "pub fn run() {}\n"),
        ],
    );

    let config_path = dir.path().join("real.toml");
    let out = arch_lint(
        Some(dir.path()),
        &[
            "graph",
            "--config",
            config_path.to_str().expect("utf-8 path"),
        ],
    );
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("domain"), "{stdout}");
}
