//! Core analyzer for orchestrating lint execution.

use crate::config::{Config, RuleConfig};
use crate::context::{FileContext, ProjectContext};
use crate::crate_roots::{manifest_declares_package, CrateRoots};
use crate::declarative::graph::ScopeGraph;
use crate::declarative::model::DeclarativeConfig;
use crate::declarative::rules::ScopeEdgeCollector;
use crate::rule::{ProjectRule, ProjectRuleBox, Rule, RuleBox};
use crate::types::{LintResult, Violation};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during analysis.
#[derive(Debug, Error)]
pub enum AnalyzerError {
    /// IO error reading files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Error parsing Rust source file.
    #[error("Parse error in {path}: {message}")]
    Parse {
        /// Path to the file that failed to parse.
        path: PathBuf,
        /// Parse error message.
        message: String,
    },

    /// Glob pattern error.
    #[error("Invalid glob pattern: {0}")]
    Glob(#[from] glob::PatternError),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),
}

/// Builder for configuring an [`Analyzer`].
#[derive(Default)]
pub struct AnalyzerBuilder {
    root: Option<PathBuf>,
    rules: Vec<RuleBox>,
    project_rules: Vec<ProjectRuleBox>,
    exclude_patterns: Vec<String>,
    include_patterns: Vec<String>,
    respect_gitignore: Option<bool>,
    config: Option<Config>,
    declarative: Option<Arc<DeclarativeConfig>>,
    fail_on_parse_error: bool,
}

impl AnalyzerBuilder {
    /// Creates a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the root directory to analyze.
    #[must_use]
    pub fn root(mut self, path: impl Into<PathBuf>) -> Self {
        self.root = Some(path.into());
        self
    }

    /// Adds a per-file rule to the analyzer.
    #[must_use]
    pub fn rule<R: Rule + 'static>(mut self, rule: R) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    /// Adds a boxed per-file rule to the analyzer.
    #[must_use]
    pub fn rule_box(mut self, rule: RuleBox) -> Self {
        self.rules.push(rule);
        self
    }

    /// Adds a project-wide rule to the analyzer.
    #[must_use]
    pub fn project_rule<R: ProjectRule + 'static>(mut self, rule: R) -> Self {
        self.project_rules.push(Box::new(rule));
        self
    }

    /// Adds a boxed project-wide rule to the analyzer.
    #[must_use]
    pub fn project_rule_box(mut self, rule: ProjectRuleBox) -> Self {
        self.project_rules.push(rule);
        self
    }

    /// Adds an exclude glob pattern.
    #[must_use]
    pub fn exclude(mut self, pattern: impl Into<String>) -> Self {
        self.exclude_patterns.push(pattern.into());
        self
    }

    /// Adds multiple exclude glob patterns.
    #[must_use]
    pub fn excludes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.exclude_patterns
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    /// Adds an include glob pattern.
    ///
    /// Patterns are matched against each file's path *relative to the analyzer
    /// root* (a leading `./` is accepted, and an absolute pattern is matched
    /// against the absolute path as well). When no include pattern is set —
    /// neither here nor via [`AnalyzerBuilder::config`] — every `*.rs` file
    /// under the root is analyzed.
    ///
    /// Patterns added here are merged with `[analyzer] include` from the
    /// configuration, the same way [`AnalyzerBuilder::exclude`] merges with
    /// `[analyzer] exclude`.
    #[must_use]
    pub fn include(mut self, pattern: impl Into<String>) -> Self {
        self.include_patterns.push(pattern.into());
        self
    }

