# arch-lint Architecture

Architecture linter with dual engines: syn (Rust AST) and Tree-sitter (cross-language).

## Overview

arch-lint is designed to catch architectural violations and enforce coding patterns that are often missed by both humans and AI during code review. It provides two complementary engines:

- **syn engine** - Per-file Rust AST analysis: built-in rules (AL001-AL013) and TOML-declared rules (ALD001-ALD008)
- **tree-sitter engine** - Cross-language layer dependency enforcement (LAYER001, PATTERN001, NAMING001)

## Crate Structure

```
arch-lint/
├── crates/
│   ├── arch-lint/            # Facade + driver: Session, report, check!() runner
│   ├── arch-lint-core/       # Core framework (traits, analyzer, config, declarative rules)
│   ├── arch-lint-rules/      # Built-in syn rules (AL001-AL013) + presets/registry
│   ├── arch-lint-ts/         # Tree-sitter engine (layer enforcement) + TsSession driver
│   ├── arch-lint-cli/        # CLI binary (argument parsing, engine dispatch)
│   └── arch-lint-macros/     # Procedural macros: #[arch_lint::allow], arch_lint::check!()
```

Dependency order (also the publish order): `macros` → `core` → `rules` → `ts` → `arch-lint` → `cli`.

### arch-lint-core

The foundation of the linting framework. Provides:

