//! Exit-status contract of the `arch-lint` binary.
//!
//! `0` ran and clean, `1` ran and found violations at or above `fail_on`,
//! `2` could not run. Each test builds a throwaway project and runs the
//! binary cargo built for this test (`CARGO_BIN_EXE_arch-lint`).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const UNWRAP_SRC: &str = "pub fn f(v: Option<u8>) -> u8 { v.unwrap() }\n";
const CLEAN_SRC: &str = "pub fn f(v: Option<u8>) -> u8 { v.unwrap_or(0) }\n";

fn project(config: &str, src: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    fs::write(dir.path().join("src/lib.rs"), src).expect("write lib.rs");
    fs::write(dir.path().join("arch-lint.toml"), config).expect("write config");
    dir
}

fn arch_lint(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .args(args)
        .arg(root)
        // Keep the global config out of the picture.
        .env("ARCH_LINT_CONFIG_DIR", root.join("no-global-config"))
        .output()
        .expect("run arch-lint")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

#[test]
fn clean_project_exits_0() {
    let dir = project("preset = \"recommended\"\n", CLEAN_SRC);
    let out = arch_lint(dir.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn error_violation_exits_1() {
    let dir = project("preset = \"recommended\"\n", UNWRAP_SRC);
    let out = arch_lint(dir.path(), &["check"]);
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stdout).contains("AL001"));
}

#[test]
fn warning_below_fail_on_exits_0() {
    let config = "preset = \"recommended\"\nfail_on = \"error\"\n\n\
                  [rules.no-unwrap-expect]\nseverity = \"warning\"\n";
    let dir = project(config, UNWRAP_SRC);
    let out = arch_lint(dir.path(), &["check"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stdout));
    assert!(String::from_utf8_lossy(&out.stdout).contains("AL001"));
}

#[test]
fn missing_explicit_config_exits_2() {
    let dir = project("preset = \"recommended\"\n", CLEAN_SRC);
    let out = arch_lint(dir.path(), &["check", "--config", "nonexistent.toml"]);
    assert_eq!(code(&out), 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nonexistent.toml"), "stderr: {stderr}");
}

#[test]
fn unknown_rule_exits_2() {
    let dir = project("preset = \"recommended\"\n", CLEAN_SRC);
    let out = arch_lint(dir.path(), &["check", "--rules", "not-a-rule"]);
    assert_eq!(code(&out), 2);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not-a-rule"), "stderr: {stderr}");
}

#[test]
fn exit_zero_reports_but_exits_0() {
    let dir = project("preset = \"recommended\"\n", UNWRAP_SRC);
    let out = arch_lint(dir.path(), &["check", "--exit-zero"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains("AL001"));
}

#[test]
fn exit_zero_does_not_mask_errors() {
    let dir = project("preset = \"recommended\"\n", CLEAN_SRC);
    let out = arch_lint(
        dir.path(),
        &["check", "--exit-zero", "--rules", "not-a-rule"],
    );
    assert_eq!(code(&out), 2);
}

#[test]
fn check_help_lists_exit_status() {
    let out = Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .args(["check", "--help"])
        .output()
        .expect("run arch-lint");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Exit status:"), "{stdout}");
    assert!(stdout.contains("--exit-zero"), "{stdout}");
}
