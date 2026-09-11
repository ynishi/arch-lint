//! End-to-end driver for the tree-sitter engine.
//!
//! A [`TsSession`] takes a validated [`ArchConfig`] and a project root,
//! discovers the source files its extractors understand, runs the
//! [`ArchRuleEngine`] over each one and returns a single
//! [`LintResult`].
//!
//! ```no_run
//! use arch_lint_ts::{ArchConfig, TsSession};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ArchConfig::from_file(Path::new("arch-lint.toml"))?;
//! let result = TsSession::new(config, ".").run()?;
//! println!("{} violations", result.violations.len());
//! # Ok(())
//! # }
//! ```

use arch_lint_core::LintResult;
use std::path::{Path, PathBuf};

use crate::config::{ArchConfig, ConfigError};
use crate::engine::ArchRuleEngine;
use crate::extractor::{ExtractError, LanguageExtractor};
use crate::kotlin::KotlinExtractor;

/// Everything that can go wrong during a [`TsSession::run`].
#[derive(Debug, thiserror::Error)]
pub enum TsSessionError {
    /// The configuration is structurally invalid.
    #[error("invalid configuration")]
    Config(#[from] ConfigError),

    /// A source file could not be walked or read.
    #[error("failed to read {path}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// The directory walk itself failed.
    #[error("failed to walk {root}")]
    Walk {
        /// Root the walk started from.
        root: PathBuf,
        /// Underlying walk error.
        #[source]
        source: ignore::Error,
    },

    /// A source file could not be parsed by its extractor.
    #[error("failed to analyze {path}")]
    Extract {
        /// Path that failed.
        path: PathBuf,
        /// Underlying extractor error.
        #[source]
        source: ExtractError,
    },
}

/// One configured tree-sitter run.
///
/// The default extractor registry contains [`KotlinExtractor`]; add more with
/// [`TsSession::with_extractor`].
pub struct TsSession {
    config: ArchConfig,
    root: PathBuf,
    extractors: Vec<Box<dyn LanguageExtractor>>,
    respect_gitignore: Option<bool>,
}

impl std::fmt::Debug for TsSession {
    /// Extractors are boxed trait objects, so they are summarized by count.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsSession")
            .field("root", &self.root)
            .field("layers", &self.config.layers.len())
            .field("extractors", &self.extractors.len())
            .field("respect_gitignore", &self.respect_gitignore)
            .finish()
    }
}

impl TsSession {
    /// Creates a session for `config`, analyzing files below `root`.
    ///
    /// `root` is the project directory. The effective analysis root is
    /// `config.root` when that is absolute, and `root.join(config.root)`
    /// otherwise.
    #[must_use]
    pub fn new(config: ArchConfig, root: impl AsRef<Path>) -> Self {
        Self {
            config,
            root: root.as_ref().to_path_buf(),
            extractors: vec![Box::new(KotlinExtractor::new())],
            respect_gitignore: None,
        }
    }

    /// Adds a language extractor to the registry.
    ///
    /// The built-in Kotlin extractor stays registered; a file is handed to the
    /// first extractor that claims its extension.
    #[must_use]
    pub fn with_extractor(mut self, extractor: Box<dyn LanguageExtractor>) -> Self {
        self.extractors.push(extractor);
        self
    }

    /// Overrides `[analyzer] respect_gitignore` from the configuration file.
    #[must_use]
    pub fn respect_gitignore(mut self, respect: bool) -> Self {
        self.respect_gitignore = Some(respect);
        self
    }

    /// The directory this session analyzes, with `config.root` applied.
    #[must_use]
    pub fn analysis_root(&self) -> PathBuf {
        if self.config.root.is_absolute() {
            self.config.root.clone()
        } else {
            self.root.join(&self.config.root)
        }
    }

