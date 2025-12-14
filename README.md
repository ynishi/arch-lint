# arch-lint

A `syn`-based extensible architecture linter for Rust projects.

[![Crates.io](https://img.shields.io/crates/v/arch-lint.svg)](https://crates.io/crates/arch-lint)
[![License](https://img.shields.io/crates/l/arch-lint.svg)](LICENSE)

## Why arch-lint?

In the age of AI-assisted coding, both humans and AI tend to miss consistent architectural violations:

- `.unwrap()` calls sneaking into production code
- Blocking I/O in async contexts causing performance issues
- Errors being swallowed with just logging
- Handler functions growing too complex
- Inconsistent error type definitions

**arch-lint catches what code review misses.** It provides machine-enforceable rules for patterns that are easy to overlook but critical for code quality.

### Key Features

- **AST-based analysis** - Deep understanding of your code structure
- **Mandatory reasoning** - Critical violations require documented reasons when suppressed
- **Extensible rules** - Easy to add custom architectural constraints
- **CI-friendly** - JSON output, exit codes, and clear violation reporting

## Installation

```bash
cargo install arch-lint-cli
```

Or add to your project:

```bash
cargo add arch-lint-core arch-lint-rules --dev
```

## Quick Start

```bash
# Initialize configuration
arch-lint init

# Run lint checks
arch-lint check

# Check specific directory
arch-lint check ./src

# Use specific rules only
arch-lint check --rules no-unwrap-expect,no-sync-io

# Output as JSON (for CI integration)
arch-lint check --format json
```

## Available Rules

| Code | Name | Description | Default |
|------|------|-------------|---------|
| AL001 | `no-unwrap-expect` | Forbids `.unwrap()` and `.expect()` in production code | Error |
| AL002 | `no-sync-io` | Forbids blocking I/O operations | Error |
| AL003 | `no-error-swallowing` | Forbids catching errors with only logging | Error |
| AL004 | `handler-complexity` | Limits handler function complexity | Warning |
| AL005 | `require-thiserror` | Requires `thiserror` derive for error types | Error |

### Rule Details

#### AL001: no-unwrap-expect

Detects `.unwrap()` and `.expect()` calls that can cause panics.

```rust
// BAD
let value = some_option.unwrap();
let parsed = "123".parse::<i32>().expect("should parse");

// GOOD
let value = some_option.ok_or(MyError::NotFound)?;
let parsed = "123".parse::<i32>().map_err(MyError::Parse)?;
```

**Configuration:**
```toml
[rules.no-unwrap-expect]
allow_in_tests = true    # Allow in test code (default: true)
allow_expect = false     # Allow .expect() but forbid .unwrap()
severity = "error"
```

#### AL002: no-sync-io

Detects blocking I/O operations that can stall async runtimes.

```rust
// BAD
let content = std::fs::read_to_string("file.txt")?;
if path.exists() { /* ... */ }

// GOOD
let content = tokio::fs::read_to_string("file.txt").await?;
if tokio::fs::try_exists(&path).await? { /* ... */ }
```

**Configuration:**
```toml
[rules.no-sync-io]
allow_patterns = ["tokio::", "async_std::"]
severity = "error"
```

#### AL003: no-error-swallowing

Detects error handling that only logs without propagation.

```rust
// BAD
if let Err(e) = do_something() {
    tracing::error!("Failed: {}", e);
    // Error is swallowed!
}

// GOOD
do_something().map_err(|e| {
    tracing::error!("Failed: {}", e);
    e
})?;
```

**Configuration:**
```toml
[rules.no-error-swallowing]
severity = "error"
```

#### AL004: handler-complexity

Limits complexity in handler functions (TEA/Elm architecture).

```rust
// BAD - Too many match arms
fn handle_action(action: Action) {
    match action {
        Action::A => { /* ... */ }
        Action::B => { /* ... */ }
        // ... 30+ arms
    }
}

// GOOD - Split into sub-handlers
fn handle_action(action: Action) {
    match action {
        Action::User(user_action) => handle_user_action(user_action),
        Action::System(sys_action) => handle_system_action(sys_action),
    }
}
```

**Configuration:**
```toml
[rules.handler-complexity]
max_handler_lines = 150
max_match_arms = 20
max_enum_variants = 30
severity = "warning"
```

#### AL005: require-thiserror

Requires `thiserror::Error` derive for error types.

```rust
// BAD
#[derive(Debug)]
pub enum MyError {
    Io(std::io::Error),
    Parse(String),
}

// GOOD
#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),
}
```

**Configuration:**
```toml
[rules.require-thiserror]
severity = "error"
```

## Configuration

Create `arch-lint.toml` in your project root:

```toml
[analyzer]
root = "./src"
exclude = [
    "**/target/**",
    "**/generated/**",
    "**/vendor/**",
]
respect_gitignore = true

[rules.no-unwrap-expect]
enabled = true
severity = "error"
allow_in_tests = true

[rules.no-sync-io]
enabled = true

[rules.handler-complexity]
enabled = true
max_handler_lines = 150
max_match_arms = 20

[rules.require-thiserror]
enabled = true
severity = "warning"
```

## Suppression

arch-lint provides multiple ways to suppress violations at different scopes.

### Scope Overview

| Scope | Method | Use Case |
|-------|--------|----------|
| Line | Comment | Single expression |
| Block | `#[arch_lint::allow(...)]` | Function, impl, module |
| File | `#![arch_lint::allow(...)]` | Entire file |
| Global | Configuration | Project-wide exclusion |

### Line-level (Comment)

Use inline comments for single expressions:

```rust
// arch-lint: allow(no-sync-io) reason="Startup initialization only"
let config = std::fs::read_to_string("config.toml")?;

// arch-lint: allow(no-unwrap-expect) reason="Guaranteed by loop invariant"
let value = some_option.unwrap();
```

### Block-level (Attribute)

Use attributes for functions, impl blocks, or modules:

```rust
#[arch_lint::allow(no_unwrap_expect, reason = "All inputs validated at entry point")]
fn parse_validated_config(input: &str) -> Config {
    let value = input.parse().unwrap();
    let count = input.len().try_into().unwrap();
    Config { value, count }
}

#[arch_lint::allow(no_sync_io, reason = "CLI startup, not in async context")]
mod startup {
    // All sync I/O allowed in this module
}
```

### File-level (Inner Attribute)

Use inner attributes at the top of the file:

```rust
//! CLI entry point - synchronous I/O is acceptable here.

#![arch_lint::allow(no_sync_io, reason = "CLI tool, no async runtime")]

fn main() {
    let config = std::fs::read_to_string("config.toml").unwrap();
    // ...
}
```

### Mandatory Reasoning

For `Severity::Error` rules (AL001, AL002, AL003, AL005), the `reason` parameter is **required**. Omitting it will generate a `Severity::Warning` violation:

```rust
// ❌ Warning: Allow directive missing required reason
#[arch_lint::allow(no_unwrap_expect)]
fn bad() { ... }

// ✅ OK: Reason provided
#[arch_lint::allow(no_unwrap_expect, reason = "Config validated at startup")]
fn good() { ... }
```

This ensures that critical suppressions are always documented and justified.

### Clippy Compatibility

Standard Clippy attributes are also recognized:

```rust
#[allow(clippy::unwrap_used)]
fn allowed_unwrap() {
    value.unwrap()  // OK - recognized by arch-lint
}
```

### Configuration File

Disable rules globally or per-file in `arch-lint.toml`:

```toml
[rules.no-unwrap-expect]
enabled = false  # Disable entirely

[rules.no-sync-io]
exclude_files = ["src/startup.rs", "src/cli/**"]
```

## Presets

Use presets for quick configuration:

| Preset | Rules | Description |
|--------|-------|-------------|
| `recommended` | AL001, AL002, AL003, AL005 | Sensible defaults |
| `strict` | All rules | Maximum safety |
| `minimal` | AL001 (relaxed) | Gradual adoption |

## Programmatic Usage

```rust
use arch_lint_core::Analyzer;
use arch_lint_rules::{NoUnwrapExpect, NoSyncIo, HandlerComplexity};

let analyzer = Analyzer::builder()
    .root("./src")
    .rule(NoUnwrapExpect::new().allow_in_tests(true))
    .rule(NoSyncIo::new())
    .rule(HandlerComplexity::new().max_match_arms(15))
    .exclude("**/generated/**")
    .build()?;

let result = analyzer.analyze()?;

if result.has_errors() {
    result.print_report();
    std::process::exit(1);
}
```

## Writing Custom Rules

```rust
use arch_lint_core::{Rule, FileContext, Violation, Severity, Location};
use syn::visit::Visit;

pub struct NoTodoComments;

impl Rule for NoTodoComments {
    fn name(&self) -> &'static str { "no-todo-comments" }
    fn code(&self) -> &'static str { "PROJ001" }
    fn description(&self) -> &'static str {
        "Forbids TODO comments in production code"
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        // Implement using syn visitor pattern
        let mut visitor = TodoVisitor::new(ctx);
        visitor.visit_file(ast);
        visitor.violations
    }
}

// Register with analyzer
let analyzer = Analyzer::builder()
    .rule(NoTodoComments)
    .build()?;
```

## CI Integration

### GitHub Actions

```yaml
name: Lint
on: [push, pull_request]

jobs:
  arch-lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install arch-lint-cli
      - run: arch-lint check --format json > lint-results.json
      - name: Check results
        run: |
          if [ $(jq '.violations | length' lint-results.json) -gt 0 ]; then
            arch-lint check
            exit 1
          fi
```

### Pre-commit Hook

```bash
#!/bin/sh
# .git/hooks/pre-commit

arch-lint check --format compact
if [ $? -ne 0 ]; then
    echo "arch-lint found violations. Please fix before committing."
    exit 1
fi
```

## Crate Structure

| Crate | Description |
|-------|-------------|
| `arch-lint` | Facade crate (re-exports core + macros) |
| `arch-lint-core` | Core framework (traits, analyzer, types) |
| `arch-lint-rules` | Built-in lint rules |
| `arch-lint-cli` | Command-line interface |
| `arch-lint-macros` | Procedural macros (`#[arch_lint::allow(...)]`) |

## Comparison with Other Tools

| Tool | Focus | AST-based | Custom Rules | Rust-specific |
|------|-------|-----------|--------------|---------------|
| **arch-lint** | Architecture patterns | Yes | Easy | Yes |
| Clippy | Code quality | Yes | Hard | Yes |
| cargo-deny | Dependencies | No | Config | Yes |
| rust-analyzer | IDE support | Yes | No | Yes |

arch-lint complements Clippy by focusing on **architectural patterns** rather than code style. Use both for comprehensive linting.

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
