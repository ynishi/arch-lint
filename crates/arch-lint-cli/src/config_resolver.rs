//! Configuration file resolution with global fallback.
//!
//! Resolves the configuration file path using a deterministic priority order:
//!
//! 1. `--config` flag (explicit path)
//! 2. `{project}/arch-lint.toml` or `.arch-lint.toml`
//! 3. `~/.arch-lint/config.toml` (global fallback)
//! 4. No config found → defaults

use std::path::{Path, PathBuf};

/// Where the configuration was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Explicitly specified via `--config` flag.
    Explicit(PathBuf),
    /// Found in the project directory.
    Project(PathBuf),
    /// Loaded from the global config directory (`~/.arch-lint/`).
    Global(PathBuf),
    /// No config found; defaults will be used.
    Default,
}

impl ConfigSource {
    /// Returns the resolved path, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Explicit(p) | Self::Project(p) | Self::Global(p) => Some(p),
            Self::Default => None,
        }
    }

    /// Returns `true` if the config was loaded from the global directory.
    #[must_use]
    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global(_))
    }
}

/// Project-level config file names, checked in order.
const PROJECT_CONFIG_NAMES: &[&str] = &["arch-lint.toml", ".arch-lint.toml"];

/// Config file name within the global config directory.
const GLOBAL_CONFIG_NAME: &str = "config.toml";

/// Resolves the configuration file path.
///
/// See module-level docs for resolution order.
#[must_use]
pub fn resolve(project_dir: &Path, explicit: Option<&Path>) -> ConfigSource {
    resolve_inner(project_dir, explicit, global_config_dir())
}

/// Testable core: accepts `global_dir` as parameter to avoid env var races.
fn resolve_inner(
    project_dir: &Path,
    explicit: Option<&Path>,
    global_dir: Option<PathBuf>,
) -> ConfigSource {
    // 1. Explicit path from --config flag
    if let Some(p) = explicit {
        return ConfigSource::Explicit(p.to_path_buf());
    }

    // 2. Project-level config
    for name in PROJECT_CONFIG_NAMES {
        let candidate = project_dir.join(name);
        if candidate.exists() {
            tracing::debug!("Found project config: {}", candidate.display());
            return ConfigSource::Project(candidate);
        }
    }

    // 3. Global fallback
    if let Some(dir) = global_dir {
        let candidate = dir.join(GLOBAL_CONFIG_NAME);
        if candidate.exists() {
            tracing::debug!("Found global config: {}", candidate.display());
            return ConfigSource::Global(candidate);
        }
    }

    ConfigSource::Default
}

/// Returns the global config directory path.
///
/// Resolution: `$ARCH_LINT_CONFIG_DIR` > `~/.arch-lint/`
///
/// The env var override enables testing and custom CI setups.
#[must_use]
pub fn global_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("ARCH_LINT_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    home::home_dir().map(|h| h.join(".arch-lint"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn explicit_takes_priority_over_project() {
        let tmp = TempDir::new().unwrap();
        let explicit = tmp.path().join("custom.toml");
        fs::write(&explicit, "").unwrap();

        // Even when project config exists, explicit wins
        let project = tmp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("arch-lint.toml"), "").unwrap();

        let result = resolve_inner(&project, Some(&explicit), None);
        assert_eq!(result, ConfigSource::Explicit(explicit));
    }

    #[test]
    fn explicit_does_not_check_existence() {
        // Explicit path is trusted as-is (caller handles missing file error)
        let result = resolve_inner(
            Path::new("/tmp"),
            Some(Path::new("/nonexistent.toml")),
            None,
        );
        assert_eq!(
            result,
            ConfigSource::Explicit(PathBuf::from("/nonexistent.toml"))
        );
    }

    #[test]
    fn project_arch_lint_toml_found() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("arch-lint.toml"), "").unwrap();

        let result = resolve_inner(tmp.path(), None, None);
        assert_eq!(
            result,
            ConfigSource::Project(tmp.path().join("arch-lint.toml"))
        );
    }

    #[test]
    fn project_dot_arch_lint_toml_found() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".arch-lint.toml"), "").unwrap();

        let result = resolve_inner(tmp.path(), None, None);
        assert_eq!(
            result,
            ConfigSource::Project(tmp.path().join(".arch-lint.toml"))
        );
    }

    #[test]
    fn arch_lint_toml_preferred_over_dot_prefix() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("arch-lint.toml"), "").unwrap();
        fs::write(tmp.path().join(".arch-lint.toml"), "").unwrap();

        let result = resolve_inner(tmp.path(), None, None);
        assert_eq!(
            result,
            ConfigSource::Project(tmp.path().join("arch-lint.toml"))
        );
    }

    #[test]
    fn global_fallback_when_no_project_config() {
        let project = TempDir::new().unwrap();
        let global = TempDir::new().unwrap();
        fs::write(global.path().join("config.toml"), "").unwrap();

        let result = resolve_inner(project.path(), None, Some(global.path().to_path_buf()));
        assert_eq!(
            result,
            ConfigSource::Global(global.path().join("config.toml"))
        );
    }

    #[test]
    fn global_skipped_when_project_config_exists() {
        let project = TempDir::new().unwrap();
        fs::write(project.path().join("arch-lint.toml"), "").unwrap();

        let global = TempDir::new().unwrap();
        fs::write(global.path().join("config.toml"), "").unwrap();

        let result = resolve_inner(project.path(), None, Some(global.path().to_path_buf()));
        assert!(matches!(result, ConfigSource::Project(_)));
    }

    #[test]
    fn global_dir_missing_config_file_returns_default() {
        let project = TempDir::new().unwrap();
        let global = TempDir::new().unwrap();
        // global dir exists but no config.toml inside

        let result = resolve_inner(project.path(), None, Some(global.path().to_path_buf()));
        assert_eq!(result, ConfigSource::Default);
    }

    #[test]
    fn no_config_anywhere_returns_default() {
        let project = TempDir::new().unwrap();
        let result = resolve_inner(project.path(), None, None);
        assert_eq!(result, ConfigSource::Default);
    }

    #[test]
    fn config_source_path_returns_none_for_default() {
        assert!(ConfigSource::Default.path().is_none());
    }

    #[test]
    fn config_source_path_returns_some_for_all_others() {
        let p = PathBuf::from("/tmp/test.toml");
        assert_eq!(ConfigSource::Explicit(p.clone()).path(), Some(p.as_path()));
        assert_eq!(ConfigSource::Project(p.clone()).path(), Some(p.as_path()));
        assert_eq!(ConfigSource::Global(p.clone()).path(), Some(p.as_path()));
    }

    #[test]
    fn is_global_only_true_for_global() {
        assert!(!ConfigSource::Explicit(PathBuf::new()).is_global());
        assert!(!ConfigSource::Project(PathBuf::new()).is_global());
        assert!(ConfigSource::Global(PathBuf::new()).is_global());
        assert!(!ConfigSource::Default.is_global());
    }
}