    /// Validates the configuration, analyzes every discovered file and
    /// returns the combined result.
    ///
    /// Violations are sorted by file path, then by line.
    ///
    /// # Errors
    ///
    /// Returns [`TsSessionError::Config`] if the configuration does not
    /// validate, [`TsSessionError::Walk`] or [`TsSessionError::Io`] if files
    /// cannot be discovered or read, and [`TsSessionError::Extract`] if a file
    /// cannot be parsed.
    pub fn run(&self) -> Result<LintResult, TsSessionError> {
        self.config.validate()?;

        let root = self.analysis_root();
        let engine = ArchRuleEngine::new(self.config.clone());
        let files = self.discover_files(&root)?;

        tracing::info!("Analyzing {} files with tree-sitter engine", files.len());

        let mut result = LintResult::new();

        for file_path in &files {
            let Some(extractor) = self.extractor_for(file_path) else {
                continue;
            };

            let source = std::fs::read_to_string(file_path).map_err(|e| TsSessionError::Io {
                path: file_path.clone(),
                source: e,
            })?;

            let rel = file_path
                .strip_prefix(&root)
                .unwrap_or(file_path)
                .to_path_buf();

            let mut analysis = extractor
                .analyze(&source)
                .map_err(|e| TsSessionError::Extract {
                    path: file_path.clone(),
                    source: e,
                })?;
            analysis.file_path = rel;

            result.violations.extend(engine.check(&analysis));
            result.files_checked += 1;
        }

        result.violations.sort_by(|a, b| {
            a.location
                .file
                .cmp(&b.location.file)
                .then(a.location.line.cmp(&b.location.line))
        });

        Ok(result)
    }

    /// Returns the first extractor that claims `path`'s extension.
    fn extractor_for(&self, path: &Path) -> Option<&dyn LanguageExtractor> {
        let ext = dotted_extension(path);
        self.extractors
            .iter()
            .find(|e| e.extensions().contains(&ext.as_str()))
            .map(AsRef::as_ref)
    }

    /// Walks `root` and returns the sorted list of files to analyze.
    fn discover_files(&self, root: &Path) -> Result<Vec<PathBuf>, TsSessionError> {
        let supported_exts: Vec<&str> = self
            .extractors
            .iter()
            .flat_map(|e| e.extensions().iter().copied())
            .collect();

        let respect_gitignore = self
            .respect_gitignore
            .unwrap_or(self.config.respect_gitignore);

        // `hidden(false)` keeps dot-directories in the walk; `require_git(false)`
        // makes the ignore sources apply to a plain directory that is not a
        // git repository, matching the syn analyzer.
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            .hidden(false)
            .git_ignore(respect_gitignore)
            .git_global(respect_gitignore)
            .git_exclude(respect_gitignore)
            .parents(respect_gitignore)
            .require_git(false);

        let mut files = Vec::new();
        for entry in builder.build() {
            let entry = entry.map_err(|e| TsSessionError::Walk {
                root: root.to_path_buf(),
                source: e,
            })?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if !supported_exts.contains(&dotted_extension(path).as_str()) {
                continue;
            }

            let rel = path.strip_prefix(root).unwrap_or(path);
            if !is_excluded(rel, &self.config.exclude) {
                files.push(path.to_path_buf());
            }
        }

        files.sort();
        Ok(files)
    }
}

/// Returns the file extension with a leading dot (`".kt"`), or `""`.
fn dotted_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map_or_else(String::new, |e| format!(".{e}"))
}