    /// Adds multiple include glob patterns.
    ///
    /// See [`AnalyzerBuilder::include`] for the matching semantics.
    #[must_use]
    pub fn includes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.include_patterns
            .extend(patterns.into_iter().map(Into::into));
        self
    }

    /// Sets whether file discovery honors `.gitignore` (default: `true`).
    ///
    /// Calling this method takes precedence over `[analyzer] respect_gitignore`
    /// from the configuration; when it is not called, the configuration value
    /// (which itself defaults to `true`) is used.
    #[must_use]
    pub fn respect_gitignore(mut self, respect: bool) -> Self {
        self.respect_gitignore = Some(respect);
        self
    }

    /// Sets the configuration.
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Sets the declarative config whose `[[scopes]]` define the
    /// [`ScopeGraph`].
    ///
    /// With it, the analyzer records every scope-crossing reference while it
    /// runs the per-file rules and hands the graph to project rules through
    /// [`ProjectContext::scope_graph`]. Without it the graph is empty. Pass
    /// the same `Arc` the declarative rules were built from
    /// ([`create_rules_shared`](crate::declarative::create_rules_shared)).
    #[must_use]
    pub fn declarative(mut self, config: Arc<DeclarativeConfig>) -> Self {
        self.declarative = Some(config);
        self
    }

    /// Sets whether to fail on parse errors (default: false).
    #[must_use]
    pub fn fail_on_parse_error(mut self, fail: bool) -> Self {
        self.fail_on_parse_error = fail;
        self
    }

    /// Builds the analyzer.
    ///
    /// # Errors
    ///
    /// Returns an error if the root directory doesn't exist.
    pub fn build(self) -> Result<Analyzer, AnalyzerError> {
        let root = self
            .root
            .or_else(|| self.config.as_ref().map(|c| c.analyzer.root.clone()))
            .unwrap_or_else(|| PathBuf::from("."));

        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()?.join(&root)
        };

        // Merge exclude patterns from config
        let mut exclude_patterns = self.exclude_patterns;
        if let Some(ref config) = self.config {
            exclude_patterns.extend(config.analyzer.exclude.clone());
        }

        // Add default excludes if none specified
        if exclude_patterns.is_empty() {
            exclude_patterns.extend(["**/target/**".to_string(), "**/vendor/**".to_string()]);
        }

        // Merge include patterns from config (same flow as excludes)
        let mut include_patterns = self.include_patterns;
        if let Some(ref config) = self.config {
            include_patterns.extend(config.analyzer.include.clone());
        }

        // An explicit builder call wins over the config value.
        let respect_gitignore = match self.respect_gitignore {
            Some(explicit) => explicit,
            None => self
                .config
                .as_ref()
                .is_none_or(|c| c.analyzer.respect_gitignore),
        };

        Ok(Analyzer {
            root,
            rules: self.rules,
            project_rules: self.project_rules,
            exclude_patterns,
            include_patterns,
            respect_gitignore,
            config: self.config.unwrap_or_default(),
            declarative: self.declarative,
            fail_on_parse_error: self.fail_on_parse_error,
        })
    }
}

/// The main analyzer that orchestrates lint execution.
///
/// Use [`Analyzer::builder()`] to construct an instance.
pub struct Analyzer {
    root: PathBuf,
    rules: Vec<RuleBox>,
    project_rules: Vec<ProjectRuleBox>,
    exclude_patterns: Vec<String>,
    include_patterns: Vec<String>,
    respect_gitignore: bool,
    config: Config,
    declarative: Option<Arc<DeclarativeConfig>>,
    fail_on_parse_error: bool,
}

impl Analyzer {
    /// Creates a new builder for configuring an analyzer.
    #[must_use]
    pub fn builder() -> AnalyzerBuilder {
        AnalyzerBuilder::new()
    }

