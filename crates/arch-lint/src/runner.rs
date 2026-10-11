//! Internal runner for `check!()` macro integration.
//!
//! This module is `#[doc(hidden)]` and not part of the public API.
//! It is called by the generated test function from `arch_lint::check!()`.
//!
//! All of the work lives in [`crate::session`]; the only thing this module
//! adds is the `cargo test` contract — turning errors and violations into
//! panics.

use crate::baseline::Baseline;
use crate::session::{Session, SessionError};
use arch_lint_core::Severity;
use arch_lint_rules::Preset;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Runs arch-lint analysis as part of `cargo test`, without a baseline.
///
/// Called by the `check!()` macro-generated test function when no
/// `baseline = "..."` argument was given. **Signature frozen**: this is the
/// pre-baseline entry point, kept at its original 3-argument shape so that a
/// `check!()` call compiled against an older `arch-lint-macros` (within the
/// same minor line) still links against a newer `arch-lint` — see
/// [`run_check_with_baseline`] for the baseline path, added as a new
/// function rather than a new parameter here for the same reason.
///
/// Macro arguments take precedence over the configuration file, which takes
/// precedence over the built-in defaults (`recommended` / `error`).
///
/// # Panics
///
/// Panics if the session cannot be built, the analysis fails, or violations
/// at or above `fail_on` severity are found.
pub fn run_check(preset: Option<&str>, config_path: Option<&str>, fail_on: Option<&str>) {
    run_check_at(&find_project_root(), preset, config_path, fail_on, None);
}

/// Runs arch-lint analysis as part of `cargo test`, with a baseline.
///
/// Called by the `check!()` macro-generated test function when a
/// `baseline = "..."` argument was given — a separate function from
/// [`run_check`] rather than a new parameter on it, so that mixed versions
/// of `arch-lint-macros` and `arch-lint` within the same minor line never produce an arity
/// mismatch: an old macro binary only ever calls `run_check`, a new one
/// calls this function only when `baseline` is actually used.
///
/// `baseline` is a path relative to the workspace root (like `config`) to a
/// file written by `arch-lint check --baseline <path> --update-baseline`.
/// Only violations beyond it fail the test; both a passing and a failing run
/// print `baseline: N suppressed, M stale` to stderr (plus one
/// `baseline: stale entry <code> <file> (<message>) xN` line per stale
/// entry), and the panic report (when it panics) repeats that summary line
/// so it is visible even when stderr is captured.
///
/// # Panics
///
/// Panics if the session cannot be built or the analysis fails, if the
/// baseline file is missing or invalid, and if violations at or above
/// `fail_on` severity remain after baseline filtering.
#[doc(hidden)]
pub fn run_check_with_baseline(
    preset: Option<&str>,
    config_path: Option<&str>,
    fail_on: Option<&str>,
    baseline: Option<&str>,
) {
    run_check_at(&find_project_root(), preset, config_path, fail_on, baseline);
}

/// Shared implementation behind [`run_check`] and [`run_check_with_baseline`],
/// parameterized on the project root so tests can point it at a fixture
/// instead of `find_project_root()`'s real `CARGO_MANIFEST_DIR` walk.
///
/// # Panics
///
/// Same conditions as [`run_check`] / [`run_check_with_baseline`].
#[doc(hidden)]
pub fn run_check_at(
    root: &Path,
    preset: Option<&str>,
    config_path: Option<&str>,
    fail_on: Option<&str>,
    baseline: Option<&str>,
) {
    let session = build_session(root, preset, config_path, fail_on)
        .unwrap_or_else(|e| panic!("arch-lint: {e}"));

    let fail_on = session.fail_on();

    let Some(baseline_arg) = baseline else {
        let result = session.run().unwrap_or_else(|e| panic!("arch-lint: {e}"));
        if result.has_violations_at(fail_on) {
            let report = result.format_test_report(fail_on);
            panic!("{report}");
        }
        return;
    };

    let baseline_path = resolve_relative(root, baseline_arg);
    let baseline_data = Baseline::load(&baseline_path).unwrap_or_else(|e| {
        panic!(
            "arch-lint: failed to load baseline {} (resolved from the workspace root): {e}\n\
             Run `arch-lint check --baseline {baseline_arg} --update-baseline` to create or \
             regenerate it.",
            baseline_path.display(),
        )
    });

    let applied = session
        .with_baseline(baseline_data)
        .run_with_baseline()
        .unwrap_or_else(|e| panic!("arch-lint: {e}"));

    // Printed on every run with a baseline — passing or failing — so a
    // stale entry is visible from `cargo test -- --nocapture` even when the
    // test does not panic.
    eprintln!(
        "baseline: {} suppressed, {} stale",
        applied.suppressed,
        applied.stale.len()
    );
    for entry in &applied.stale {
        eprintln!(
            "baseline: stale entry {} {} ({}) x{}",
            entry.code,
            entry.file.display(),
            entry.message,
            entry.count,
        );
    }

    if applied.result.has_violations_at(fail_on) {
        let mut report = applied.result.format_test_report(fail_on);
        let _ = writeln!(
            report,
            "baseline: {} suppressed, {} stale",
            applied.suppressed,
            applied.stale.len()
        );
        panic!("{report}");
    }
}

