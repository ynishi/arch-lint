//! Internal runner for `check!()` macro integration.
//!
//! This module is `#[doc(hidden)]` and not part of the public API.
//! It is called by the generated test function from `arch_lint::check!()`.

use arch_lint_core::{Analyzer, Config, Severity};
use arch_lint_rules::Preset;
use std::path::{Path, PathBuf};

/// Config file names to search for, in priority order.
const CONFIG_CANDIDATES: &[&str] = &["arch-lint.toml", ".arch-lint.toml"];

/// Runs arch-lint analysis as part of `cargo test`.
///
/// Called by the `check!()` macro-generated test function.
/// Panics with a formatted report if violations are found.
///
/// # Panics
///
/// Panics if violations at or above `fail_on` severity are found,
/// or if the analyzer cannot be built.
pub fn run_check(preset: Option<&str>, config_path: Option<&str>, fail_on: Option<&str>) {
    let root = find_project_root();
    let content = read_config_content(&root, config_path);
    let config = parse_config(&content);

    let effective_preset = resolve_preset(preset, &config);
    let effective_fail_on = resolve_fail_on(fail_on, &config);
    let preset_rules = effective_preset.rules();
    let declarative_rules = load_declarative_rules(&content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in preset_rules {
        builder = builder.rule_box(rule);
    }
    for rule in declarative_rules {
        builder = builder.rule_box(rule);
    }

    let analyzer = builder.build().unwrap_or_else(|e| {
        panic!("arch-lint: failed to build analyzer: {e}");
    });

    let result = analyzer.analyze().unwrap_or_else(|e| {
        panic!("arch-lint: analysis failed: {e}");
    });

    if result.has_violations_at(effective_fail_on) {
        let report = result.format_test_report(effective_fail_on);
        panic!("{report}");
    }
}

/// Reads the raw TOML content from the config file.
///
/// Returns an empty string if no config file is found.
fn read_config_content(root: &Path, explicit_path: Option<&str>) -> String {
    if let Some(path) = explicit_path {
        let full_path = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            root.join(path)
        };
        return std::fs::read_to_string(&full_path).unwrap_or_else(|e| {
            panic!(
                "arch-lint: failed to read config from {}: {e}",
                full_path.display()
            );
        });
    }

    for candidate in CONFIG_CANDIDATES {
        let path = root.join(candidate);
        if path.exists() {
            return std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "arch-lint: failed to read config from {}: {e}",
                    path.display()
                );
            });
        }
    }

    String::new()
}

/// Parses a `Config` from TOML content.
fn parse_config(content: &str) -> Config {
    if content.is_empty() {
        return Config::default();
    }
    Config::parse(content).unwrap_or_else(|e| {
        panic!("arch-lint: failed to parse config: {e}");
    })
}

/// Loads declarative rules from TOML content.
///
/// Returns an empty vec if no declarative sections are present.
fn load_declarative_rules(content: &str) -> Vec<arch_lint_core::RuleBox> {
    if content.is_empty() {
        return vec![];
    }
    arch_lint_core::declarative::load_rules_from_toml(content)
        .unwrap_or_else(|e| panic!("arch-lint: declarative config error: {e}"))
}

/// Checks whether a `Cargo.toml` file defines a `[workspace]` section
/// by parsing as TOML, avoiding false positives from comments or strings.
fn has_workspace_section(cargo_toml: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return false;
    };
    table.contains_key("workspace")
}

/// Finds the project root by looking for `Cargo.toml` from `CARGO_MANIFEST_DIR`.
fn find_project_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points to the crate containing the test,
    // which may be a workspace member. Walk up to find workspace root.
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = PathBuf::from(&manifest_dir);

        // Check if there's a workspace Cargo.toml above
        let mut candidate = manifest_path.as_path();
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

        // No workspace root found — use manifest dir itself
        return manifest_path;
    }

    // Fallback: current directory
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Resolves the effective preset from macro arg > config > default.
fn resolve_preset(macro_arg: Option<&str>, config: &Config) -> Preset {
    let name = macro_arg
        .or(config.preset.as_deref())
        .unwrap_or("recommended");

    match name {
        "recommended" => Preset::Recommended,
        "strict" => Preset::Strict,
        "minimal" => Preset::Minimal,
        other => panic!(
            "arch-lint: unknown preset `{other}`. Valid presets: recommended, strict, minimal"
        ),
    }
}

