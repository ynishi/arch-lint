//! Internal runner for `check!()` macro integration.
//!
//! This module is `#[doc(hidden)]` and not part of the public API.
//! It is called by the generated test function from `arch_lint::check!()`.
//!
//! All of the work lives in [`crate::session`]; the only thing this module
//! adds is the `cargo test` contract — turning errors and violations into
//! panics.

use crate::session::{Session, SessionError};
use arch_lint_core::Severity;
use arch_lint_rules::Preset;
use std::path::PathBuf;

/// Runs arch-lint analysis as part of `cargo test`.
///
/// Called by the `check!()` macro-generated test function.
/// Panics with a formatted report if violations are found.
///
/// Macro arguments take precedence over the configuration file, which takes
/// precedence over the built-in defaults (`recommended` / `error`).
///
/// # Panics
///
/// Panics if the session cannot be built or the analysis fails, and if
/// violations at or above `fail_on` severity are found.
pub fn run_check(preset: Option<&str>, config_path: Option<&str>, fail_on: Option<&str>) {
    let root = find_project_root();

    let session = build_session(&root, preset, config_path, fail_on)
        .unwrap_or_else(|e| panic!("arch-lint: {e}"));

    let fail_on = session.fail_on();
    let result = session.run().unwrap_or_else(|e| panic!("arch-lint: {e}"));

    if result.has_violations_at(fail_on) {
        let report = result.format_test_report(fail_on);
        panic!("{report}");
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
        Some(path) => {
            let path = PathBuf::from(path);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            Session::from_config_path(root, path)?
        }
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
