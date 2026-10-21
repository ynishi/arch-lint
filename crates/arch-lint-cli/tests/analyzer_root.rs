//! `[analyzer] root` is honoured by the binary and is relative to the project
//! directory (issue 24).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const PACKAGE_MANIFEST: &str = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, content).expect("write");
}

fn arch_lint(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .arg(root)
        // Keep the global config out of the picture.
        .env("ARCH_LINT_CONFIG_DIR", root.join("no-global-config"))
        .output()
        .expect("run arch-lint")
}

/// A package whose `sub` directory is the configured root; the unwrap lives
/// outside it.
fn issue_layout(config: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "Cargo.toml", PACKAGE_MANIFEST);
    write(dir.path(), "arch-lint.toml", config);
    write(dir.path(), "sub/src/lib.rs", "pub fn b() {}\n");
    write(
        dir.path(),
        "other/src/lib.rs",
        "pub fn a() { let x: Option<u8> = None; x.unwrap(); }\n",
    );
    dir
}

#[test]
fn check_walks_only_the_configured_root() {
    let dir = issue_layout("[analyzer]\nroot = \"sub\"\n");
    let out = arch_lint(dir.path());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stdout.contains("AL001"), "{stdout}");
    assert!(stdout.contains("1 file"), "{stdout}");
}

#[test]
fn missing_root_exits_2_and_names_the_path() {
    let dir = issue_layout("[analyzer]\nroot = \"missing\"\n");
    let out = arch_lint(dir.path());

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&dir.path().join("missing").display().to_string()),
        "stderr: {stderr}"
    );
}
