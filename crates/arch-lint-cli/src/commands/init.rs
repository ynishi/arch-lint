//! Init command implementation.

use anyhow::{bail, Result};
use std::path::Path;

const DEFAULT_CONFIG: &str = r#"# arch-lint configuration
# See https://github.com/ynishi/arch-lint for documentation

# Preset: "recommended" (default), "strict", or "minimal"
# preset = "recommended"

# Exit non-zero on violations at or above this severity: "error" (default), "warning", "info"
# fail_on = "error"

[analyzer]
# Root directory to analyze (default: current directory)
# root = "./src"

# Glob patterns to exclude from analysis
# Matched against each file's path relative to the root and its absolute path.
exclude = [
    "**/target/**",
    "**/vendor/**",
    "**/generated/**",
]

# Skip files ignored by git (.gitignore, .git/info/exclude, global gitignore)
# respect_gitignore = true

# If set, only files matching one of these patterns are analyzed
# include = ["src/**"]

# Rule configurations
# Each rule can be enabled/disabled, have its severity overridden,
# and skip specific files via `exclude_files` (globs relative to the root).

[rules.no-unwrap-expect]
enabled = true
# severity = "warning"  # Override default severity
allow_in_tests = true

[rules.no-sync-io]
enabled = true
# exclude_files = ["src/startup.rs", "src/cli/**"]

# [rules.handler-complexity]
# enabled = true
# max_handler_lines = 150
# max_match_arms = 20
# max_enum_variants = 30
"#;

/// Runs the init command.
pub fn run(force: bool) -> Result<()> {
    let config_path = Path::new("arch-lint.toml");

    if config_path.exists() && !force {
        bail!(
            "Configuration file already exists at {}. Use --force to overwrite.",
            config_path.display()
        );
    }

    std::fs::write(config_path, DEFAULT_CONFIG)?;

    println!("Created arch-lint.toml");
    println!("\nNext steps:");
    println!("  1. Edit arch-lint.toml to configure rules");
    println!("  2. Run: arch-lint check");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_CONFIG;
    use arch_lint_core::Config;
    use arch_lint_rules::Preset;

    #[test]
    fn default_config_template_is_accepted_by_every_preset() {
        let config = Config::parse(DEFAULT_CONFIG).expect("template is valid TOML");
        for preset in [Preset::Recommended, Preset::Strict, Preset::Minimal] {
            assert!(
                preset.rules_with_config(&config).is_ok(),
                "template holds an option {preset:?} rejects"
            );
        }
    }
}
