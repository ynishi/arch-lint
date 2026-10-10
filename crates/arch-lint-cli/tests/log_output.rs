//! Logs go to stderr, stdout is the report (issue #18).
//!
//! `tracing` previously defaulted to stdout, so `INFO` lines (`Analyzing
//! ...`, `Found N files ...`) preceded the report on every run and broke
//! machine-readable formats (`--format json`, `--format compact`). These
//! tests pin the fix: a plain run is silent on stderr, `--verbose` (or
//! `RUST_LOG`) sends logs to stderr only, and stdout always parses cleanly.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const CLEAN_SRC: &str = "pub fn f(v: Option<u8>) -> u8 { v.unwrap_or(0) }\n";
const UNWRAP_SRC: &str = "pub fn f(v: Option<u8>) -> u8 { v.unwrap() }\n";

fn project_with(src: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    fs::write(dir.path().join("src/lib.rs"), src).expect("write lib.rs");
    fs::write(
        dir.path().join("arch-lint.toml"),
        "preset = \"recommended\"\n",
    )
    .expect("write config");
    dir
}

fn project() -> TempDir {
    project_with(CLEAN_SRC)
}

/// A project with one `AL001` violation, so `--format compact` has at least
/// one `file:line:col: ...` line to inspect.
fn project_with_violation() -> TempDir {
    project_with(UNWRAP_SRC)
}

/// Does `line` start with the compact report's `file:line:col: ` shape
/// (`^[^:]+:\d+:\d+: `)? Written without a regex dependency: split on `:`
/// and check the second and third fields are decimal numbers, followed by
/// a space.
fn starts_with_file_line_col(line: &str) -> bool {
    let mut parts = line.splitn(4, ':');
    let _file = parts.next();
    let line_num = parts.next();
    let col_num = parts.next();
    let rest = parts.next();
    matches!(line_num, Some(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        && matches!(col_num, Some(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        && matches!(rest, Some(s) if s.starts_with(' '))
}

/// Runs `arch-lint check` against `dir` with `extra_args`, with `RUST_LOG`
/// scrubbed from the environment so the host's own setting cannot leak into
/// the test.
fn arch_lint(dir: &Path, extra_args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .args(extra_args)
        .arg(dir)
        .env("ARCH_LINT_CONFIG_DIR", dir.join("no-global-config"))
        .env_remove("RUST_LOG")
        .output()
        .expect("run arch-lint")
}

#[test]
fn json_format_stdout_is_valid_json() {
    let dir = project();
    let out = arch_lint(dir.path(), &["--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(
        out.stdout.first().copied(),
        Some(b'{'),
        "stdout did not start with '{{': {}",
        String::from_utf8_lossy(&out.stdout)
    );

    serde_json::from_slice::<serde_json::Value>(&out.stdout).expect("stdout must parse as JSON");
}

#[test]
fn compact_and_text_formats_have_no_logs_on_stdout_even_with_verbose() {
    // `--verbose` is the case that matters here: at the default `warn`
    // level, logs are already suppressed, so a test without `--verbose`
    // cannot tell "logs go to stderr" apart from "logs are off".
    let dir = project_with_violation();

    for format in ["compact", "text"] {
        let out = arch_lint(dir.path(), &["--verbose", "--format", format]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.lines().any(|line| line.contains(" INFO ")
                || line.contains("DEBUG")
                || line.contains(" WARN ")),
            "format {format}: stdout contains a log line under --verbose: {stdout}"
        );
    }
}

#[test]
fn compact_format_first_line_is_file_line_col() {
    let dir = project_with_violation();
    let out = arch_lint(dir.path(), &["--verbose", "--format", "compact"]);
    assert_eq!(out.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first_line = stdout
        .lines()
        .next()
        .expect("compact output should have at least one line");
    assert!(
        starts_with_file_line_col(first_line),
        "first compact line is not `file:line:col: ...`: {first_line:?}"
    );
}

#[test]
fn stderr_is_empty_without_verbose_on_a_successful_run() {
    let dir = project();
    let out = arch_lint(dir.path(), &["--format", "json"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stderr.is_empty(),
        "stderr should be empty: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn verbose_sends_info_logs_to_stderr_and_stdout_stays_json() {
    let dir = project();
    let out = arch_lint(dir.path(), &["--verbose", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Analyzing"), "stderr: {stderr}");

    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .expect("stdout must still parse as JSON under --verbose");
}

#[test]
fn rust_log_env_var_overrides_default_level_onto_stderr() {
    let dir = project();
    let out = Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .args(["--format", "json"])
        .arg(dir.path())
        .env("ARCH_LINT_CONFIG_DIR", dir.path().join("no-global-config"))
        .env("RUST_LOG", "info")
        .output()
        .expect("run arch-lint");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.lines().any(|line| line.contains(" INFO ")),
        "RUST_LOG=info should put INFO lines on stderr: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.lines().any(|line| line.contains(" INFO ")),
        "INFO lines must not leak onto stdout: {stdout}"
    );
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .expect("stdout must still parse as JSON with RUST_LOG=info");
}

#[test]
fn empty_rust_log_falls_back_to_verbose_default_instead_of_silencing_everything() {
    // `EnvFilter::builder().from_env_lossy()` treats `RUST_LOG=` (set but
    // empty) as "no directives", i.e. fall back to the default directive,
    // not as "an empty filter that matches nothing". `--verbose` must still
    // turn logging on in that case.
    let dir = project();
    let out = Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .args(["--verbose"])
        .arg(dir.path())
        .env("ARCH_LINT_CONFIG_DIR", dir.path().join("no-global-config"))
        .env("RUST_LOG", "")
        .output()
        .expect("run arch-lint");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Analyzing"),
        "RUST_LOG= (empty) with --verbose should still log: {stderr}"
    );
}

#[test]
fn rust_log_error_wins_over_verbose_and_stays_silent_on_success() {
    // Documents that RUST_LOG always overrides --verbose, in either
    // direction: here it lowers the level below what --verbose would pick.
    let dir = project();
    let out = Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .args(["--verbose"])
        .arg(dir.path())
        .env("ARCH_LINT_CONFIG_DIR", dir.path().join("no-global-config"))
        .env("RUST_LOG", "error")
        .output()
        .expect("run arch-lint");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "RUST_LOG=error should override --verbose and stay silent on success: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn invalid_rust_log_directive_is_reported_and_report_is_unaffected() {
    let dir = project_with_violation();
    let out = Command::new(env!("CARGO_BIN_EXE_arch-lint"))
        .arg("check")
        .args(["--format", "compact"])
        .arg(dir.path())
        .env("ARCH_LINT_CONFIG_DIR", dir.path().join("no-global-config"))
        .env("RUST_LOG", "bogus=notalevel")
        .output()
        .expect("run arch-lint");
    assert_eq!(out.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ignoring"),
        "an invalid RUST_LOG directive should be reported, not silently dropped: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout
            .lines()
            .next()
            .is_some_and(|line| starts_with_file_line_col(line)),
        "the report must still be produced despite the bad directive: {stdout}"
    );
}