/// Resolves the effective `fail_on` severity from macro arg > config > default.
///
/// Priority: explicit macro arg > config file > default ("error").
fn resolve_fail_on(macro_arg: Option<&str>, config: &Config) -> Severity {
    let name = macro_arg.or(config.fail_on.as_deref()).unwrap_or("error");

    match name {
        "error" => Severity::Error,
        "warning" => Severity::Warning,
        "info" => Severity::Info,
        other => {
            panic!("arch-lint: unknown severity `{other}`. Valid values: error, warning, info")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preset_defaults_to_recommended() {
        let config = Config::default();
        assert_eq!(resolve_preset(None, &config), Preset::Recommended);
    }

    #[test]
    fn resolve_preset_macro_arg_takes_precedence() {
        let mut config = Config::default();
        config.preset = Some("minimal".to_string());
        // macro arg "strict" overrides config "minimal"
        assert_eq!(resolve_preset(Some("strict"), &config), Preset::Strict);
    }

    #[test]
    fn resolve_preset_from_config() {
        let mut config = Config::default();
        config.preset = Some("strict".to_string());
        assert_eq!(resolve_preset(None, &config), Preset::Strict);
    }

    #[test]
    #[should_panic(expected = "unknown preset")]
    fn resolve_preset_invalid_panics() {
        let config = Config::default();
        resolve_preset(Some("nonexistent"), &config);
    }

    #[test]
    fn resolve_fail_on_defaults_to_error() {
        let config = Config::default();
        assert_eq!(resolve_fail_on(None, &config), Severity::Error);
    }

    #[test]
    fn resolve_fail_on_from_config() {
        let mut config = Config::default();
        config.fail_on = Some("warning".to_string());
        assert_eq!(resolve_fail_on(None, &config), Severity::Warning);
    }

    #[test]
    fn resolve_fail_on_macro_arg_overrides_config() {
        let mut config = Config::default();
        config.fail_on = Some("info".to_string());
        // Explicit "warning" from macro overrides config "info"
        assert_eq!(resolve_fail_on(Some("warning"), &config), Severity::Warning);
    }

    #[test]
    fn resolve_fail_on_explicit_error_overrides_config() {
        let mut config = Config::default();
        config.fail_on = Some("warning".to_string());
        // Explicit "error" from macro must override config "warning"
        assert_eq!(resolve_fail_on(Some("error"), &config), Severity::Error);
    }

    #[test]
    #[should_panic(expected = "unknown severity")]
    fn resolve_fail_on_invalid_panics() {
        let config = Config::default();
        resolve_fail_on(Some("critical"), &config);
    }

    // ── Declarative rules loading ──

    #[test]
    fn load_declarative_rules_empty_content() {
        let rules = load_declarative_rules("");
        assert!(rules.is_empty());
    }

    #[test]
    fn load_declarative_rules_no_declarative_sections() {
        let toml = r#"
preset = "recommended"
fail_on = "error"
"#;
        let rules = load_declarative_rules(toml);
        assert!(rules.is_empty());
    }

    #[test]
    fn load_declarative_rules_creates_restrict_use_rule() {
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
        let rules = load_declarative_rules(toml);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name(), "restrict-use");
    }

    #[test]
    fn load_declarative_rules_creates_all_rule_types() {
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
        let rules = load_declarative_rules(toml);
        assert_eq!(rules.len(), 3);

        let names: Vec<&str> = rules.iter().map(|r| r.name()).collect();
        assert!(names.contains(&"restrict-use"));
        assert!(names.contains(&"require-use"));
        assert!(names.contains(&"deny-scope-dep"));
    }

    #[test]
    fn parse_config_with_declarative_sections() {
        // Config parser should ignore declarative sections (serde skips unknown fields)
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
        let config = parse_config(toml);
        assert_eq!(config.preset.as_deref(), Some("minimal"));
    }
}
