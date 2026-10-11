//! `cargo arch-lint` subcommand parity: issue #12.
//!
//! `cargo-arch-lint` is a second `[[bin]]` built from the same `main.rs` as
//! `arch-lint`. Cargo's external-subcommand convention invokes it as
//! `cargo-arch-lint arch-lint <args>` (the subcommand name, `arch-lint`,
//! arrives as the first argument) — these tests run it directly with that
//! argument prepended, which is exactly what `cargo arch-lint <args>` does.
//! `arch-lint` and `cargo-arch-lint` must otherwise behave identically.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Repository root, derived from this crate's manifest directory — the same
/// pattern `tests/sarif.rs` uses, so both binaries resolve
/// `examples/sample-project` the same way regardless of the test runner's
/// working directory.
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

/// Runs the `cargo-arch-lint` binary with `arch-lint` prepended to `args` —
/// the same argv cargo itself would pass when running `cargo arch-lint
/// <args>`.
fn cargo_arch_lint(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-arch-lint"))
        .arg("arch-lint")
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("run cargo-arch-lint")
}

/// Runs the `cargo-arch-lint` binary with `args` verbatim — no leading
/// `arch-lint` word, as if invoked directly rather than through cargo.
fn cargo_arch_lint_direct(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-arch-lint"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("run cargo-arch-lint")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("exit code")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const SUBCOMMANDS: &[&str] = &["check", "list-rules", "schema", "init", "graph"];

/// Acceptance 1 / Test 1: `cargo-arch-lint arch-lint --help` and
/// `arch-lint --help` list the same subcommands, with usage lines that name
/// the respective invocation.
#[test]
fn help_lists_same_subcommands_under_both_names() {
    let cargo_form = cargo_arch_lint(&["--help"]);
    let plain_form = arch_lint(&["--help"]);
    assert_eq!(code(&cargo_form), 0, "{}", stderr(&cargo_form));
    assert_eq!(code(&plain_form), 0, "{}", stderr(&plain_form));

    let cargo_help = stdout(&cargo_form);
    let plain_help = stdout(&plain_form);

    for name in SUBCOMMANDS {
        assert!(cargo_help.contains(name), "{cargo_help}");
        assert!(plain_help.contains(name), "{plain_help}");
    }

    assert!(
        cargo_help
            .lines()
            .any(|l| l.contains("Usage: cargo arch-lint")),
        "{cargo_help}"
    );
    assert!(
        plain_help.lines().any(|l| l.contains("Usage: arch-lint")),
        "{plain_help}"
    );
}

/// Acceptance 2 / Test 2: `cargo arch-lint check ...` and `arch-lint check
/// ...` produce identical stdout and the same (non-zero) exit code.
#[test]
fn check_produces_identical_output_under_both_names() {
    let cargo_form = cargo_arch_lint(&["check", "examples/sample-project", "--format", "compact"]);
    let plain_form = arch_lint(&["check", "examples/sample-project", "--format", "compact"]);

    assert_eq!(code(&cargo_form), 1, "{}", stderr(&cargo_form));
    assert_eq!(code(&cargo_form), code(&plain_form));
    assert_eq!(stdout(&cargo_form), stdout(&plain_form));
}

/// Acceptance 3 / Test 3: repeating the word `arch-lint` (the way a user
/// might type `cargo arch-lint arch-lint check ...`) is a clap error, not
/// silently accepted — only cargo's own copy of the word is stripped. The
/// plain `arch-lint` binary never strips anything, so the same user typo
/// (`arch-lint arch-lint check ...`) errors the same way.
#[test]
fn repeated_arch_lint_word_is_a_clap_error_under_both_names() {
    let cargo_form = cargo_arch_lint(&["arch-lint", "check", "examples/sample-project"]);
    let plain_form = arch_lint(&["arch-lint", "check", "examples/sample-project"]);

    assert_eq!(code(&cargo_form), 2, "{}", stderr(&cargo_form));
    assert_eq!(code(&plain_form), 2, "{}", stderr(&plain_form));
    assert!(
        stderr(&cargo_form).contains("arch-lint"),
        "{}",
        stderr(&cargo_form)
    );
    assert!(
        stderr(&plain_form).contains("arch-lint"),
        "{}",
        stderr(&plain_form)
    );
}

/// Acceptance 4 / Test 4: `--version` prints the same version string under
/// both names.
#[test]
fn version_is_identical_under_both_names() {
    let cargo_form = cargo_arch_lint(&["--version"]);
    let plain_form = arch_lint(&["--version"]);
    assert_eq!(code(&cargo_form), 0, "{}", stderr(&cargo_form));
    assert_eq!(code(&plain_form), 0, "{}", stderr(&plain_form));

    let cargo_version = stdout(&cargo_form);
    let plain_version = stdout(&plain_form);
    assert!(cargo_version.contains("0.7.0"), "{cargo_version}");
    assert_eq!(cargo_version, plain_version);
}

/// Test 5: `cargo-arch-lint` invoked directly, without the `arch-lint`
/// word (i.e. not through cargo), still works — the strip is conditional
/// on the first argument being exactly `arch-lint`, not unconditional.
#[test]
fn cargo_arch_lint_works_without_the_arch_lint_word() {
    let direct =
        cargo_arch_lint_direct(&["check", "examples/sample-project", "--format", "compact"]);
    let plain_form = arch_lint(&["check", "examples/sample-project", "--format", "compact"]);

    assert_eq!(code(&direct), 1, "{}", stderr(&direct));
    assert_eq!(code(&direct), code(&plain_form));
    assert_eq!(stdout(&direct), stdout(&plain_form));
}