- **`Rule` trait** - Per-file AST-based lint rules
- **`ProjectRule` trait** - Project-wide structural rules
- **`Analyzer`** - Orchestrates file discovery and rule execution
- **`Violation`** - Represents lint findings with location and suggestions
- **`Config`** - TOML-based configuration (`preset`, `fail_on`, `[analyzer]`, `[rules.*]` with `enabled` / `severity` / `exclude_files` / rule options)
- **`declarative`** - TOML-declared rules: `[[scopes]]` + `[[restrict-use]]` (ALD001), `[[require-use]]` (ALD002), `[[deny-scope-dep]]` (ALD003), `[[layering]]` (ALD004), `[[deny-attr]]` (ALD005), `[[require-attr]]` (ALD006), `[[naming]]` (ALD007), `[[require-return]]` (ALD008). Pipeline: `config_dto` (serde) → `loader` (validation) → `model` (domain) → `rules` (visitors)
- **`ScopeGraph`** - Dependency graph between declared scopes (nodes = `[[scopes]]` names, edges = scope-crossing `use`/inline references with their sites). Collected once per `Analyzer::analyze()` by a `ScopeEdgeCollector` visitor that runs inside the existing per-file pass. Path resolution is the one ALD003 uses (`crate::`, `self::` and `super::` paths — the latter two rewritten relative to the file's module, inline `mod` blocks included — against `src/x/y.rs` / `src/x/y/mod.rs` candidates; bare paths and `pub use` re-exports are not followed); unlike ALD003 the graph records every reference, regardless of `allow` suppressions and of `check_inline` — rules built on the graph decide what to filter. Reachable via `ProjectContext::scope_graph()` (for a `ProjectRule`), `Analyzer::scope_graph()` (standalone, no rule run), and `Session::scope_graph()` (the SDK driver); enabled by `AnalyzerBuilder::declarative(Arc<DeclarativeConfig>)`. Prerequisite for cycle detection / independence / allow-list rules and `arch-lint graph` (separate issues)
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
| AL006 | `require-tracing` | Requires `tracing` instead of `log` |
| AL007 | `tracing-env-init` | Prevents hardcoded log levels in tracing init |
| AL009 | `async-trait-send-check` | Checks `async_trait` Send bounds |
| AL010 | `prefer-from-over-into` | Prefers `From` over `Into` impls |
| AL011 | `no-panic-in-lib` | Forbids panic macros in library code |
| AL012 | `require-doc-comments` | Requires docs on public items |
| AL013 | `no-silent-result-drop` | Forbids silently discarding `Result` errors |

`presets.rs` is the registry: it maps names/codes to constructors, applies
`[rules.<name>]` options, and defines the presets. Every rule must be
registered there to be reachable from `list-rules`, `--rules`, and `strict`.

Presets:
- `Recommended` - AL001, AL002, AL003, AL005, AL006, AL007, AL013 (default)
- `Strict` - All rules; no test exceptions, `.expect()` and `.ok()` forbidden
- `Minimal` - AL001 only (`.expect()` allowed) for gradual adoption

### arch-lint-ts

Tree-sitter based cross-language layer enforcement. Provides:

- **`LanguageExtractor` trait** - Extension point for new languages
- **`KotlinExtractor`** - Kotlin import/class extraction via Tree-sitter
- **`LayerResolver`** - Package-to-layer mapping (longest-prefix-match)
- **`ArchRuleEngine`** - Layer dependency (LAYER001), pattern constraint (PATTERN001), naming convention (NAMING001) checks
- **`ArchConfig`** - TOML deserialization for `[analyzer]` (`root`, `exclude`, `respect_gitignore`), `[[layers]]`, `[dependencies]`, `[[constraints]]`
- **`TsSession`** - The driver: validates the config, discovers files, runs the engine over each one, sorts the result

Internal data flow:

```
Source (.kt) → KotlinExtractor → FileAnalysis → ArchRuleEngine → Vec<Violation>
                            ↑ TsSession drives this loop and collects LintResult
```

`TsSession::new(config, root)` starts with the Kotlin extractor registered;
`.with_extractor(..)` adds more and `.respect_gitignore(bool)` overrides
`[analyzer] respect_gitignore`. File discovery walks the analysis root with the
`ignore` crate (`require_git(false)`, hidden entries kept) and applies
`[analyzer] exclude` as globs against the root-relative path, falling back to
substring matching for patterns that do not match as globs.

See [docs/tree-sitter-engine.md](docs/tree-sitter-engine.md) for full documentation.

### arch-lint-cli

Command-line interface with dual engine dispatch. It is a thin wrapper: the
pipeline itself lives in the `arch-lint` facade (`Session`, `report::render`)
and in `arch-lint-ts` (`TsSession`). The CLI owns argument parsing, engine
detection, colour policy and the exit code.

```bash
arch-lint check [PATH]           # Run lint checks (auto-detect engine)
arch-lint check --engine ts      # Force tree-sitter engine
arch-lint check --format json    # JSON output for CI
arch-lint list-rules             # Show available rules
arch-lint init                   # Create syn config
arch-lint init --ts              # Create tree-sitter config (with [[layers]])
```

Engine auto-detection: the config is parsed as TOML and the tree-sitter engine
is used when a non-empty `[[layers]]` array is present.

`check` (syn engine) selects rules the same way `check!()` does:
`--rules` > `preset` in config > `recommended`, then adds declarative rules
from the config. Both paths go through `Session`, so the selection logic exists
once. It exits non-zero when a violation at or above `fail_on` (config, default
`error`) is found.

ANSI colours in `--format text` are emitted only when stdout is a terminal
(`std::io::IsTerminal`); the same applies to the `tracing` log lines. Redirected
and piped output is plain.

### arch-lint-macros

Procedural macros:
- `#[arch_lint::allow(rule, reason = "...")]` - Block-level suppression attribute
- `arch_lint::check!()` - Generates a `#[test]` that runs the analyzer via `arch_lint::__internal::run_check`

### arch-lint (facade + driver)

Re-exports `arch-lint-core`, `arch-lint-rules` (as `arch_lint::rules`), and the
macros, and hosts the driver layer that turns "root + config" into a
`LintResult`:

- **`session::Session`** - the SDK entry point. `discover(root)` walks the
  config lookup order (`<root>/arch-lint.toml` → `<root>/.arch-lint.toml` →
  `$ARCH_LINT_CONFIG_DIR` or `~/.arch-lint/` `config.toml` → defaults) and
  records the winner in `ConfigSource`; `from_config_path` and
  `from_config_str` cover explicit and in-memory configuration. It resolves
  preset and `fail_on`, loads the declarative rules from the same document,
  applies `with_preset` / `with_fail_on` / `with_rules` / `with_rule_filter` /
  `with_exclude` overrides, and `run()` builds the `Analyzer` and analyzes.
  Every failure is a `SessionError` — the driver never panics and never exits.
- **`session::Session::find_workspace_root`** - the `[workspace]` walk used by
  `check!()`, public and testable.
- **`report::render`** - `LintResult` → text / JSON / compact string. ANSI
  colours are opt-in (`Format::Text { color }`, default `false`), so a library
  caller gets plain output unless it asks otherwise.

`runner.rs` is what `check!()` calls. It is now only the `cargo test` contract:
find the workspace root, build a `Session`, apply the macro's `preset` /
`config` / `fail_on` arguments as overrides, run, and panic with the formatted
report when violations reach `fail_on`.

```
check!()  ─┐
           ├─→ Session::discover → run() → LintResult ─→ report::render
CLI check ─┘
```

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
exclude_files = ["src/cli/**"]   # skip this rule for matching files
```

## Configuration

`arch-lint.toml`:

```toml
preset = "recommended"   # or "strict" / "minimal"
fail_on = "error"        # exit non-zero at this severity or above

[analyzer]
root = "./src"
exclude = ["**/generated/**", "**/vendor/**"]

[rules.no-unwrap-expect]
enabled = true
severity = "warning"
allow_in_tests = true

[rules.no-sync-io]
enabled = true
allow_patterns = ["tokio::", "async_std::"]
```

`[analyzer].include` and `respect_gitignore` are honored by the syn engine.
File discovery walks `root` with the `ignore` crate: gitignored paths are
skipped unless `respect_gitignore = false`, then `include` (if non-empty) keeps
only matching files, then `exclude` drops the rest. `include` and `exclude`
patterns are matched against the path relative to `root` as well as the
absolute path. `[analyzer].parallelism` is still parsed but not honored — file
analysis runs sequentially.

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
4. **Parallel Execution** - Run rules concurrently (`[analyzer].parallelism`)
5. **SARIF / GitHub annotations output** - CI-native reporting
6. **More tree-sitter languages** - Kotlin is the only extractor today

## Testing

```bash
cargo test -p arch-lint-core  # Core tests only
cargo test -p arch-lint-rules # Rules tests only
cargo test -p arch-lint       # check!() integration test on this repo
```

CI (`.github/workflows/ci.yml`) runs rustfmt, clippy (`-D warnings` on lib/bin
targets), per-crate tests, an MSRV check, rustdoc, and cargo-deny.

Each rule includes unit tests with synthetic code examples.

## License

MIT OR Apache-2.0
