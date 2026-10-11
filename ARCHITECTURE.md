# arch-lint Architecture

Architecture linter with dual engines: syn (Rust AST) and Tree-sitter (cross-language).

## Overview

arch-lint is designed to catch architectural violations and enforce coding patterns that are often missed by both humans and AI during code review. It provides two complementary engines:

- **syn engine** - Per-file Rust AST analysis: built-in rules (AL001-AL013) and TOML-declared rules (ALD001-ALD011)
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
- **`declarative`** - TOML-declared rules: `[[scopes]]` + `[[restrict-use]]` (ALD001), `[[require-use]]` (ALD002), `[[deny-scope-dep]]` (ALD003), `[[layering]]` (ALD004), `[[deny-attr]]` (ALD005), `[[require-attr]]` (ALD006), `[[naming]]` (ALD007), `[[require-return]]` (ALD008), `[[acyclic]]` (ALD009), `[[independent]]` (ALD010) and `[[allow-dep]]` (ALD011). Pipeline: `config_dto` (serde) → `loader` (validation) → `model` (domain) → `rules` (visitors). `create_rules_shared()` builds the per-file `RuleBox`es (ALD001-ALD008); `create_project_rules_shared()` builds the three project-wide `ProjectRuleBox`es, `AcyclicRule` (ALD009), `IndependentRule` (ALD010) and `AllowDepRule` (ALD011), which each run once per project against the `ScopeGraph` rather than once per file. The per-site suppression classification all three share (`rules::graph_suppress::classify_site`) lives in its own module rather than being copy-pasted
- **`ScopeGraph`** - Dependency graph between declared scopes (nodes = `[[scopes]]` names, edges = scope-crossing `use`/inline references with their sites). Collected once per `Analyzer::analyze()` by a `ScopeEdgeCollector` visitor that runs inside the existing per-file pass. Path resolution is the one ALD003 uses (`crate::`, `self::` and `super::` paths — the latter two rewritten relative to the file's module, inline `mod` blocks included — against `src/x/y.rs` / `src/x/y/mod.rs` candidates; bare paths and `pub use` re-exports are not followed); unlike ALD003 the graph records every reference regardless of `check_inline` — rules built on the graph decide what to filter. Every `EdgeSite` does carry every `allow` directive in scope at it, each tagged with a `DirectiveSource` (`Scope`/`Item`/`Comment`) saying where it came from (enclosing `mod`/`fn`/`impl`/`trait` attributes, the site's own item attributes, or the inline comment), parsed with the same functions `Suppressions` uses for the per-file rules, rule names normalized to `snake_case`, reachable through `EdgeSite::allow_directive` / `allow_directives`; `AcyclicRule` is the first consumer, dropping a site from its working graph when a directive names it, and — only when every matching directive on that site is `Comment`-sourced and none carries a reason, on an error-severity rule, for a site that actually sat on a cycle before suppression — reporting a missing-reason warning (mirroring `Suppressions::decide`) in its place instead. `IndependentRule` (ALD010) and `AllowDepRule` (ALD011) reuse the same per-site classification (every matching edge is offending by definition, so their missing-reason warning fires unconditionally rather than only when the site sat on a cycle). Reachable via `ProjectContext::scope_graph()` (for a `ProjectRule`), `Analyzer::scope_graph()` (standalone, no rule run), and `Session::scope_graph()` (the SDK driver); enabled by `AnalyzerBuilder::declarative(Arc<DeclarativeConfig>)`. `[[acyclic]]` (ALD009) is the cycle-detection consumer (Tarjan SCC over the induced sub-graph); `[[independent]]` (ALD010) is the pairwise-independence consumer (one violation per offending edge within a named scope set); `[[allow-dep]]` (ALD011) is the allow-list consumer (one violation per edge out of `from` landing outside its declared `to`); `arch-lint graph` (below, under `arch-lint` facade + driver) is the rendering consumer
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
cargo arch-lint check [PATH]           # Run lint checks (auto-detect engine)
cargo arch-lint check --engine ts      # Force tree-sitter engine
cargo arch-lint check --format json    # JSON output for CI
cargo arch-lint list-rules             # Show available rules
cargo arch-lint init                   # Create syn config
cargo arch-lint init --ts              # Create tree-sitter config (with [[layers]])
cargo arch-lint graph [PATH]            # Render the scope graph (Mermaid; syn engine only)
cargo arch-lint graph --format dot      # Same, as Graphviz DOT
```

`crates/arch-lint-cli` builds two binaries from the same `main.rs`:
`cargo-arch-lint` (so `cargo arch-lint ...` works, Cargo's convention for
external subcommands — it strips the leading `arch-lint` argument cargo
passes before handing the rest to clap) and the bare `arch-lint` binary,
which accepts the same arguments directly. Both share one `Cli` definition;
only the clap `bin_name` used in `Usage:` lines differs between them.

Engine auto-detection: the config is parsed as TOML and the tree-sitter engine
is used when a non-empty `[[layers]]` array is present. `graph` does not
auto-detect: a `[[layers]]` configuration is rejected outright, since there is
no scope graph to draw for the tree-sitter engine.

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
- **`report::render`** - `LintResult` → text / JSON / compact / SARIF
  2.1.0 string. ANSI colours are opt-in (`Format::Text { color }`, default
  `false`), so a library caller gets plain output unless it asks otherwise.
  `Format::Sarif` is built from a small typed model in the private
  `report::sarif` submodule — `render` stays the single entry point.
  `Format` is `#[non_exhaustive]` (since `Format::Sarif` was added), so a
  caller outside this crate matching on it needs a wildcard arm.
