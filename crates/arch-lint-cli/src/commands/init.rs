//! Init command implementation.

use anyhow::{bail, Result};
use std::path::Path;

const DEFAULT_CONFIG: &str = r#"# arch-lint configuration
# See https://github.com/example/arch-lint for documentation

[analyzer]
# Root directory to analyze (default: current directory)
# root = "./src"

# Glob patterns to exclude from analysis
exclude = [
    "**/target/**",
    "**/vendor/**",
    "**/generated/**",
]

# Respect .gitignore files
respect_gitignore = true

# Rule configurations
# Each rule can be enabled/disabled and have its severity overridden

[rules.no-unwrap-expect]
enabled = true
# severity = "warning"  # Override default severity
allow_in_tests = true

[rules.no-sync-io]
enabled = true

# [rules.handler-complexity]
# enabled = true
# max_lines = 150
# max_match_arms = 20
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
