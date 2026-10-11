# SDK: Rules in Rust Code

Writing custom `Rule` implementations, and using `Session` / `TsSession` programmatically instead of the CLI or `check!()`. [Back to README](../README.md).

## `check!()` Macro Options

```rust
// Strict preset — all rules enabled
arch_lint::check!(preset = "strict");

// Custom config path
arch_lint::check!(config = "my-lint.toml");

// Fail on warnings too (default: fail on errors only)
arch_lint::check!(fail_on = "warning");

// Combined
arch_lint::check! {
    preset = "strict",
    config = "arch-lint.toml",
    fail_on = "warning",
}
```

## Writing Custom Rules

### Quick Start: RequiredCrateRule

For enforcing required crate usage, use the built-in `RequiredCrateRule` builder:

```rust
use arch_lint_core::RequiredCrateRule;

// Require utoipa over other OpenAPI crates
let rule = RequiredCrateRule::new("PROJ001", "require-utoipa")
    .prefer("utoipa")
    .over(&["paperclip", "okapi", "rweb"])
    .detect_macro_path()  // Detects macro calls
    .severity(Severity::Warning);

// Require tracing over log
let rule = RequiredCrateRule::new("PROJ002", "require-tracing")
    .prefer("tracing")
    .over(&["log"])
    .detect_macro_path();
```

This automatically detects patterns like:
```rust
// BAD
paperclip::path!("/api");
log::info!("message");

// GOOD
utoipa::path!("/api");
tracing::info!("message");
```

Benefits: ~4x less code than manual `Rule` impl, built-in suppression support, consistent error messages.

### Advanced: Custom Rules (Rust Code)

For complex logic, implement the `Rule` trait directly:

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


## Programmatic Usage

`Session` is the SDK entry point. It runs the same pipeline as `check!()` and
the CLI — find the config, resolve the preset and `fail_on`, build the preset
and declarative rules, analyze — but it never panics and never exits the
process.

```bash
cargo add arch-lint
```

```rust
use arch_lint::report::{render, Format};
use arch_lint::Session;

// Config lookup: <root>/arch-lint.toml -> <root>/.arch-lint.toml ->
// $ARCH_LINT_CONFIG_DIR (or ~/.arch-lint/) config.toml -> built-in defaults.
let session = Session::discover(".")?;
println!("config: {:?}", session.config_source());

let fail_on = session.fail_on();
let result = session.run()?;

print!("{}", render(&result, Format::default())?);   // no ANSI unless asked
print!("{}", render(&result, Format::Json)?);        // same JSON as --format json

if result.has_violations_at(fail_on) {
    std::process::exit(1);
}
```

Overrides are builder-style, and every failure is a `SessionError`:

```rust
use arch_lint::rules::Preset;
use arch_lint::{Session, Severity};

let session = Session::from_config_path(".", "ci/arch-lint.toml")?
    .with_preset(Preset::Strict)
    .with_fail_on(Severity::Warning)
    .with_exclude("**/generated/**")
    .with_rule_filter(&["no-unwrap-expect", "AL002"])?;   // unknown name -> Err

let result = session.run()?;
```

`Session::from_config_str(root, toml)` takes configuration from memory, and
`Session::find_workspace_root(path)` performs the `[workspace]` walk that
`check!()` uses. For rules assembled by hand, `Analyzer::builder()` from
`arch-lint-core` is still available and is what `Session` drives underneath.

`Session::with_baseline(baseline)` plus `run_with_baseline()` (instead of
`run()`) apply a [`Baseline`](cli.md#adopting-on-an-existing-codebase) and return
an `Applied { result, suppressed, stale }`, where `result` carries only the
violations the baseline did not match:

```rust
use arch_lint::baseline::Baseline;
use arch_lint::Session;

let baseline = Baseline::load("arch-lint.baseline.toml")?;
let applied = Session::discover(".")?.with_baseline(baseline).run_with_baseline()?;
eprintln!("baseline: {} suppressed, {} stale", applied.suppressed, applied.stale.len());
```

### Cross-language (tree-sitter)

`TsSession` is the same shape for the tree-sitter engine:

```rust
use arch_lint::report::{render, Format};
use arch_lint_ts::{ArchConfig, TsSession};
use std::path::Path;

let config = ArchConfig::from_file(Path::new("arch-lint.toml"))?;

let result = TsSession::new(config, ".")
    .respect_gitignore(true)     // default; also read from [analyzer]
    .run()?;

print!("{}", render(&result, Format::Compact)?);
```

Register additional languages with `.with_extractor(Box::new(MyExtractor))`;
the built-in Kotlin extractor stays registered.