- **`graph::render`** - `ScopeGraph` → Mermaid or Graphviz DOT text
  (`GraphFormat`), both plain string writers with no external dependency,
  the output raw (no fence of its own). Every Mermaid node id is prefixed
  `s_` so a scope named after a Mermaid keyword (`end`, `style`, ...)
  cannot be parsed as that keyword instead of a node id. `graph::violating_edges`
  maps a `LintResult` to the `(from, to)` pairs a renderer should style as
  violating: a violation whose code is `ALD003`, `ALD004`, `ALD009`,
  `ALD010` or `ALD011` and whose primary location or a label's location
  names the same `file`/`line`/`column` as one of the edge's `EdgeSite`
  locations (`offset`/`length` excluded from the comparison). The CLI's
  `graph` subcommand calls `Session::scope_graph` (borrows) before
  `Session::run` (consumes) to get both from one configuration.
- **`baseline` module** - `Baseline` (freeze today's violations so a gate can
  turn on before a codebase is clean), `Entry`, `Applied`, `BaselineError`,
  the standalone `fingerprint(&Violation) -> String` and
  `apply(LintResult, &Baseline) -> Applied` functions, and
  `Session::{with_baseline, run_with_baseline}`. A violation's identity is
  `(code, file, message)` — `fingerprint` joins them into one readable
  string, computed centrally rather than per-rule (`Violation`'s fields are
  all `pub` and the struct is not `#[non_exhaustive]`, so a rule-emitted
  `fingerprint` field would have broken every existing `Violation::new(...)`
  struct literal) — plus a per-file count kept in `Entry`, the same scheme
  PHPStan uses and deliberately not line-keyed, so an edit above a
  baselined violation does not invalidate it. `apply` consumes one count
  per matching entry per violation, keeping the unmatched remainder as new
  and reporting any leftover baseline count as `Applied::stale`. The CLI's
  `check --baseline` / `--update-baseline` and the `check!(baseline = ...)`
  macro option are both built on this; `fingerprint` is meant to double as
  the SARIF `partialFingerprints` value (issue #11).

`runner.rs` is what `check!()` calls. It is now only the `cargo test` contract:
find the workspace root, build a `Session`, apply the macro's `preset` /
`config` / `fail_on` / `baseline` arguments as overrides, run (applying the
baseline when given), and panic with the formatted report — a trailing
`baseline: N suppressed, M stale` line included when a baseline is active —
when violations reach `fail_on`.

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
- Support for multiple output formats (text, JSON, compact, SARIF)

## Future Extensions

1. **Auto-fix** - Generate code replacements for violations
2. **LSP Integration** - IDE support via Language Server Protocol
3. **Incremental Analysis** - Only re-analyze changed files
4. **Parallel Execution** - Run rules concurrently (`[analyzer].parallelism`)
5. **GitHub annotations output** (`::error` workflow commands) - SARIF (issue
   #11) already covers CI-native reporting; this would be the older,
   capped-at-10-per-severity alternative, not an upgrade to it
6. **More tree-sitter languages** - Kotlin is the only extractor today

## Testing

```bash
cargo test -p arch-lint-core  # Core tests only
cargo test -p arch-lint-rules # Rules tests only
cargo test -p arch-lint       # check!() integration test on this repo
```

CI (`.github/workflows/ci.yml`) runs rustfmt, clippy (`-D warnings` on lib/bin
targets), per-crate tests, an MSRV check, rustdoc, and cargo-deny.
`.github/workflows/release.yml` is separate: on an `arch-lint-v*` tag push it
builds prebuilt binaries for six targets and opens a draft GitHub Release; it
never runs `cargo publish`. A manual `workflow_dispatch` run instead only
builds and uploads the same archives as workflow artifacts, without creating
or touching a release — a dry run of the build step alone (issue #14).
`.github/workflows/self-check.yml` is a third: on every pull request and on
push to `main` it runs this repository's own `action.yml`
(`uses: ./`) against `examples/sample-project`, once with `--format sarif`
and once without, both with `upload-sarif: false` so the fixture's
intentional violations never reach this repository's own code-scanning tab
— it is checking that the action (and the SARIF file it writes) works, not
that the fixture is clean (issue #15).

The repository ships two more public-facing entry points consumed by other
projects rather than by this one's own CI: `action.yml` at the repository
root, a composite GitHub Action (`taiki-e/install-action` to install
`arch-lint-cli`, run `cargo arch-lint`, `github/codeql-action/upload-sarif`
to report), and `.pre-commit-hooks.yaml`, one `language: system` hook
(`entry: arch-lint check --format compact`) for the pre-commit hook
catalogue — `system` rather than `rust` because this repository's root
`Cargo.toml` is a virtual manifest, which `language: rust`'s own install
step cannot build from; `system` installs nothing, so the caller needs
`arch-lint-cli` on `PATH` already (issue #15).

Each rule includes unit tests with synthetic code examples.

## License

MIT OR Apache-2.0
