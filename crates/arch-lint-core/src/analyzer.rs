//! Core analyzer for orchestrating lint execution.

use crate::config::{Config, RuleConfig};
use crate::context::{FileContext, ProjectContext};
use crate::rule::{ProjectRule, ProjectRuleBox, Rule, RuleBox};
use crate::types::{LintResult, Violation};

use std::path::{Path, PathBuf};
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
    config: Option<Config>,
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
    #[must_use]
    pub fn include(mut self, pattern: impl Into<String>) -> Self {
        self.include_patterns.push(pattern.into());
        self
    }

    /// Sets the configuration.
    #[must_use]
    pub fn config(mut self, config: Config) -> Self {
        self.config = Some(config);
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

        Ok(Analyzer {
            root,
            rules: self.rules,
            project_rules: self.project_rules,
            exclude_patterns,
            include_patterns: self.include_patterns,
            config: self.config.unwrap_or_default(),
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
    #[allow(dead_code)] // Reserved for future include pattern support
    include_patterns: Vec<String>,
    config: Config,
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

        info!("Found {} files to analyze", files.len());

        // Run per-file rules
        for file_path in &files {
            match self.analyze_file(file_path) {
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
            .with_cargo_files(self.discover_cargo_files()?);

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

    /// Analyzes a single file and returns violations.
    fn analyze_file(&self, path: &Path) -> Result<Vec<Violation>, AnalyzerError> {
        debug!("Analyzing: {}", path.display());

        let content = std::fs::read_to_string(path)?;
        let ast = syn::parse_file(&content).map_err(|e| AnalyzerError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let ctx = FileContext::new(path, &content, &self.root);
        let mut violations = Vec::new();

        for rule in &self.rules {
            if !self.config.is_rule_enabled(rule.name()) {
                debug!("Skipping disabled rule: {}", rule.name());
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

    /// Discovers all Rust source files to analyze.
    fn discover_files(&self) -> Result<Vec<PathBuf>, AnalyzerError> {
        let pattern = format!("{}/**/*.rs", self.root.display());
        let mut files = Vec::new();

        for entry in glob::glob(&pattern)? {
            let path = entry.map_err(|e| AnalyzerError::Io(e.into_error()))?;

            // Check exclude patterns
            if self.should_exclude(&path) {
                debug!("Excluding: {}", path.display());
                continue;
            }

            files.push(path);
        }

        Ok(files)
    }

    /// Discovers Cargo.toml files in the project.
    fn discover_cargo_files(&self) -> Result<Vec<PathBuf>, AnalyzerError> {
        let pattern = format!("{}/**/Cargo.toml", self.root.display());
        let mut files = Vec::new();

        for entry in glob::glob(&pattern)? {
            let path = entry.map_err(|e| AnalyzerError::Io(e.into_error()))?;
            if !self.should_exclude(&path) {
                files.push(path);
            }
        }

        Ok(files)
    }

    /// Checks if a path should be excluded.
    fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        for pattern in &self.exclude_patterns {
            // Simple glob matching
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
}
