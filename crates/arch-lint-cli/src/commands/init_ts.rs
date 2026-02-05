//! Init command for tree-sitter config.

use anyhow::{bail, Result};
use std::path::Path;

const TS_CONFIG_TEMPLATE: &str = r#"# arch-lint configuration (tree-sitter engine)
# This config enables cross-language architecture enforcement.
# The presence of [[layers]] activates the tree-sitter engine automatically.

[analyzer]
root = "."
exclude = ["**/test/**", "**/build/**", "**/generated/**"]

# Layer definitions
# Each layer has a name and a list of package prefixes.
# Files whose package matches a prefix belong to that layer.

[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "application"
packages = ["com.example.app", "com.example.usecase"]

[[layers]]
name = "infrastructure"
packages = ["com.example.infra"]

[[layers]]
name = "presentation"
packages = ["com.example.api", "com.example.handler"]

# Dependency rules: which layers may depend on which.
# A layer may always depend on itself (same-layer imports are allowed).

[dependencies]
domain = []
application = ["domain"]
infrastructure = ["domain", "application"]
presentation = ["domain", "application", "infrastructure"]

# Custom constraints (optional)
# Pattern-based import restrictions.

# [[constraints]]
# type = "no-import-pattern"
# pattern = "java.sql"
# in_layers = ["domain", "application"]
# severity = "warning"
# message = "Avoid direct JDBC usage in upper layers"
"#;

/// Runs the init --ts command.
pub fn run(force: bool) -> Result<()> {
    let config_path = Path::new("arch-lint.toml");

    if config_path.exists() && !force {
        bail!(
            "Configuration file already exists at {}. Use --force to overwrite.",
            config_path.display()
        );
    }

    std::fs::write(config_path, TS_CONFIG_TEMPLATE)?;

    println!("Created arch-lint.toml (tree-sitter engine)");
    println!();
    println!("Next steps:");
    println!("  1. Edit [[layers]] and [dependencies] for your project");
    println!("  2. Run: arch-lint check");
    println!();
    println!("The tree-sitter engine activates automatically when [[layers]] is present.");

    Ok(())
}