/// Resolves `path` against `root` the same way `config` does: absolute paths
/// are kept as-is, relative ones are joined to the workspace root.
fn resolve_relative(root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// Builds the session, applying the macro's overrides on top of the config.
fn build_session(
    root: &std::path::Path,
    preset: Option<&str>,
    config_path: Option<&str>,
    fail_on: Option<&str>,
) -> Result<Session, SessionError> {
    let mut session = match config_path {
        Some(path) => Session::from_config_path(root, resolve_relative(root, path))?,
        None => Session::discover(root)?,
    };

    if let Some(name) = preset {
        session = session.with_preset(name.parse::<Preset>()?);
    }
    if let Some(name) = fail_on {
        session = session.with_fail_on(name.parse::<Severity>()?);
    }

    Ok(session)
}

/// Finds the project root by looking for `Cargo.toml` from `CARGO_MANIFEST_DIR`.
///
/// `CARGO_MANIFEST_DIR` points to the crate containing the test, which may be
/// a workspace member, so the workspace root above it is preferred.
fn find_project_root() -> PathBuf {
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        return Session::find_workspace_root(&PathBuf::from(manifest_dir));
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Writes `content` as the project config and builds a session for it.
    fn session_for(content: &str) -> (TempDir, Session) {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("arch-lint.toml"), content).unwrap();
        let session = Session::discover(tmp.path()).expect("discover");
        (tmp, session)
    }

    // ── Preset precedence: macro arg > config > default ──

    #[test]
    fn resolve_preset_defaults_to_recommended() {
        let (_tmp, session) = session_for("");
        let session = build_session_from(session, None, None).expect("build");
        assert_eq!(session.preset(), Preset::Recommended);
    }

    #[test]
    fn resolve_preset_macro_arg_takes_precedence() {
        let (_tmp, session) = session_for("preset = \"minimal\"");
        let session = build_session_from(session, Some("strict"), None).expect("build");
        assert_eq!(session.preset(), Preset::Strict);
    }

    #[test]
    fn resolve_preset_from_config() {
        let (_tmp, session) = session_for("preset = \"strict\"");
        let session = build_session_from(session, None, None).expect("build");
        assert_eq!(session.preset(), Preset::Strict);
    }

    #[test]
    fn resolve_preset_invalid_is_an_error() {
        let (_tmp, session) = session_for("");
        let err = match build_session_from(session, Some("nonexistent"), None) {
            Ok(_) => panic!("must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown preset"), "{err}");
    }

    // ── fail_on precedence: macro arg > config > default ──

    #[test]
    fn resolve_fail_on_defaults_to_error() {
        let (_tmp, session) = session_for("");
        let session = build_session_from(session, None, None).expect("build");
        assert_eq!(session.fail_on(), Severity::Error);
    }

    #[test]
    fn resolve_fail_on_from_config() {
        let (_tmp, session) = session_for("fail_on = \"warning\"");
        let session = build_session_from(session, None, None).expect("build");
        assert_eq!(session.fail_on(), Severity::Warning);
    }

    #[test]
    fn resolve_fail_on_macro_arg_overrides_config() {
        let (_tmp, session) = session_for("fail_on = \"info\"");
        let session = build_session_from(session, None, Some("warning")).expect("build");
        assert_eq!(session.fail_on(), Severity::Warning);
    }

    #[test]
    fn resolve_fail_on_explicit_error_overrides_config() {
        let (_tmp, session) = session_for("fail_on = \"warning\"");
        let session = build_session_from(session, None, Some("error")).expect("build");
        assert_eq!(session.fail_on(), Severity::Error);
    }

    #[test]
    fn resolve_fail_on_invalid_is_an_error() {
        let (_tmp, session) = session_for("");
        let err = match build_session_from(session, None, Some("critical")) {
            Ok(_) => panic!("must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown severity"), "{err}");
    }

    /// Mirrors the override half of [`build_session`] on an existing session.
    fn build_session_from(
        mut session: Session,
        preset: Option<&str>,
        fail_on: Option<&str>,
    ) -> Result<Session, SessionError> {
        if let Some(name) = preset {
            session = session.with_preset(name.parse::<Preset>()?);
        }
        if let Some(name) = fail_on {
            session = session.with_fail_on(name.parse::<Severity>()?);
        }
        Ok(session)
    }

    // ── Config discovery through the runner's entry point ──

    #[test]
    fn build_session_reads_explicit_relative_config() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("custom.toml"), "preset = \"minimal\"").unwrap();

        let session = build_session(tmp.path(), None, Some("custom.toml"), None).expect("build");
        assert_eq!(session.preset(), Preset::Minimal);
    }

    #[test]
    fn build_session_reports_missing_explicit_config() {
        let tmp = TempDir::new().unwrap();
        let err = match build_session(tmp.path(), None, Some("missing.toml"), None) {
            Ok(_) => panic!("must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("Failed to read config file"), "{err}");
    }

    #[test]
    fn build_session_discovers_project_config() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".arch-lint.toml"),
            "preset = \"strict\"\nfail_on = \"warning\"",
        )
        .unwrap();

        let session = build_session(tmp.path(), None, None, None).expect("build");
        assert_eq!(session.preset(), Preset::Strict);
        assert_eq!(session.fail_on(), Severity::Warning);
        assert_eq!(session.root(), tmp.path());
    }
}