    /// Returns the root directory being analyzed.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the number of registered rules.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len() + self.project_rules.len()
    }

    /// Analyzes all files and returns the results.
    ///
    /// # Errors
    ///
    /// Returns an error if file discovery or parsing fails.
    pub fn analyze(&self) -> Result<LintResult, AnalyzerError> {
        info!("Starting analysis at {:?}", self.root);

        let mut result = LintResult::new();
        let files = self.discover_files()?;
        let cargo_files = self.discover_cargo_files()?;
        let roots = self.crate_roots(&cargo_files);

        info!("Found {} files to analyze", files.len());

        // Run per-file rules; the scope graph is collected on the same pass.
        let mut graph = self.empty_graph();
        for file_path in &files {
            match self.analyze_file(file_path, &roots, &mut graph) {
                Ok(violations) => {
                    result.violations.extend(violations);
                    result.files_checked += 1;
                }
                Err(AnalyzerError::Parse { path, message }) => {
                    warn!("Failed to parse {}: {}", path.display(), message);
                    if self.fail_on_parse_error {
                        return Err(AnalyzerError::Parse { path, message });
                    }
                }
                Err(e) => return Err(e),
            }
        }

        // Run project-wide rules
        let project_ctx = ProjectContext::new(&self.root)
            .with_source_files(files.clone())
            .with_cargo_files(cargo_files)
            .with_scope_graph(graph);

        for rule in &self.project_rules {
            if !self.config.is_rule_enabled(rule.name()) {
                debug!("Skipping disabled rule: {}", rule.name());
                continue;
            }

            let violations = rule.check_project(&project_ctx);
            let violations = self.apply_severity_override(rule.name(), violations);
            result.violations.extend(violations);
        }

        // Sort violations by file, then line
        result.violations.sort_by(|a, b| {
            a.location
                .file
                .cmp(&b.location.file)
                .then(a.location.line.cmp(&b.location.line))
                .then(a.location.column.cmp(&b.location.column))
        });

        info!(
            "Analysis complete: {} violations in {} files",
            result.violations.len(),
            result.files_checked
        );

        Ok(result)
    }

    /// Builds the [`ScopeGraph`] of the project without running any rule.
    ///
    /// Discovers files exactly as [`Analyzer::analyze`] does and collects the
    /// scope-crossing references of each. The result equals the graph that
    /// `analyze()` hands to project rules. Files that fail to parse are
    /// skipped with a warning (or fail the call when
    /// [`AnalyzerBuilder::fail_on_parse_error`] is set), as in `analyze()`.
    ///
    /// # Errors
    ///
    /// Returns an error if file discovery or reading fails.
    pub fn scope_graph(&self) -> Result<ScopeGraph, AnalyzerError> {
        let mut graph = self.empty_graph();
        let Some(declarative) = &self.declarative else {
            return Ok(graph);
        };

        let roots = self.crate_roots(&self.discover_cargo_files()?);
        for path in &self.discover_files()? {
            match Self::read_and_parse(path) {
                Ok((content, ast)) => {
                    let ctx = FileContext::new(path, &content, &self.root).with_crate_roots(&roots);
                    ScopeEdgeCollector::collect(declarative, &ctx, &ast, &mut graph);
                }
                Err(AnalyzerError::Parse { path, message }) => {
                    warn!("Failed to parse {}: {}", path.display(), message);
                    if self.fail_on_parse_error {
                        return Err(AnalyzerError::Parse { path, message });
                    }
                }
                Err(e) => return Err(e),
            }
        }

        Ok(graph)
    }

    /// A graph with every declared scope as a node and no edges.
    fn empty_graph(&self) -> ScopeGraph {
        match &self.declarative {
            Some(config) => ScopeGraph::with_nodes(config.scopes().keys().cloned()),
            None => ScopeGraph::new(),
        }
    }

    /// Reads and parses one source file.
    fn read_and_parse(path: &Path) -> Result<(String, syn::File), AnalyzerError> {
        let content = std::fs::read_to_string(path)?;
        let ast = syn::parse_file(&content).map_err(|e| AnalyzerError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok((content, ast))
    }

    /// Finds the packages among `cargo_files`: the manifests that have a
    /// `[package]` table, each as its root-relative directory.
    ///
    /// A virtual manifest (`[workspace]` only) is not a package. A manifest
    /// that cannot be read or parsed is skipped with a warning, and its files
    /// fall back to the enclosing package (or the analyzer root), as they do
    /// when the manifest is excluded from discovery.
    fn crate_roots(&self, cargo_files: &[PathBuf]) -> CrateRoots {
        let mut dirs = Vec::new();
        for manifest in cargo_files {
            let content = match std::fs::read_to_string(manifest) {
                Ok(content) => content,
                Err(e) => {
                    warn!("Failed to read {}: {}", manifest.display(), e);
                    continue;
                }
            };
            match manifest_declares_package(&content) {
                Ok(true) => {
                    let dir = manifest.parent().unwrap_or(manifest);
                    dirs.push(self.relative_to_root(dir).to_path_buf());
                }
                Ok(false) => debug!("Not a package manifest: {}", manifest.display()),
                Err(e) => warn!("Failed to parse {}: {}", manifest.display(), e),
            }
        }
        CrateRoots::new(dirs)
    }

    /// Analyzes a single file and returns violations, recording its
    /// scope-crossing references into `graph`.
    fn analyze_file(
        &self,
        path: &Path,
        roots: &CrateRoots,
        graph: &mut ScopeGraph,
    ) -> Result<Vec<Violation>, AnalyzerError> {
        debug!("Analyzing: {}", path.display());

        let (content, ast) = Self::read_and_parse(path)?;
        let ctx = FileContext::new(path, &content, &self.root).with_crate_roots(roots);

        if let Some(declarative) = &self.declarative {
            ScopeEdgeCollector::collect(declarative, &ctx, &ast, graph);
        }

        let mut violations = Vec::new();

        for rule in &self.rules {
            if !self.config.is_rule_enabled(rule.name()) {
                debug!("Skipping disabled rule: {}", rule.name());
                continue;
            }

            if self
                .config
                .rules
                .get(rule.name())
                .is_some_and(|c| c.excludes_file(&ctx.relative_path))
            {
                debug!(
                    "Skipping rule {} for excluded file: {}",
                    rule.name(),
                    ctx.relative_path.display()
                );
                continue;
            }

            let rule_violations = rule.check(&ctx, &ast);
            let rule_violations = self.apply_severity_override(rule.name(), rule_violations);
            violations.extend(rule_violations);
        }

        Ok(violations)
    }

    /// Applies severity overrides from configuration.
    fn apply_severity_override(
        &self,
        rule_name: &str,
        mut violations: Vec<Violation>,
    ) -> Vec<Violation> {
        if let Some(severity) = self.config.rule_severity(rule_name) {
            for v in &mut violations {
                v.severity = severity;
            }
        }
        violations
    }

    /// Builds the directory walker used by both discovery passes.
    ///
    /// Visibility rules are chosen to stay compatible with the previous
    /// `glob("<root>/**/*.rs")` implementation: the `glob` crate's default
    /// [`MatchOptions`](glob::MatchOptions) has `require_literal_leading_dot =
    /// false`, so hidden directories and dotfiles *were* traversed. The walker
    /// therefore sets `hidden(false)` (i.e. "do not skip hidden entries").
    ///
    /// When `respect_gitignore` is on, only the git ignore sources are
    /// consulted — `.gitignore` at any level, `.git/info/exclude`, and the
    /// global gitignore — with `require_git(false)` so that a plain directory
    /// carrying a `.gitignore` (a scaffolded project, a test fixture) is
    /// filtered the same way a checked-out repository is. Non-git ignore files
    /// (`.ignore`, `.rgignore`) are never honored.
    fn walker(&self) -> ignore::Walk {
        let mut builder = ignore::WalkBuilder::new(&self.root);
        builder
            .hidden(false)
            .ignore(false)
            .git_ignore(self.respect_gitignore)
            .git_global(self.respect_gitignore)
            .git_exclude(self.respect_gitignore)
            .parents(self.respect_gitignore)
            .require_git(false);
        builder.build()
    }

    /// Walks the root and collects files accepted by `keep`.
    ///
    /// Results are sorted so that discovery order does not depend on the
    /// order the filesystem hands directory entries back.
    fn walk_files<F>(&self, keep: F) -> Result<Vec<PathBuf>, AnalyzerError>
    where
        F: Fn(&Path) -> bool,
    {
        let mut files = Vec::new();

        for entry in self.walker() {
            let entry = entry.map_err(|e| {
                let message = e.to_string();
                match e.into_io_error() {
                    Some(io) => AnalyzerError::Io(io),
                    None => AnalyzerError::Io(std::io::Error::other(message)),
                }
            })?;

            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = entry.into_path();
            if keep(&path) {
                files.push(path);
            }
        }

        files.sort();
        Ok(files)
    }

    /// Returns `path` relative to the analyzer root, when it is under it.
    fn relative_to_root<'a>(&self, path: &'a Path) -> &'a Path {
        match path.strip_prefix(&self.root) {
            Ok(relative) => relative,
            Err(_) => path,
        }
    }

    /// Matches a glob pattern against both the root-relative and the absolute
    /// form of `path`.
    ///
    /// A leading `./` on the pattern is stripped so that `./src/**` and
    /// `src/**` behave identically; matching the absolute form additionally
    /// lets patterns be written as absolute paths.
    fn matches_pattern(&self, pattern: &str, path: &Path) -> bool {
        let normalized = match pattern.strip_prefix("./") {
            Some(stripped) => stripped,
            None => pattern,
        };
        let Ok(glob_pattern) = glob::Pattern::new(normalized) else {
            return false;
        };
        glob_pattern.matches_path(self.relative_to_root(path)) || glob_pattern.matches_path(path)
    }

    /// Discovers all Rust source files to analyze.
    ///
    /// Ordered steps: walk the root (honoring `.gitignore` when
    /// `respect_gitignore` is set), keep `*.rs` files, apply `include`
    /// patterns when any are configured, then drop anything matching an
    /// `exclude` pattern.
    fn discover_files(&self) -> Result<Vec<PathBuf>, AnalyzerError> {
        self.walk_files(|path| {
            if path.extension().is_none_or(|ext| ext != "rs") {
                return false;
            }

            if !self.should_include(path) {
                debug!("Not included: {}", path.display());
                return false;
            }

            if self.should_exclude(path) {
                debug!("Excluding: {}", path.display());
                return false;
            }

            true
        })
    }

    /// Discovers Cargo.toml files in the project.
    ///
    /// Follows the same `respect_gitignore` setting and the same `exclude`
    /// patterns as [`Analyzer::discover_files`]. `include` is deliberately not
    /// applied here: it scopes the *source files* that are linted, and
    /// filtering manifests by it would hide crates from project-wide rules.
    fn discover_cargo_files(&self) -> Result<Vec<PathBuf>, AnalyzerError> {
        self.walk_files(|path| {
            path.file_name().is_some_and(|name| name == "Cargo.toml") && !self.should_exclude(path)
        })
    }

    /// Checks if a path is covered by the configured include patterns.
    ///
    /// An empty include list means "everything".
    fn should_include(&self, path: &Path) -> bool {
        self.include_patterns.is_empty()
            || self
                .include_patterns
                .iter()
                .any(|pattern| self.matches_pattern(pattern, path))
    }

    /// Checks if a path should be excluded.
    fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        for pattern in &self.exclude_patterns {
            // Glob matching against the root-relative and the absolute path
            if self.matches_pattern(pattern, path) {
                return true;
            }

            // Legacy: match the absolute path as a plain string, so that
            // patterns relying on `*` crossing separators keep working
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                if glob_pattern.matches(&path_str) {
                    return true;
                }
            }

            // Also check as substring for patterns like "**/target/**"
            let normalized_pattern = pattern.replace("**", "");
            if !normalized_pattern.is_empty() && path_str.contains(&normalized_pattern) {
                return true;
            }
        }

        false
    }

    /// Gets the rule configuration for a specific rule.
    #[must_use]
    pub fn rule_config(&self, rule_name: &str) -> Option<&RuleConfig> {
        self.config.rules.get(rule_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        let analyzer = Analyzer::builder()
            .root(".")
            .exclude("**/target/**")
            .build()
            .expect("Failed to build analyzer");

        assert!(analyzer.root().exists());
    }

    #[test]
    fn test_exclude_patterns() {
        let analyzer = Analyzer::builder()
            .root(".")
            .exclude("**/target/**")
            .exclude("**/vendor/**")
            .build()
            .expect("Failed to build analyzer");

        assert!(analyzer.should_exclude(Path::new("/foo/target/debug/main.rs")));
        assert!(analyzer.should_exclude(Path::new("/foo/vendor/lib.rs")));
        assert!(!analyzer.should_exclude(Path::new("/foo/src/lib.rs")));
    }

    /// Writes `contents` to `root/rel`, creating parent directories.
    fn write_file(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create fixture directory");
        }
        std::fs::write(&path, contents).expect("Failed to write fixture file");
    }

    /// Returns discovered files as root-relative, slash-separated strings.
    fn discovered(analyzer: &Analyzer) -> Vec<String> {
        analyzer
            .discover_files()
            .expect("Discovery failed")
            .iter()
            .map(|p| {
                p.strip_prefix(analyzer.root())
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    /// A tempdir with a `.gitignore` covering `gen/`, one kept and one ignored file.
    fn gitignore_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        write_file(root, ".gitignore", "gen/\n");
        write_file(root, "src/app/kept.rs", "pub fn kept() {}\n");
        write_file(root, "gen/ignored.rs", "pub fn ignored() {}\n");
        dir
    }

    #[test]
    fn test_gitignore_respected_by_default() {
        let dir = gitignore_fixture();

        let analyzer = Analyzer::builder()
            .root(dir.path())
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(discovered(&analyzer), vec!["src/app/kept.rs".to_string()]);
    }

    #[test]
    fn test_gitignore_ignored_when_disabled() {
        let dir = gitignore_fixture();

        let analyzer = Analyzer::builder()
            .root(dir.path())
            .respect_gitignore(false)
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(
            discovered(&analyzer),
            vec!["gen/ignored.rs".to_string(), "src/app/kept.rs".to_string()]
        );
    }

    #[test]
    fn test_respect_gitignore_flows_from_config() {
        let dir = gitignore_fixture();

        let mut config = Config::default();
        config.analyzer.respect_gitignore = false;

        let analyzer = Analyzer::builder()
            .root(dir.path())
            .config(config)
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(
            discovered(&analyzer),
            vec!["gen/ignored.rs".to_string(), "src/app/kept.rs".to_string()]
        );
    }

    #[test]
    fn test_include_restricts_to_matching_files() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        write_file(root, "src/app/handler.rs", "pub fn handler() {}\n");
        write_file(root, "src/infra/db.rs", "pub fn db() {}\n");
        write_file(root, "tests/it.rs", "pub fn it() {}\n");

        let analyzer = Analyzer::builder()
            .root(root)
            .include("src/app/**")
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(
            discovered(&analyzer),
            vec!["src/app/handler.rs".to_string()]
        );
    }

    #[test]
    fn test_include_flows_from_config_and_accepts_leading_dot_slash() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        write_file(root, "src/app/handler.rs", "pub fn handler() {}\n");
        write_file(root, "src/infra/db.rs", "pub fn db() {}\n");

        let mut config = Config::default();
        config.analyzer.include = vec!["./src/infra/**".to_string()];

        let analyzer = Analyzer::builder()
            .root(root)
            .config(config)
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(discovered(&analyzer), vec!["src/infra/db.rs".to_string()]);
    }

    #[test]
    fn test_relative_exclude_pattern() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        write_file(root, "src/lib.rs", "pub fn lib() {}\n");
        write_file(root, "src/generated/schema.rs", "pub fn schema() {}\n");

        let analyzer = Analyzer::builder()
            .root(root)
            .exclude("src/generated/**")
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(discovered(&analyzer), vec!["src/lib.rs".to_string()]);
        assert!(analyzer.should_exclude(&root.join("src/generated/schema.rs")));
        assert!(!analyzer.should_exclude(&root.join("src/lib.rs")));
    }

    #[test]
    fn test_discovery_outside_git_repository() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        assert!(!root.join(".git").exists());
        write_file(root, "src/lib.rs", "pub fn lib() {}\n");
        write_file(root, "src/nested/deep/mod.rs", "pub fn deep() {}\n");
        write_file(root, "Cargo.toml", "[package]\nname = \"fixture\"\n");

        let analyzer = Analyzer::builder()
            .root(root)
            .build()
            .expect("Failed to build analyzer");

        assert_eq!(
            discovered(&analyzer),
            vec![
                "src/lib.rs".to_string(),
                "src/nested/deep/mod.rs".to_string()
            ]
        );

        let cargo_files = analyzer
            .discover_cargo_files()
            .expect("Cargo discovery failed");
        assert_eq!(cargo_files, vec![root.join("Cargo.toml")]);
    }

    #[test]
    fn test_discover_cargo_files_follows_gitignore() {
        let dir = tempfile::tempdir().expect("Failed to create tempdir");
        let root = dir.path();
        write_file(root, ".gitignore", "gen/\n");
        write_file(root, "Cargo.toml", "[package]\nname = \"fixture\"\n");
        write_file(root, "gen/Cargo.toml", "[package]\nname = \"gen\"\n");

        let respecting = Analyzer::builder()
            .root(root)
            .build()
            .expect("Failed to build analyzer");
        assert_eq!(
            respecting
                .discover_cargo_files()
                .expect("Cargo discovery failed"),
            vec![root.join("Cargo.toml")]
        );

        let ignoring = Analyzer::builder()
            .root(root)
            .respect_gitignore(false)
            .build()
            .expect("Failed to build analyzer");
        assert_eq!(
            ignoring
                .discover_cargo_files()
                .expect("Cargo discovery failed"),
            vec![root.join("Cargo.toml"), root.join("gen/Cargo.toml")]
        );
    }
}
