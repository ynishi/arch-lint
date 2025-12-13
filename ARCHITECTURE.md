# arch-lint Architecture

A `syn`-based extensible architecture linter for Rust projects.

## Overview

arch-lint is designed to catch architectural violations and enforce coding patterns that are often missed by both humans and AI during code review. It provides a framework for defining custom lint rules based on AST analysis.

## Crate Structure

```
arch-lint/
├── crates/
│   ├── arch-lint-core/       # Core framework
│   ├── arch-lint-rules/      # Built-in rules
│   ├── arch-lint-cli/        # CLI binary
│   └── arch-lint-macros/     # Procedural macros (future)
```

### arch-lint-core

The foundation of the linting framework. Provides:

- **`Rule` trait** - Per-file AST-based lint rules
- **`ProjectRule` trait** - Project-wide structural rules
- **`Analyzer`** - Orchestrates file discovery and rule execution
- **`Violation`** - Represents lint findings with location and suggestions
- **`Config`** - TOML-based configuration system
- **`utils`** - Helpers for attribute parsing, allowance directives, path matching

### arch-lint-rules

Collection of general-purpose rules:

| Code | Name | Description |
|------|------|-------------|
| AL001 | `no-unwrap-expect` | Forbids `.unwrap()` and `.expect()` |
| AL002 | `no-sync-io` | Forbids blocking I/O in async contexts |
| AL003 | `no-error-swallowing` | Forbids catching errors with only logging |
| AL004 | `handler-complexity` | Limits handler function complexity |
| AL005 | `require-thiserror` | Requires `thiserror` for error types |

Presets:
- `Recommended` - AL001, AL002, AL003, AL005 (default)
- `Strict` - All rules with stricter settings
- `Minimal` - AL001 only for gradual adoption

### arch-lint-cli

Command-line interface:

```bash
arch-lint check [PATH]           # Run lint checks
arch-lint check --format json    # JSON output for CI
arch-lint list-rules             # Show available rules
arch-lint init                   # Create config file
```

### arch-lint-macros

Procedural macros for rule definition (planned):
- `#[derive(LintRule)]` - Auto-implement boilerplate

## Core Abstractions

### Rule Trait

```rust
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn code(&self) -> &'static str;
    fn description(&self) -> &'static str { "" }
    fn default_severity(&self) -> Severity { Severity::Error }
    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation>;
}
```

Rules receive:
- `FileContext` - Path, content, test detection, module path
- `syn::File` - Parsed AST

Rules return `Vec<Violation>` with location, message, and optional suggestions.

### ProjectRule Trait

```rust
pub trait ProjectRule: Send + Sync {
    fn name(&self) -> &'static str;
    fn code(&self) -> &'static str;
    fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation>;
}
```

For structural rules that analyze project layout rather than individual files.

### Analyzer

Builder pattern for configuration:

```rust
let analyzer = Analyzer::builder()
    .root("./src")
    .rule(NoUnwrapExpect::new().allow_in_tests(true))
    .rule(NoSyncIo::new())
    .exclude("**/generated/**")
    .config(Config::from_file("arch-lint.toml")?)
    .build()?;

let result = analyzer.analyze()?;
```

### Violation

```rust
pub struct Violation {
    pub code: String,           // "AL001"
    pub rule: String,           // "no-unwrap-expect"
    pub severity: Severity,     // Error, Warning, Info
    pub location: Location,     // file, line, column
    pub message: String,
    pub suggestion: Option<Suggestion>,
    pub labels: Vec<Label>,     // Additional context
}
```

## Suppression Mechanisms

### 1. Standard Rust Attributes

```rust
#[allow(clippy::unwrap_used)]
fn allowed() { v.unwrap(); }
```

### 2. Inline Comments

```rust
// arch-lint: allow(no-sync-io) reason="startup only"
std::fs::read_to_string("config.toml")?;
```

### 3. Configuration File

```toml
[rules.no-unwrap-expect]
enabled = true
severity = "warning"
allow_in_tests = true
```

## Configuration

`arch-lint.toml`:

```toml
[analyzer]
root = "./src"
exclude = ["**/generated/**", "**/vendor/**"]
respect_gitignore = true

[rules.no-unwrap-expect]
enabled = true
severity = "warning"
allow_in_tests = true

[rules.no-sync-io]
enabled = true
allow_patterns = ["tokio::", "async_std::"]
```

## Extending with Custom Rules

```rust
use arch_lint_core::{Rule, FileContext, Violation, Severity, Location};
use syn::visit::Visit;

pub struct MyRule;

impl Rule for MyRule {
    fn name(&self) -> &'static str { "my-rule" }
    fn code(&self) -> &'static str { "PROJ001" }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = MyVisitor::new(ctx);
        visitor.visit_file(ast);
        visitor.violations
    }
}

// Register with analyzer
let analyzer = Analyzer::builder()
    .rule(MyRule)
    .build()?;
```

## Design Principles

### 1. Visitor Pattern for AST Traversal

Rules implement custom visitors using `syn::visit::Visit`:

```rust
impl<'ast> Visit<'ast> for MyVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        // Custom logic
        syn::visit::visit_expr_method_call(self, node);
    }
}
```

### 2. Context Tracking

Visitors maintain state as they traverse:
- `in_test_context` - Inside `#[test]` or `#[cfg(test)]`
- `in_allowed_context` - Inside `#[allow(...)]`

### 3. Fail Gracefully

- Parse errors are logged but don't stop analysis
- Rules run independently - one failure doesn't affect others

### 4. Helpful Diagnostics

- Clear error messages with location
- Actionable suggestions for fixes
- Support for multiple output formats (text, JSON, compact)

## Future Extensions

1. **Auto-fix** - Generate code replacements for violations
2. **LSP Integration** - IDE support via Language Server Protocol
3. **Incremental Analysis** - Only re-analyze changed files
4. **Parallel Execution** - Run rules concurrently
5. **More Built-in Rules**:
   - `no-error-swallowing` - Forbid catching errors without propagation
   - `handler-complexity` - Limit handler function complexity
   - `require-thiserror` - Require `thiserror` for error types
   - `no-panic-in-lib` - Forbid `panic!` in library code

## Testing

```bash
cargo test                    # Run all tests
cargo test -p arch-lint-core  # Core tests only
cargo test -p arch-lint-rules # Rules tests only
```

Each rule includes unit tests with synthetic code examples.

## License

MIT OR Apache-2.0
