//! TOML configuration for architecture layer rules.
//!
//! Extends the base arch-lint config with `[[layers]]`, `[dependencies]`,
//! and `[[constraints]]` sections.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use arch_lint_core::Severity;

/// Top-level architecture lint configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ArchConfig {
    /// Project root directory.
    #[serde(default = "default_root")]
    pub root: PathBuf,

    /// Glob patterns to exclude.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// Layer definitions.
    #[serde(rename = "layers", default)]
    pub layers: Vec<LayerDef>,

    /// Dependency rules: layer_name -> list of allowed dependencies.
    #[serde(default)]
    pub dependencies: HashMap<String, Vec<String>>,

    /// Custom constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
}

/// A named architecture layer.
#[derive(Debug, Clone, Deserialize)]
pub struct LayerDef {
    /// Layer name (e.g., `"domain"`, `"infrastructure"`).
    pub name: String,
    /// Package prefixes belonging to this layer.
    pub packages: Vec<String>,
}

/// A custom constraint rule.
#[derive(Debug, Clone, Deserialize)]
pub struct Constraint {
    /// Constraint type: `"no-import-pattern"` or `"naming-rule"`.
    #[serde(rename = "type")]
    pub kind: String,

    /// Pattern to match against import paths (used by `no-import-pattern`).
    #[serde(default)]
    pub pattern: String,

    /// Layers this constraint applies to.
    #[serde(default)]
    pub in_layers: Vec<String>,

    /// Severity for violations of this constraint.
    #[serde(default = "default_severity")]
    pub severity: Severity,

    /// Human-readable message.
    #[serde(default)]
    pub message: String,

    /// Import path must contain this substring to trigger the rule (used by `naming-rule`).
    #[serde(default)]
    pub import_matches: String,

    /// Source file must have a declaration matching this substring (used by `naming-rule`).
    #[serde(default)]
    pub source_must_match: String,

    /// Source file must NOT have a declaration matching this substring (used by `naming-rule`).
    #[serde(default)]
    pub source_must_not_match: String,
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

fn default_severity() -> Severity {
    Severity::Error
}

/// Errors when loading configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Failed to read config file.
    #[error("failed to read {path}: {source}")]
    Io {
        /// Path that failed.
        path: PathBuf,
        /// IO error.
        source: std::io::Error,
    },
    /// Failed to parse TOML.
    #[error("invalid config: {message}")]
    Parse {
        /// Parse error detail.
        message: String,
    },
    /// Config is structurally invalid.
    #[error("config validation: {0}")]
    Validation(String),
}

impl ArchConfig {
    /// Load from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns error if file cannot be read or parsed.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Self::parse(&content)
    }

    /// Parse from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns error if TOML is invalid.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        /// Wrapper to handle `[analyzer]` section in the TOML.
        #[derive(Deserialize)]
        struct RawConfig {
            #[serde(default)]
            analyzer: AnalyzerSection,
            #[serde(rename = "layers", default)]
            layers: Vec<LayerDef>,
            #[serde(default)]
            dependencies: HashMap<String, Vec<String>>,
            #[serde(default)]
            constraints: Vec<Constraint>,
        }

        #[derive(Deserialize, Default)]
        struct AnalyzerSection {
            #[serde(default = "default_root")]
            root: PathBuf,
            #[serde(default)]
            exclude: Vec<String>,
        }

        let raw: RawConfig = toml::from_str(content).map_err(|e| ConfigError::Parse {
            message: e.to_string(),
        })?;

        Ok(Self {
            root: raw.analyzer.root,
            exclude: raw.analyzer.exclude,
            layers: raw.layers,
            dependencies: raw.dependencies,
            constraints: raw.constraints,
        })
    }

    /// Validate config consistency.
    ///
    /// # Errors
    ///
    /// Returns error describing the first problem found.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let layer_names: std::collections::HashSet<&str> =
            self.layers.iter().map(|l| l.name.as_str()).collect();

        for (layer, deps) in &self.dependencies {
            if !layer_names.contains(layer.as_str()) {
                return Err(ConfigError::Validation(format!(
                    "dependencies.{layer}: unknown layer"
                )));
            }
            for dep in deps {
                if !layer_names.contains(dep.as_str()) {
                    return Err(ConfigError::Validation(format!(
                        "dependencies.{layer}: unknown dep '{dep}'"
                    )));
                }
            }
            if deps.contains(layer) {
                return Err(ConfigError::Validation(format!(
                    "dependencies.{layer}: self-dependency"
                )));
            }
        }

        for (i, c) in self.constraints.iter().enumerate() {
            for l in &c.in_layers {
                if !layer_names.contains(l.as_str()) {
                    return Err(ConfigError::Validation(format!(
                        "constraints[{i}]: unknown layer '{l}'"
                    )));
                }
            }
        }

        for l in &self.layers {
            if !self.dependencies.contains_key(&l.name) {
                return Err(ConfigError::Validation(format!(
                    "layer '{}' has no entry in [dependencies]",
                    l.name
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[dependencies]
domain = []
"#;
        let config = ArchConfig::parse(toml).expect("parse failed");
        assert_eq!(config.layers.len(), 1);
        assert_eq!(config.layers[0].name, "domain");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[analyzer]
root = "./src"
exclude = ["**/test/**"]

[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "app"
packages = ["com.example.app"]

[dependencies]
domain = []
app = ["domain"]

[[constraints]]
type = "no-import-pattern"
pattern = "java.sql"
in_layers = ["domain"]
severity = "warning"
message = "No JDBC in domain"
"#;
        let config = ArchConfig::parse(toml).expect("parse failed");
        assert_eq!(config.layers.len(), 2);
        assert_eq!(config.constraints.len(), 1);
        assert_eq!(config.constraints[0].severity, Severity::Warning);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_catches_unknown_layer_in_deps() {
        let toml = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[dependencies]
domain = ["nonexistent"]
"#;
        let config = ArchConfig::parse(toml).expect("parse failed");
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_catches_missing_dep_entry() {
        let toml = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "app"
packages = ["com.example.app"]

[dependencies]
domain = []
"#;
        let config = ArchConfig::parse(toml).expect("parse failed");
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("app"));
    }

    #[test]
    fn validate_catches_self_dependency() {
        let toml = r#"
[[layers]]
name = "domain"
packages = ["com.example.domain"]

[dependencies]
domain = ["domain"]
"#;
        let config = ArchConfig::parse(toml).expect("parse failed");
        assert!(config.validate().is_err());
    }
}
