//! Configuration types for arch-lint.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level configuration for arch-lint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Preset to use (e.g., "recommended", "strict", "minimal").
    #[serde(default)]
    pub preset: Option<String>,

    /// Severity threshold for test failure (default: "error").
    /// Violations at or above this severity cause `check!()` to fail.
    #[serde(default)]
    pub fail_on: Option<String>,

    /// Analyzer configuration.
    #[serde(default)]
    pub analyzer: AnalyzerConfig,

    /// Per-rule configurations.
    #[serde(default)]
    pub rules: HashMap<String, RuleConfig>,
}

impl Config {
    /// Creates a new default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: &std::path::Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Self::parse(&content)
    }

    /// Parses configuration from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        toml::from_str(content).map_err(|e| ConfigError::Parse {
            message: e.to_string(),
        })
    }

    /// Checks if a rule is enabled.
    #[must_use]
    pub fn is_rule_enabled(&self, rule_name: &str) -> bool {
        self.rules
            .get(rule_name)
            .map_or(true, |c| c.enabled.unwrap_or(true))
    }

    /// Gets the severity override for a rule.
    #[must_use]
    pub fn rule_severity(&self, rule_name: &str) -> Option<crate::Severity> {
        self.rules.get(rule_name).and_then(|c| c.severity)
    }
}

/// Analyzer-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    /// Root directory to analyze (default: current directory).
    #[serde(default = "default_root")]
    pub root: PathBuf,

    /// Glob patterns to exclude from analysis.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Glob patterns to include (if empty, all *.rs files).
    #[serde(default)]
    pub include: Vec<String>,

    /// Whether to respect .gitignore files.
    #[serde(default = "default_true")]
    pub respect_gitignore: bool,

    /// Maximum number of parallel file analyses.
    #[serde(default)]
    pub parallelism: Option<usize>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            root: default_root(),
            exclude: vec!["**/target/**".to_string(), "**/vendor/**".to_string()],
            include: Vec::new(),
            respect_gitignore: true,
            parallelism: None,
        }
    }
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_true() -> bool {
    true
}

/// Per-rule configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleConfig {
    /// Whether this rule is enabled.
    #[serde(default)]
    pub enabled: Option<bool>,

    /// Severity override for this rule.
    #[serde(default)]
    pub severity: Option<crate::Severity>,

    /// Rule-specific options as key-value pairs.
    #[serde(flatten)]
    pub options: HashMap<String, toml::Value>,
}

impl RuleConfig {
    /// Gets an option value as a specific type.
    #[must_use]
    pub fn get_option<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.options
            .get(key)
            .and_then(|v| v.clone().try_into().ok())
    }

    /// Gets a boolean option with a default value.
    #[must_use]
    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.options
            .get(key)
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    }

    /// Gets an integer option with a default value.
    #[must_use]
    pub fn get_int(&self, key: &str, default: i64) -> i64 {
        self.options
            .get(key)
            .and_then(toml::Value::as_integer)
            .unwrap_or(default)
    }

    /// Gets a string option with a default value.
    #[must_use]
    pub fn get_str<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.options
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
    }

    /// Gets a string array option.
    #[must_use]
    pub fn get_str_array(&self, key: &str) -> Vec<String> {
        self.options
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Configuration errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// IO error reading config file.
    #[error("Failed to read config file {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Parse error in config file.
    #[error("Failed to parse config: {message}")]
    Parse {
        /// Parse error message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.analyzer.respect_gitignore);
        assert!(config.rules.is_empty());
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[analyzer]
root = "./src"
exclude = ["**/generated/**"]

[rules.no-unwrap-expect]
enabled = true
severity = "warning"
allow_in_tests = true
"#;

        let config = Config::parse(toml).expect("Failed to parse");
        assert_eq!(config.analyzer.root, PathBuf::from("./src"));
        assert!(config.is_rule_enabled("no-unwrap-expect"));

        let rule_config = config.rules.get("no-unwrap-expect").unwrap();
        assert!(rule_config.get_bool("allow_in_tests", false));
    }
}