/// Returns `true` if `rel` matches any exclude pattern.
///
/// A pattern is first tried as a glob against the root-relative path. When it
/// does not parse as a glob, or does not match, the historical substring
/// behaviour is used as a fallback: `**/` and `/**` are stripped and the
/// remainder is matched anywhere in the path.
fn is_excluded(rel: &Path, patterns: &[String]) -> bool {
    let rel_str = rel.to_string_lossy();

    patterns.iter().any(|pattern| {
        if glob::Pattern::new(pattern).is_ok_and(|p| p.matches_path(rel)) {
            return true;
        }
        let clean = pattern.replace("**/", "").replace("/**", "");
        !clean.is_empty() && rel_str.contains(&clean)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    const CONFIG: &str = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "infra"
packages = ["com.example.infra"]

[dependencies]
domain = []
infra = ["domain"]
"#;

    fn write(root: &Path, rel: &str, content: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("has parent")).expect("create dir");
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn run_reports_layer_violations_with_relative_paths() {
        let tmp = TempDir::new().expect("tempdir");
        write(
            tmp.path(),
            "src/domain/Order.kt",
            "package com.example.domain\n\nimport com.example.infra.OrderTable\n\nclass Order\n",
        );

        let config = ArchConfig::parse(CONFIG).expect("parse");
        let result = TsSession::new(config, tmp.path()).run().expect("run");

        assert_eq!(result.files_checked, 1);
        assert_eq!(result.violations.len(), 1);
        assert_eq!(
            result.violations[0].location.file,
            Path::new("src/domain/Order.kt")
        );
    }

    #[test]
    fn run_rejects_invalid_config() {
        let tmp = TempDir::new().expect("tempdir");
        let config = ArchConfig::parse(
            "[[layers]]\nname = \"domain\"\npackages = [\"a\"]\n\n[dependencies]\ndomain = [\"nope\"]\n",
        )
        .expect("parse");

        let err = match TsSession::new(config, tmp.path()).run() {
            Ok(_) => panic!("must fail"),
            Err(e) => e,
        };
        assert!(matches!(err, TsSessionError::Config(_)));
    }

    #[test]
    fn violations_are_sorted_by_file_then_line() {
        let tmp = TempDir::new().expect("tempdir");
        write(
            tmp.path(),
            "src/b/Second.kt",
            "package com.example.domain\n\nimport com.example.infra.A\n",
        );
        write(
            tmp.path(),
            "src/a/First.kt",
            "package com.example.domain\n\nimport com.example.infra.B\n",
        );

        let config = ArchConfig::parse(CONFIG).expect("parse");
        let result = TsSession::new(config, tmp.path()).run().expect("run");

        let files: Vec<_> = result
            .violations
            .iter()
            .map(|v| v.location.file.clone())
            .collect();
        assert_eq!(
            files,
            vec![
                PathBuf::from("src/a/First.kt"),
                PathBuf::from("src/b/Second.kt")
            ]
        );
    }

    #[test]
    fn exclude_glob_is_matched_against_relative_path() {
        let tmp = TempDir::new().expect("tempdir");
        write(
            tmp.path(),
            "src/domain/Order.kt",
            "package com.example.domain\n\nimport com.example.infra.OrderTable\n",
        );
        write(
            tmp.path(),
            "src/test/OrderTest.kt",
            "package com.example.domain\n\nimport com.example.infra.OrderTable\n",
        );

        let mut config = ArchConfig::parse(CONFIG).expect("parse");
        config.exclude = vec!["src/test/**".to_string()];

        let result = TsSession::new(config, tmp.path()).run().expect("run");
        assert_eq!(result.files_checked, 1);
    }

    #[test]
    fn exclude_substring_fallback_still_applies() {
        let tmp = TempDir::new().expect("tempdir");
        write(
            tmp.path(),
            "src/domain/Order.kt",
            "package com.example.domain\n",
        );
        write(
            tmp.path(),
            "src/generated/Gen.kt",
            "package com.example.domain\n",
        );

        let mut config = ArchConfig::parse(CONFIG).expect("parse");
        // Neither a valid relative glob for this path nor a literal prefix:
        // only the substring fallback can match it.
        config.exclude = vec!["**/generated/**".to_string()];

        let result = TsSession::new(config, tmp.path()).run().expect("run");
        assert_eq!(result.files_checked, 1);
    }

    #[test]
    fn gitignored_files_are_skipped_unless_disabled() {
        let tmp = TempDir::new().expect("tempdir");
        write(tmp.path(), ".gitignore", "build/\n");
        write(
            tmp.path(),
            "build/Generated.kt",
            "package com.example.domain\n\nimport com.example.infra.A\n",
        );
        write(
            tmp.path(),
            "src/domain/Order.kt",
            "package com.example.domain\n",
        );

        let config = ArchConfig::parse(CONFIG).expect("parse");
        let respected = TsSession::new(config.clone(), tmp.path())
            .run()
            .expect("run");
        assert_eq!(respected.files_checked, 1);

        let ignored = TsSession::new(config, tmp.path())
            .respect_gitignore(false)
            .run()
            .expect("run");
        assert_eq!(ignored.files_checked, 2);
    }

    #[test]
    fn analysis_root_applies_config_root() {
        let config = ArchConfig::parse("[analyzer]\nroot = \"./src\"\n").expect("parse");
        let session = TsSession::new(config, "/project");
        assert_eq!(session.analysis_root(), PathBuf::from("/project/./src"));

        let absolute = ArchConfig::parse("[analyzer]\nroot = \"/elsewhere\"\n").expect("parse");
        let session = TsSession::new(absolute, "/project");
        assert_eq!(session.analysis_root(), PathBuf::from("/elsewhere"));
    }

    #[test]
    fn unsupported_extensions_are_ignored() {
        let tmp = TempDir::new().expect("tempdir");
        write(tmp.path(), "src/README.md", "not kotlin\n");

        let config = ArchConfig::parse(CONFIG).expect("parse");
        let result = TsSession::new(config, tmp.path()).run().expect("run");
        assert_eq!(result.files_checked, 0);
    }
}
