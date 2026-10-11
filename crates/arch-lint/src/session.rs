//! End-to-end driver: a project root plus a configuration file become a
//! [`LintResult`].
//!
//! A [`Session`] owns everything the `syn`-based engine needs for one run:
//! the analysis root, the parsed [`Config`], the declarative rules loaded from
//! the same TOML document, the effective preset and the effective `fail_on`
//! severity. It never panics and never exits the process — every failure is a
//! [`SessionError`].
//!
//! ```no_run
//! use arch_lint::Session;
//!
//! # fn main() -> Result<(), arch_lint::session::SessionError> {
//! let session = Session::discover(".")?;
//! let fail_on = session.fail_on();
//! let result = session.run()?;
//! assert!(!result.has_violations_at(fail_on));
//! # Ok(())
//! # }
//! ```
//!
//! # Configuration lookup
//!
//! [`Session::discover`] resolves the configuration file in this order:
//!
//! 1. `<root>/arch-lint.toml`
//! 2. `<root>/.arch-lint.toml`
//! 3. `<global dir>/config.toml`, where the global directory is
//!    `$ARCH_LINT_CONFIG_DIR` if set, otherwise `~/.arch-lint/`
//! 4. no file at all — built-in defaults
//!
//! The winning candidate is recorded in [`Session::config_source`].

use arch_lint_core::declarative::model::DeclarativeConfig;
use arch_lint_core::declarative::LoadRulesError;
use arch_lint_core::{
    Analyzer, AnalyzerError, Config, ConfigError, LintResult, ParseSeverityError, ProjectRuleBox,
    RuleBox, ScopeGraph, Severity,
};
use arch_lint_rules::{
    rule_from_config, validate_rule_tables, ParsePresetError, Preset, RuleOptionsError,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Project-level configuration file names, checked in order.
const PROJECT_CONFIG_NAMES: &[&str] = &["arch-lint.toml", ".arch-lint.toml"];

/// Configuration file name inside the global configuration directory.
const GLOBAL_CONFIG_NAME: &str = "config.toml";

/// Preset used when neither the caller nor the configuration file picks one.
const DEFAULT_PRESET: &str = "recommended";

/// Severity threshold used when neither the caller nor the configuration
/// file picks one.
const DEFAULT_FAIL_ON: &str = "error";

/// Where the configuration used by a [`Session`] came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// A path the caller named explicitly (the CLI `--config` flag).
    Explicit(PathBuf),
    /// A file found in the project directory.
    Project(PathBuf),
    /// A file found in the global configuration directory.
    Global(PathBuf),
    /// Nothing was found; built-in defaults apply.
    Default,
}

impl ConfigSource {
    /// Resolves a configuration source for `project_dir`.
    ///
    /// `explicit` short-circuits the search and is trusted as-is: its
    /// existence is checked when the [`Session`] reads it, so that a missing
    /// `--config` path reports a read error rather than silently falling back
    /// to defaults.
    #[must_use]
    pub fn resolve(project_dir: &Path, explicit: Option<&Path>) -> Self {
        Self::resolve_in(project_dir, explicit, global_config_dir())
    }

    /// Same as [`ConfigSource::resolve`], with the global directory supplied
    /// by the caller instead of read from the environment.
    ///
    /// Pass `None` to disable the global fallback entirely. Tests use this to
    /// avoid racing on `$ARCH_LINT_CONFIG_DIR`.
    #[must_use]
    pub fn resolve_in(
        project_dir: &Path,
        explicit: Option<&Path>,
        global_dir: Option<PathBuf>,
    ) -> Self {
        if let Some(p) = explicit {
            return Self::Explicit(p.to_path_buf());
        }

        for name in PROJECT_CONFIG_NAMES {
            let candidate = project_dir.join(name);
            if candidate.exists() {
                tracing::debug!("Found project config: {}", candidate.display());
                return Self::Project(candidate);
            }
        }

        if let Some(dir) = global_dir {
            let candidate = dir.join(GLOBAL_CONFIG_NAME);
            if candidate.exists() {
                tracing::debug!("Found global config: {}", candidate.display());
                return Self::Global(candidate);
            }
        }

        Self::Default
    }

    /// Returns the resolved path, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Explicit(p) | Self::Project(p) | Self::Global(p) => Some(p),
            Self::Default => None,
        }
    }

    /// Returns `true` if the configuration came from the global directory.
    #[must_use]
    pub fn is_global(&self) -> bool {
        matches!(self, Self::Global(_))
    }
}

/// Returns the global configuration directory.
///
/// Resolution order: `$ARCH_LINT_CONFIG_DIR`, then `~/.arch-lint/`. Returns
/// `None` when neither is available (no home directory on this platform).
#[must_use]
pub fn global_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("ARCH_LINT_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    home::home_dir().map(|h| h.join(".arch-lint"))
}

/// Renders `" in <path>"` for error messages, or an empty string.
fn in_path(path: Option<&PathBuf>) -> String {
    path.map_or_else(String::new, |p| format!(" in {}", p.display()))
}

/// Everything that can go wrong while preparing or running a [`Session`].
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The configuration file could not be read.
    #[error("Failed to read config file {path}")]
    ReadConfig {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// The configuration file is not valid arch-lint TOML.
    #[error("Failed to parse config{}", in_path(.path.as_ref()))]
    ParseConfig {
        /// Path the content came from, if it came from a file.
        path: Option<PathBuf>,
        /// Underlying parse error.
        #[source]
        source: ConfigError,
    },

    /// The declarative rule sections of the configuration are invalid.
    #[error("Invalid declarative rules{}", in_path(.path.as_ref()))]
    Declarative {
        /// Path the content came from, if it came from a file.
        path: Option<PathBuf>,
        /// Underlying load error.
        #[source]
        source: LoadRulesError,
    },

    /// The configured or requested preset does not exist.
    #[error(transparent)]
    Preset(#[from] ParsePresetError),

    /// The configured or requested `fail_on` severity does not exist.
    #[error(transparent)]
    Severity(#[from] ParseSeverityError),

    /// A `[rules.<name>]` table carries an option the rule does not accept.
    #[error(transparent)]
    RuleOptions(#[from] RuleOptionsError),

    /// A rule name passed to [`Session::with_rule_filter`] is not known.
    #[error("Unknown rule `{0}`. Run `arch-lint list-rules` to see valid names.")]
    UnknownRule(String),

    /// The analyzer could not be built from this session.
    #[error("Failed to build analyzer")]
    Build(#[source] AnalyzerError),

    /// The analysis itself failed.
    #[error("Analysis failed")]
    Analyze(#[source] AnalyzerError),
}

/// One configured `syn`-engine run.
///
/// Build one with [`Session::discover`], [`Session::from_config_path`] or
/// [`Session::from_config_str`], adjust it with the `with_*` methods, then
/// call [`Session::run`].
pub struct Session {
    root: PathBuf,
    config: Config,
    config_source: ConfigSource,
    preset: Preset,
    fail_on: Severity,
    declarative: Vec<RuleBox>,
    /// Project-wide declarative rules (`[[acyclic]]`, `ALD009`, and
    /// `[[independent]]`, `ALD010`), built from the same config as
    /// `declarative`.
    declarative_project: Vec<ProjectRuleBox>,
    /// The declarative config the rules were built from; the analyzer uses it
    /// to collect the [`ScopeGraph`]. `None` when the config had no content.
    declarative_config: Option<Arc<DeclarativeConfig>>,
    rules: Option<Vec<RuleBox>>,
    excludes: Vec<String>,
}

impl std::fmt::Debug for Session {
    /// Rule sets are boxed trait objects, so they are summarized by count.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("root", &self.root)
            .field("config_source", &self.config_source)
            .field("preset", &self.preset)
            .field("fail_on", &self.fail_on)
            .field("declarative_rules", &self.declarative.len())
            .field("declarative_project_rules", &self.declarative_project.len())
            .field("explicit_rules", &self.rules.as_ref().map(Vec::len))
            .field("excludes", &self.excludes)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Builds a session for `root`, discovering its configuration file.
    ///
    /// See the [module docs](self) for the lookup order.
    ///
    /// # Errors
    ///
    /// Returns an error if the discovered file cannot be read or parsed, or
    /// if it names an unknown preset, severity or declarative rule.
    pub fn discover(root: impl AsRef<Path>) -> Result<Self, SessionError> {
        let root = root.as_ref();
        let source = ConfigSource::resolve(root, None);
        Self::from_source(root, source)
    }

    /// Builds a session for `root` from an explicit configuration file.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` cannot be read or parsed, or if it names an
    /// unknown preset, severity or declarative rule.
    pub fn from_config_path(
        root: impl AsRef<Path>,
        path: impl AsRef<Path>,
    ) -> Result<Self, SessionError> {
        let source = ConfigSource::Explicit(path.as_ref().to_path_buf());
        Self::from_source(root.as_ref(), source)
    }

    /// Builds a session for `root` from configuration held in memory.
    ///
    /// Useful for embedders that keep arch-lint configuration somewhere other
    /// than a file. The resulting [`Session::config_source`] is
    /// [`ConfigSource::Default`]. An empty string yields the built-in
    /// defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if `toml_str` cannot be parsed, or if it names an
    /// unknown preset, severity or declarative rule.
    pub fn from_config_str(root: impl AsRef<Path>, toml_str: &str) -> Result<Self, SessionError> {
        Self::assemble(
            root.as_ref().to_path_buf(),
            toml_str,
            ConfigSource::Default,
            None,
        )
    }

    /// Builds a session for `root` from an already-resolved [`ConfigSource`].
    ///
    /// This is what [`Session::discover`] and [`Session::from_config_path`]
    /// call. Use it directly when the caller has to inspect the source before
    /// building the session — the CLI does, to pick an engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be read or parsed, or if it names
    /// an unknown preset, severity or declarative rule.
    pub fn from_source(root: impl AsRef<Path>, source: ConfigSource) -> Result<Self, SessionError> {
        let root = root.as_ref().to_path_buf();

        let Some(path) = source.path() else {
            return Self::assemble(root, "", source, None);
        };

        if source.is_global() {
            tracing::info!("Using global config: {}", path.display());
        }
        let path = path.to_path_buf();
        let content = std::fs::read_to_string(&path).map_err(|e| SessionError::ReadConfig {
            path: path.clone(),
            source: e,
        })?;
        Self::assemble(root, &content, source, Some(path))
    }

    /// Shared constructor: parse `content`, resolve preset / `fail_on`, load
    /// declarative rules.
    fn assemble(
        root: PathBuf,
        content: &str,
        config_source: ConfigSource,
        path: Option<PathBuf>,
    ) -> Result<Self, SessionError> {
        let (config, declarative, declarative_project, declarative_config) = if content.is_empty() {
            (Config::default(), Vec::new(), Vec::new(), None)
        } else {
            let config = Config::parse(content).map_err(|source| SessionError::ParseConfig {
                path: path.clone(),
                source,
            })?;
            let declarative_config = arch_lint_core::declarative::load_config_from_toml(content)
                .map(Arc::new)
                .map_err(|source| SessionError::Declarative { path, source })?;
            let declarative = arch_lint_core::declarative::create_rules_shared(&declarative_config);
            let declarative_project =
                arch_lint_core::declarative::create_project_rules_shared(&declarative_config);
            // Every `[rules.<name>]` table is checked here, not only the ones
            // the active preset builds, so a typo in any of them is an error.
            validate_rule_tables(&config)?;
            (
                config,
                declarative,
                declarative_project,
                Some(declarative_config),
            )
        };

        let preset: Preset = config.preset.as_deref().unwrap_or(DEFAULT_PRESET).parse()?;
        let fail_on: Severity = config
            .fail_on
            .as_deref()
            .unwrap_or(DEFAULT_FAIL_ON)
            .parse()?;

        Ok(Self {
            root,
            config,
            config_source,
            preset,
            fail_on,
            declarative,
            declarative_project,
            declarative_config,
            rules: None,
            excludes: Vec::new(),
        })
    }

    /// Walks up from `start` looking for a `Cargo.toml` that declares a
    /// `[workspace]` section, and returns the directory containing it.
    ///
    /// Returns `start` itself when no workspace manifest is found. The
    /// manifest is parsed as TOML rather than substring-matched, so a
    /// commented-out or quoted `[workspace]` does not count.
    #[must_use]
    pub fn find_workspace_root(start: &Path) -> PathBuf {
        let mut candidate = start;
        loop {
            let cargo_toml = candidate.join("Cargo.toml");
            if cargo_toml.exists() && has_workspace_section(&cargo_toml) {
                return candidate.to_path_buf();
            }
            match candidate.parent() {
                Some(parent) => candidate = parent,
                None => break,
            }
        }
        start.to_path_buf()
    }

    /// Overrides the preset, ignoring the one named in the configuration file.
    #[must_use]
    pub fn with_preset(mut self, preset: Preset) -> Self {
        self.preset = preset;
        self
    }

    /// Overrides the severity threshold reported by [`Session::fail_on`].
    #[must_use]
    pub fn with_fail_on(mut self, fail_on: Severity) -> Self {
        self.fail_on = fail_on;
        self
    }

    /// Replaces the preset rule set with an explicit list.
    ///
    /// Declarative rules from the configuration file are still added on top.
    #[must_use]
    pub fn with_rules(mut self, rules: Vec<RuleBox>) -> Self {
        self.rules = Some(rules);
        self
    }

    /// Adds one exclude pattern on top of the ones in the configuration file.
    #[must_use]
    pub fn with_exclude(mut self, pattern: impl Into<String>) -> Self {
        self.excludes.push(pattern.into());
        self
    }

    /// Replaces the preset rule set with the named rules.
    ///
    /// Names may be kebab-case rule names (`no-unwrap-expect`) or codes
    /// (`AL001`). Per-rule options from the configuration file are applied.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::UnknownRule`] for the first name that does not
    /// match a built-in rule, and [`SessionError::RuleOptions`] if a
    /// `[rules.<name>]` table carries an option that rule does not accept.
    pub fn with_rule_filter(mut self, names: &[&str]) -> Result<Self, SessionError> {
        let mut rules = Vec::with_capacity(names.len());
        for name in names {
            match rule_from_config(name, &self.config)? {
                Some(rule) => rules.push(rule),
                None => return Err(SessionError::UnknownRule((*name).to_string())),
            }
        }
        self.rules = Some(rules);
        Ok(self)
    }

    /// The directory this session analyzes.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The parsed configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Where the configuration came from.
    #[must_use]
    pub fn config_source(&self) -> &ConfigSource {
        &self.config_source
    }

    /// The effective preset.
    #[must_use]
    pub fn preset(&self) -> Preset {
        self.preset
    }

    /// The effective severity threshold.
    ///
    /// Pass it to [`LintResult::has_violations_at`] to decide whether the run
    /// counts as a failure.
    #[must_use]
    pub fn fail_on(&self) -> Severity {
        self.fail_on
    }

    /// Runs the analysis.
    ///
    /// Consumes the session because the rule set it carries is a collection of
    /// boxed trait objects that is moved into the analyzer.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::RuleOptions`] if a `[rules.<name>]` table
    /// carries an option that rule does not accept,
    /// [`SessionError::Build`] if the analyzer rejects the configuration, and
    /// [`SessionError::Analyze`] if the analysis itself fails.
    pub fn run(self) -> Result<LintResult, SessionError> {
        // Computed before destructuring `self`: an explicit `with_rules` (or
        // `with_rule_filter`) wins and the preset is not evaluated at all;
        // otherwise the preset supplies the rule set.
        let preset_rules = match self.rules {
            Some(_) => None,
            None => Some(self.preset.rules_with_config(&self.config)?),
        };
        let Self {
            root,
            config,
            declarative,
            declarative_project,
            declarative_config,
            rules: explicit,
            excludes,
            ..
        } = self;
        let rules = explicit.or(preset_rules).unwrap_or_default();

        let mut builder = Self::base_builder(&root, config, excludes, declarative_config.as_ref());
        for rule in rules {
            builder = builder.rule_box(rule);
        }
        for rule in declarative {
            builder = builder.rule_box(rule);
        }
        for rule in declarative_project {
            builder = builder.project_rule_box(rule);
        }

        let analyzer = builder.build().map_err(SessionError::Build)?;

        tracing::info!("Analyzing {:?} with {} rules", root, analyzer.rule_count());

        analyzer.analyze().map_err(SessionError::Analyze)
    }

    /// Builds the [`ScopeGraph`] of the project without running any rule.
    ///
    /// Same file discovery (root, `[analyzer]` settings, `with_exclude`
    /// overrides) and the same `[[scopes]]` as [`Session::run`], so the graph
    /// equals the one project rules see during a run. A config without
    /// `[[scopes]]` yields an empty graph.
    ///
    /// # Errors
    ///
    /// Returns an error if the analyzer cannot be built or file discovery
    /// fails.
    pub fn scope_graph(&self) -> Result<ScopeGraph, SessionError> {
        let analyzer = Self::base_builder(
            &self.root,
            self.config.clone(),
            self.excludes.clone(),
            self.declarative_config.as_ref(),
        )
        .build()
        .map_err(SessionError::Build)?;
        analyzer.scope_graph().map_err(SessionError::Analyze)
    }

    /// The analyzer builder shared by [`Session::run`] and
    /// [`Session::scope_graph`]: root, config, excludes and the declarative
    /// config, without rules.
    fn base_builder(
        root: &Path,
        config: Config,
        excludes: Vec<String>,
        declarative_config: Option<&Arc<DeclarativeConfig>>,
    ) -> arch_lint_core::AnalyzerBuilder {
        let mut builder = Analyzer::builder().root(root).config(config);
        for pattern in excludes {
            builder = builder.exclude(pattern);
        }
        if let Some(declarative_config) = declarative_config {
            builder = builder.declarative(Arc::clone(declarative_config));
        }
        builder
    }
}

/// Returns `true` if `cargo_toml` parses as TOML and has a `[workspace]` key.
fn has_workspace_section(cargo_toml: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return false;
    };
    table.contains_key("workspace")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── ConfigSource resolution ──

    #[test]
    fn explicit_takes_priority_over_project() {
        let tmp = TempDir::new().unwrap();
        let explicit = tmp.path().join("custom.toml");
        fs::write(&explicit, "").unwrap();

        let project = tmp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("arch-lint.toml"), "").unwrap();

        let result = ConfigSource::resolve_in(&project, Some(&explicit), None);
        assert_eq!(result, ConfigSource::Explicit(explicit));
    }

    #[test]
    fn explicit_does_not_check_existence() {
        let result = ConfigSource::resolve_in(
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

        let result = ConfigSource::resolve_in(tmp.path(), None, None);
        assert_eq!(
            result,
            ConfigSource::Project(tmp.path().join("arch-lint.toml"))
        );
    }

    #[test]
    fn project_dot_arch_lint_toml_found() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".arch-lint.toml"), "").unwrap();

        let result = ConfigSource::resolve_in(tmp.path(), None, None);
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

        let result = ConfigSource::resolve_in(tmp.path(), None, None);
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

        let result =
            ConfigSource::resolve_in(project.path(), None, Some(global.path().to_path_buf()));
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

        let result =
            ConfigSource::resolve_in(project.path(), None, Some(global.path().to_path_buf()));
        assert!(matches!(result, ConfigSource::Project(_)));
    }

    #[test]
    fn global_dir_missing_config_file_returns_default() {
        let project = TempDir::new().unwrap();
        let global = TempDir::new().unwrap();

        let result =
            ConfigSource::resolve_in(project.path(), None, Some(global.path().to_path_buf()));
        assert_eq!(result, ConfigSource::Default);
    }

    #[test]
    fn no_config_anywhere_returns_default() {
        let project = TempDir::new().unwrap();
        let result = ConfigSource::resolve_in(project.path(), None, None);
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

    // ── Preset / fail_on precedence ──

    #[test]
    fn preset_defaults_to_recommended() {
        let session = Session::from_config_str(".", "").expect("defaults");
        assert_eq!(session.preset(), Preset::Recommended);
    }

    #[test]
    fn preset_comes_from_config() {
        let session = Session::from_config_str(".", "preset = \"strict\"").expect("parse");
        assert_eq!(session.preset(), Preset::Strict);
    }

    #[test]
    fn with_preset_overrides_config() {
        let session = Session::from_config_str(".", "preset = \"minimal\"")
            .expect("parse")
            .with_preset(Preset::Strict);
        assert_eq!(session.preset(), Preset::Strict);
    }

    #[test]
    fn unknown_preset_is_rejected() {
        let err = match Session::from_config_str(".", "preset = \"nonexistent\"") {
            Ok(_) => panic!("must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown preset"), "{err}");
    }

    #[test]
    fn fail_on_defaults_to_error() {
        let session = Session::from_config_str(".", "").expect("defaults");
        assert_eq!(session.fail_on(), Severity::Error);
    }

    #[test]
    fn fail_on_comes_from_config() {
        let session = Session::from_config_str(".", "fail_on = \"warning\"").expect("parse");
        assert_eq!(session.fail_on(), Severity::Warning);
    }

    #[test]
    fn with_fail_on_overrides_config() {
        let session = Session::from_config_str(".", "fail_on = \"info\"")
            .expect("parse")
            .with_fail_on(Severity::Warning);
        assert_eq!(session.fail_on(), Severity::Warning);
    }

    #[test]
    fn with_fail_on_error_overrides_config_warning() {
        let session = Session::from_config_str(".", "fail_on = \"warning\"")
            .expect("parse")
            .with_fail_on(Severity::Error);
        assert_eq!(session.fail_on(), Severity::Error);
    }

    #[test]
    fn unknown_fail_on_is_rejected() {
        let err = match Session::from_config_str(".", "fail_on = \"critical\"") {
            Ok(_) => panic!("must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown severity"), "{err}");
    }

    // ── Declarative rules ──

    #[test]
    fn declarative_empty_content() {
        let session = Session::from_config_str(".", "").expect("defaults");
        assert!(session.declarative.is_empty());
    }

    #[test]
    fn declarative_no_sections() {
        let session =
            Session::from_config_str(".", "preset = \"recommended\"\nfail_on = \"error\"")
                .expect("parse");
        assert!(session.declarative.is_empty());
    }

    #[test]
    fn declarative_creates_restrict_use_rule() {
        let toml = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx"
scope = "domain"
deny = ["sqlx::*"]
message = "No DB in domain."
"#;
        let session = Session::from_config_str(".", toml).expect("parse");
        assert_eq!(session.declarative.len(), 1);
        assert_eq!(session.declarative[0].name(), "restrict-use");
    }

    #[test]
    fn declarative_creates_all_rule_types() {
        let toml = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[restrict-use]]
name = "no-sqlx"
scope = "domain"
deny = ["sqlx::*"]
message = "No DB."

[[require-use]]
name = "prefer-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing."

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
"#;
        let session = Session::from_config_str(".", toml).expect("parse");
        assert_eq!(session.declarative.len(), 3);

        let names: Vec<&str> = session.declarative.iter().map(|r| r.name()).collect();
        assert!(names.contains(&"restrict-use"));
        assert!(names.contains(&"require-use"));
        assert!(names.contains(&"deny-scope-dep"));
    }

    #[test]
    fn config_keeps_preset_alongside_declarative_sections() {
        let toml = r#"
preset = "minimal"

[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx"
scope = "domain"
deny = ["sqlx::*"]
message = "No DB."
"#;
        let session = Session::from_config_str(".", toml).expect("parse");
        assert_eq!(session.config().preset.as_deref(), Some("minimal"));
        assert_eq!(session.preset(), Preset::Minimal);
    }

    #[test]
    fn invalid_declarative_section_is_rejected() {
        let toml = r#"
[[scopes]]
name = "d"
paths = ["src/**"]

[[restrict-use]]
name = "x"
scope = "d"
denyy = ["sqlx::*"]
deny = ["sqlx::*"]
message = "m"
"#;
        let err = match Session::from_config_str(".", toml) {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("Invalid declarative rules"));
        let source = std::error::Error::source(&err).expect("source preserved");
        assert!(source.to_string().contains("denyy"), "{source}");
    }

    #[test]
    fn rule_table_outside_the_preset_is_still_validated() {
        // `handler-complexity` is not in `minimal`, but its table is checked.
        let toml = "preset = \"minimal\"\n\n[rules.handler-complexity]\nmax_match_arm = 3\n";
        let err = match Session::from_config_str(".", toml) {
            Ok(_) => panic!("a typo in any rule table must fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("handler-complexity"), "{err}");
        assert!(err.to_string().contains("max_match_arm"), "{err}");
    }

    #[test]
    fn tables_of_unknown_rules_are_accepted() {
        // Custom rules keep their own option keys.
        let toml = "[rules.required-crate]\nenabled = true\ncrate_name = \"tracing\"\n";
        let session = Session::from_config_str(".", toml).expect("custom rule table is accepted");
        assert!(session.config().is_rule_enabled("required-crate"));
    }

    // ── Rule filter ──

    #[test]
    fn rule_filter_accepts_names_and_codes() {
        let session = Session::from_config_str(".", "")
            .expect("defaults")
            .with_rule_filter(&["AL001", "require-doc-comments"])
            .expect("valid");
        let rules = session.rules.as_ref().expect("explicit rules");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].code(), "AL012");
    }

    #[test]
    fn rule_filter_rejects_unknown() {
        let err = match Session::from_config_str(".", "")
            .expect("defaults")
            .with_rule_filter(&["AL001", "bogus"])
        {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("bogus"), "{err}");
    }

    // ── Config source / IO ──

    #[test]
    fn from_config_path_reports_missing_file() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("nope.toml");
        let err = match Session::from_config_path(tmp.path(), &missing) {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("Failed to read config file"),
            "{err}"
        );
    }

    #[test]
    fn discover_prefers_project_config_over_global() {
        let project = TempDir::new().unwrap();
        fs::write(
            project.path().join("arch-lint.toml"),
            "preset = \"minimal\"",
        )
        .unwrap();

        let session = Session::discover(project.path()).expect("discover");
        assert_eq!(session.preset(), Preset::Minimal);
        assert_eq!(
            session.config_source(),
            &ConfigSource::Project(project.path().join("arch-lint.toml"))
        );
        assert_eq!(session.root(), project.path());
    }

    #[test]
    fn from_config_str_records_default_source() {
        let session = Session::from_config_str(".", "preset = \"strict\"").expect("parse");
        assert_eq!(session.config_source(), &ConfigSource::Default);
    }

    // ── Workspace root walk ──

    #[test]
    fn find_workspace_root_walks_up_to_workspace_manifest() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\"]\n",
        )
        .unwrap();
        let member = tmp.path().join("crates/a");
        fs::create_dir_all(&member).unwrap();
        fs::write(member.join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();

        assert_eq!(Session::find_workspace_root(&member), tmp.path());
    }

    #[test]
    fn find_workspace_root_falls_back_to_start() {
        let tmp = TempDir::new().unwrap();
        let member = tmp.path().join("solo");
        fs::create_dir_all(&member).unwrap();
        fs::write(member.join("Cargo.toml"), "[package]\nname = \"solo\"\n").unwrap();

        assert_eq!(Session::find_workspace_root(&member), member);
    }

    #[test]
    fn find_workspace_root_ignores_commented_workspace_key() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("pkg");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "# [workspace]\n[package]\nname = \"pkg\"\n",
        )
        .unwrap();

        assert_eq!(Session::find_workspace_root(&dir), dir);
    }
}
