# arch-lint

Architecture linter for Rust projects and cross-language layer enforcement.

[![Crates.io](https://img.shields.io/crates/v/arch-lint.svg)](https://crates.io/crates/arch-lint)
[![License](https://img.shields.io/crates/l/arch-lint.svg)](LICENSE-MIT)
[![CI](https://github.com/ynishi/arch-lint/actions/workflows/ci.yml/badge.svg)](https://github.com/ynishi/arch-lint/actions/workflows/ci.yml)

## Why arch-lint?

Code review catches a problem once. arch-lint catches it forever.

In the age of AI-assisted coding, both humans and LLMs produce the same architectural violations repeatedly:

- `.unwrap()` calls sneaking into production code
- Blocking I/O in async contexts
- Domain layer importing infrastructure directly
- Errors being swallowed with just logging
- Wrong crate used when the team has a preferred alternative

**The first time is understandable. The second time is a system failure.** Relying on willpower, diligence, or "be more careful next time" does not scale. arch-lint encodes review feedback as machine-enforceable rules so that the same mistake never requires human (or AI) attention again.

### How It Works

1. A PR review catches an architectural violation
2. You encode the rule in `arch-lint.toml` (TOML only, no code required) or as a custom `Rule`
3. `cargo test` fails on every future occurrence — automatically

### Key Features

- **`cargo test` integration** - `arch_lint::check!()` runs all rules as part of your test suite
- **Declarative rules** - Define architecture constraints in TOML without writing Rust code
- **Dual engine** - syn (Rust AST) + Tree-sitter (Kotlin, and more to come)
- **Layer enforcement** - TOML-defined scopes with dependency rules
- **Mandatory reasoning** - Critical violations require documented reasons when suppressed
- **CI-friendly** - JSON output, exit codes, and clear violation reporting

## Quick Start (30 seconds)

### 1. Add dependency

```bash
cargo add arch-lint --dev
```

### 2. Generate config

```bash
cargo install arch-lint-cli
arch-lint init
```

This creates `arch-lint.toml` with sensible defaults (unwrap/sync-io checks enabled).

### 3. Create the test gate

```rust
// tests/architecture.rs
arch_lint::check!();  // expands to a #[test] function
```

### 4. Run

```bash
cargo test
```

Every rule violation now fails your test suite. Done.

### Options

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

## Declarative Rules (No Code Required)

Define architecture constraints directly in `arch-lint.toml`. When a review catches a pattern you want to prevent, add a TOML block — no Rust code needed.

### Scope-Based Dependency Control

```toml
# Define architectural scopes
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "handler"
paths = ["src/handler/**"]

# Domain must not import infrastructure
[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain layer must not depend on infrastructure. Use ports/adapters."

# Domain must not use database crates directly
[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::*", "diesel::*", "sea_orm::*"]
message = "Database access belongs in the infra layer."
```

Both `[[restrict-use]]` and `[[deny-scope-dep]]` check `use` imports **and**
inline qualified paths (e.g. `sqlx::query(...)` or `crate::infra::Db::new()`
written without a `use`). To restrict a rule to `use` statements only, set
`check-inline = false` on that rule.

`[[restrict-use]]` accepts an `except` list — a path that matches it is never
reported, even when it matches `deny`. `[[scopes]]` accepts an `exclude` list —
a file matching it is not part of the scope even when it matches `paths`, and
that applies to every rule kind that uses the scope:

```toml
[[scopes]]
name = "domain"
paths = ["src/domain/**"]
exclude = ["src/**/tests/**"]   # test helpers are not domain code

[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::**"]
except = ["sqlx::types::*"]     # the type wrappers are fine
message = "Database access belongs in the infra layer."
```

### Ordered Layers

```toml
[[layering]]
name = "clean-arch"                    # optional, defaults to "layering"
order = ["handler", "domain", "infra"] # scope names, top to bottom
adjacent_only = false                  # optional, default false
message = "Layers may only depend downward."
severity = "error"                     # optional, default error
check-inline = true                    # optional, default true
```

Each entry of `order` must be a declared `[[scopes]]` name; at least two,
no duplicates. A layer may depend on any layer **below** it — referencing a
layer above it is a violation (ALD004), and with `adjacent_only = true` so is
skipping past the next layer down. One `[[layering]]` replaces the
`[[deny-scope-dep]]` matrix you would otherwise write by hand.

> Use `[[layering]]`, not `[[layers]]`: the latter key selects the
> tree-sitter engine.

### Attributes on Items

```toml
[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"                       # or files = [...]
attrs = ["derive(sqlx::FromRow)", "serde::Serialize"]
on = ["struct", "enum"]                # optional, default: every kind
message = "Domain types must stay ORM-agnostic."
severity = "error"

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]        # the item must carry at least one
on = ["fn"]
visibility = "pub"                     # optional: "pub" (default) | "any"
message = "Handlers must be instrumented."
severity = "warning"                   # optional, default warning (deny-attr defaults to error)
```

`on` accepts `struct`, `enum`, `fn`, `trait`, `impl`, `mod`, `type`, `const`,
`static`, `union`. Only module-level and impl-level items are inspected —
items declared inside a function body are ignored.

**How an attribute pattern matches.** The attribute's own path is rendered with
`::` (`#[tracing::instrument]` → `tracing::instrument`, `#[derive(..)]` →
`derive`) and compared to the pattern. A pattern written as `derive(P)` matches
only derive lists; a bare pattern matches the attribute path *or* an entry of a
derive list. A derive entry matches when it equals the pattern in full or when
its **last segment** equals the pattern — so `FromRow` matches both
`#[derive(FromRow)]` and `#[derive(sqlx::FromRow)]`, while `sqlx::FromRow`
matches only `#[derive(sqlx::FromRow)]`.

**Visibility.** `visibility = "pub"` selects items declared `pub`. Items of a
trait definition inherit the trait's visibility, and items of a `impl Trait for
T` block (and `impl` blocks themselves) count as public.

### Naming

```toml
[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"                          # struct|enum|trait|fn|mod|type|const|static
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
visibility = "pub"                      # optional: "pub" (default) | "any"
message = "Domain traits are ports: name them *Repository or *Port."
severity = "warning"                    # optional, default warning
```

Every item of that kind whose identifier does not match the regex is reported
(ALD007). An invalid regex is a config error, not a silent skip.

### Return Types

```toml
[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
visibility = "pub"                      # optional, default "pub"
async = true                            # optional: only async (or only sync) fns
pattern = "^Result<.*,\\s*AppError>$"
message = "Handlers must return Result<_, AppError>."
severity = "error"
```

The return type is rendered from the AST and normalized before matching: no
spaces around `<`, `>` and `::`, exactly one space after `,`. So
`Result < Vec < u8 > , AppError >` is matched as `Result<Vec<u8>, AppError>`,
and a function with no return type is matched as `()`. Free functions and
impl/trait methods are checked; nested functions are not.

### Crate Preference Enforcement

```toml
# Team decided: use tracing, not log
[[require-use]]
name = "prefer-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing instead of log for structured logging."
```

### Real-World Example: Review Feedback to Rule

**Before** — a reviewer has to say this every time:

> "Don't call sqlx directly from the domain layer. Use the repository trait."

**After** — add to `arch-lint.toml`:

```toml
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::*"]
message = "Use repository traits instead of direct DB access in domain."
```

Now `cargo test` catches it automatically. The reviewer never has to say it again. The AI never generates it unchecked again.

## AI Coding Integration

arch-lint is designed to work as a guardrail for AI-generated code. Since `check!()` integrates into `cargo test`, any AI coding workflow that runs tests automatically gets arch-lint enforcement for free:

- **Claude Code / Cursor / Copilot** — AI runs `cargo test` as part of its feedback loop; arch-lint violations surface immediately
- **CI/CD** — AI-generated PRs are checked by the same rules as human PRs
- **CLAUDE.md / .cursorrules** — Point the AI at `arch-lint.toml` to explain project constraints upfront

The goal: the AI learns from the same rule set that humans follow, and neither needs to remember past review feedback.

## Available Rules

| Code | Name | Description | Default |
|------|------|-------------|---------|
| AL001 | `no-unwrap-expect` | Forbids `.unwrap()` and `.expect()` in production code | Error |
| AL002 | `no-sync-io` | Forbids blocking I/O operations | Error |
| AL003 | `no-error-swallowing` | Forbids catching errors with only logging | Error |
| AL004 | `handler-complexity` | Limits handler function complexity | Warning |
| AL005 | `require-thiserror` | Requires `thiserror` derive for error types | Error |
| AL006 | `require-tracing` | Requires `tracing` crate instead of `log` crate | Warning |
| AL007 | `tracing-env-init` | Prevents hardcoded log levels in tracing initialization | Warning |
| AL009 | `async-trait-send-check` | Checks proper usage of `async_trait` Send bounds | Warning |
| AL010 | `prefer-from-over-into` | Prefers `From` trait implementation over `Into` | Warning |
| AL011 | `no-panic-in-lib` | Forbids panic macros in library code | Error |
| AL012 | `require-doc-comments` | Requires documentation comments on public items | Warning |
| AL013 | `no-silent-result-drop` | Forbids silently discarding `Result` error information | Warning |

Declarative rules defined in `arch-lint.toml` (see [Declarative Rules](#declarative-rules-no-code-required)):

| Code | Section | Description |
|------|---------|-------------|
| ALD001 | `[[restrict-use]]` | Forbids `use` paths (and inline qualified paths) within a scope |
| ALD002 | `[[require-use]]` | Requires a preferred crate over alternatives within a file set |
| ALD003 | `[[deny-scope-dep]]` | Forbids one scope from depending on another |
| ALD004 | `[[layering]]` | Forbids upward (and, with `adjacent_only`, layer-skipping) dependencies in an ordered layer stack |
| ALD005 | `[[deny-attr]]` | Forbids given attributes on items of a scope |
| ALD006 | `[[require-attr]]` | Requires at least one of the given attributes on items of a scope |
| ALD007 | `[[naming]]` | Requires identifiers of one item kind to match a regex |
| ALD008 | `[[require-return]]` | Requires functions to return a type matching a regex |

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

`is_file()` / `is_dir()` are reported only when the receiver is a path. On a
value that came from `file_type()`, `metadata()`, `symlink_metadata()` or
`fs::metadata(..)` (directly, through `?` / `unwrap()` / `ok()`, a `let`
binding in the same block, or a closure parameter such as
`entry.file_type().is_some_and(|ft| ft.is_file())`) they are pure accessors
and are not reported — the `metadata()` call itself still is.

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

#### AL006: require-tracing

Requires `tracing` crate instead of `log` for structured, async-aware logging.

```rust
// BAD
log::info!("Processing request");
log::error!("Error: {}", e);

// GOOD
tracing::info!("Processing request");
tracing::error!("Error: {}", e);
```

**Configuration:**
```toml
[rules.require-tracing]
severity = "warning"
```

#### AL007: tracing-env-init

Prevents hardcoded log levels in tracing initialization so `RUST_LOG` works at runtime.

```rust
// BAD - Hardcoded level
tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::new("debug"))
    .init();

// GOOD - Use environment variable
tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .init();

// GOOD - With fallback
let filter = EnvFilter::try_from_default_env()
    .unwrap_or_else(|_| EnvFilter::new("info"));
```

**Configuration:**
```toml
[rules.tracing-env-init]
severity = "warning"
```

#### AL009: async-trait-send-check

Checks `async_trait` Send bounds — unnecessary `Send` makes traits harder to implement in single-threaded runtimes.

```rust
// BAD - Unnecessary Send bound in single-threaded context
#[async_trait]
trait Handler {
    async fn handle(&self);
}

// GOOD - Explicitly opt out of Send for single-threaded
#[async_trait(?Send)]
trait Handler {
    async fn handle(&self);
}

// GOOD - Explicit Send bound for multi-threaded
#[async_trait]
trait Service: Send + Sync {
    async fn process(&self);
}
```

**Configuration:**
```toml
[rules.async-trait-send-check]
severity = "warning"
runtime_mode = "single-thread"  # "single-thread" or "multi-thread"
```

#### AL010: prefer-from-over-into

Prefers `From` over `Into` — implementing `From` gives you `Into` for free via blanket impl.

```rust
// BAD - Implementing Into directly
impl Into<String> for MyType {
    fn into(self) -> String {
        self.0
    }
}

// GOOD - Implement From, get Into for free
impl From<MyType> for String {
    fn from(value: MyType) -> String {
        value.0
    }
}
```

**Configuration:**
```toml
[rules.prefer-from-over-into]
severity = "warning"
```

#### AL011: no-panic-in-lib

Forbids panic macros (`panic!`, `todo!`, `unimplemented!`, `unreachable!`) in library code — return `Result` instead.

```rust
// BAD - Panicking in library code
pub fn parse_config(input: &str) -> Config {
    let value = input.parse().unwrap();
    todo!("implement parsing");
    unimplemented!();
}

// GOOD - Return Result instead
pub fn parse_config(input: &str) -> Result<Config, ParseError> {
    let value = input.parse()
        .map_err(|_| ParseError::InvalidInput)?;
    Ok(Config { value })
}
```

**Configuration:**
```toml
[rules.no-panic-in-lib]
severity = "error"
allow_in_tests = true  # Allow panic macros in test code
```

#### AL012: require-doc-comments

Requires `///` on public items — makes `cargo doc` output useful and forces API design thinking.

```rust
// BAD - No documentation
pub fn process_data(input: &[u8]) -> Result<Output> {
    // ...
}

pub struct Config {
    name: String,
}

// GOOD - Documented
/// Processes the input data and returns the result.
///
/// # Errors
/// Returns `ProcessError` if the input is invalid.
pub fn process_data(input: &[u8]) -> Result<Output> {
    // ...
}

/// Configuration data for the application.
pub struct Config {
    /// Name of the configuration.
    name: String,
}
```

**Configuration:**
```toml
[rules.require-doc-comments]
severity = "warning"
require_fn_docs = true     # Require docs for public functions
require_struct_docs = true # Require docs for public structs
require_enum_docs = true   # Require docs for public enums
```

#### AL013: no-silent-result-drop

Detects `Result` values whose error is discarded without being handled or logged.

```rust
// BAD
let _ = save(&data);
save(&data).ok();

// GOOD
save(&data)?;
if let Err(e) = save(&data) {
    tracing::warn!(error = %e, "save failed, continuing");
}
```

**Configuration:**
```toml
[rules.no-silent-result-drop]
allow_in_tests = true        # Allow in test code (default: true; `strict` sets false)
allow_ok = false             # Allow `.ok()` conversion (default: false)
allow_let_underscore = false # Allow `let _ = result` (default: false)
severity = "warning"
```

## Configuration

Create `arch-lint.toml` in your project root:

```toml
# Preset: "recommended" (default), "strict", or "minimal"
preset = "recommended"

# Fail threshold: "error" (default), "warning", or "info"
fail_on = "error"

[analyzer]
root = "./src"
exclude = [
    "**/target/**",
    "**/generated/**",
    "**/vendor/**",
]
respect_gitignore = true
# include = ["src/app/**", "src/domain/**"]

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

[rules.require-tracing]
enabled = true
severity = "warning"

[rules.tracing-env-init]
enabled = true
severity = "warning"

# --- Declarative rules (no Rust code needed) ---

[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "handler"
paths = ["src/handler/**"]
exclude = ["src/**/tests/**"]

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infrastructure."

[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::*", "diesel::*"]
message = "Direct DB access is not allowed in domain layer."

[[require-use]]
name = "prefer-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing instead of log."

[[layering]]
order = ["handler", "domain", "infra"]
message = "Layers may only depend downward."

[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"
attrs = ["derive(sqlx::FromRow)"]
on = ["struct", "enum"]
message = "Domain types must stay ORM-agnostic."

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]
on = ["fn"]
message = "Handlers must be instrumented."

[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
message = "Domain traits are ports."

[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
async = true
pattern = "^Result<.*,\\s*AppError>$"
message = "Handlers must return Result<_, AppError>."
```

### `[analyzer]` options

| Key | Default | Meaning |
|-----|---------|---------|
| `root` | `"."` | Directory to walk. |
| `respect_gitignore` | `true` | Skip files git ignores. |
| `include` | `[]` (all `*.rs`) | Glob patterns; when non-empty, only matching files are analyzed. |
| `exclude` | `["**/target/**", "**/vendor/**"]` | Glob patterns dropped from the result. |
| `parallelism` | unset | **Reserved** — parsed, but analysis currently runs sequentially. |

File discovery runs in this order:

1. Walk `root` (hidden files and directories are traversed, matching the
   historical `**/*.rs` glob behavior).
2. If `respect_gitignore = true`, skip anything ignored by a `.gitignore` at
   any level, by `.git/info/exclude`, or by the global gitignore. Ignore rules
   are applied even when `root` is **not** inside a git repository, so a
   scaffolded project or a fixture directory carrying a `.gitignore` is
   filtered the same way a checked-out repository is. Non-git ignore files
   (`.ignore`, `.rgignore`) are never consulted.
3. Keep `*.rs` files matching at least one `include` pattern (all of them when
   `include` is empty).
4. Drop files matching any `exclude` pattern.

`include` and `exclude` patterns are matched against each file's path
**relative to `root`** as well as against its absolute path, so both
`exclude = ["src/generated/**"]` and `exclude = ["**/generated/**"]` work. A
leading `./` on a pattern is accepted (`./src/**` ≡ `src/**`).

`Cargo.toml` discovery — used by project-wide rules such as `required-crate` —
follows the same `respect_gitignore` and `exclude` settings, but ignores
`include`, which scopes source files only.

## Suppression

arch-lint provides multiple ways to suppress violations at different scopes.

### Scope Overview

| Scope | Method | Use Case |
|-------|--------|----------|
| Line | Comment | Single expression |
| Block | `#[arch_lint::allow(...)]` | Function, impl, module |
| File | Configuration (`exclude_files`) | Entire file |
| Global | Configuration (`enabled = false`) | Project-wide exclusion |

Both directives apply to the built-in rules (AL001-AL013) **and** to the
declarative rules from `arch-lint.toml` (ALD001-ALD008); see
[Declarative rules](#declarative-rules-ald001-ald008) below for the names a
declarative rule answers to.

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

### Mandatory Reasoning

For `Severity::Error` rules (AL001, AL002, AL003, AL005), the `reason` parameter is **required**. Omitting it will generate a `Severity::Warning` violation:

```rust
// BAD — reason omitted → generates a Warning violation
#[arch_lint::allow(no_unwrap_expect)]
fn bad() { ... }

// GOOD — reason provided
#[arch_lint::allow(no_unwrap_expect, reason = "Config validated at startup")]
fn good() { ... }
```

This ensures that critical suppressions are always documented and justified.

The reason is required on the **comment** path: a comment directive without a
reason on an error rule suppresses the violation and reports a warning
(`Allow directive for '<rule>' is missing required reason`) in its place. An
attribute without a reason suppresses silently.

### Declarative rules (ALD001-ALD008)

The declarative rules honor the same two directives, with the same semantics.
A declarative rule answers to three names, each in kebab-case or `snake_case`:

| Name | Example | Suppresses |
|------|---------|------------|
| Rule kind | `restrict-use` / `restrict_use` | every `[[restrict-use]]` rule |
| TOML `name` | `no-sqlx-in-domain` | only that one TOML rule |
| Rule code | `ALD001` | every rule reported under that code |

`allow(all)` suppresses everything, as it does for the built-in rules.

```rust
// Comment on the violating line or the line above it
// arch-lint: allow(no-sqlx-in-domain) reason="legacy row mapping"
use sqlx::Pool;

// Attribute on the offending item
#[arch_lint::allow(no_orm_derives_in_domain, reason = "legacy row mapping")]
#[derive(sqlx::FromRow)]
pub struct User { pub id: i64 }

// Attribute on an enclosing mod / fn / impl, covering everything inside it
#[arch_lint::allow(layering, reason = "composition root wires the layers")]
pub fn wire() {
    let _ = crate::handler::api::list_users;
}
```

Unlike some built-in rules, declarative rules are **not** switched off inside
`#[cfg(test)]` modules or `#[test]` functions: they express architecture
constraints, so test code is held to them too. Take a directory out of a scope
with `exclude` on `[[scopes]]` if that is not what you want.

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
| `recommended` | AL001, AL002, AL003, AL005, AL006, AL007, AL013 | Sensible defaults |
| `strict` | All rules (AL001-AL013), no test exceptions, `.expect()` and `.ok()` forbidden | Maximum safety |
| `minimal` | AL001 (relaxed, `.expect()` allowed) | Gradual adoption |

Declarative rules from `arch-lint.toml` always run in addition to the preset.
Per-rule options under `[rules.<name>]` override the preset defaults, and
`exclude_files` skips a rule for matching files (globs relative to the analyzer root).

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

For direct API access without the `check!()` macro:

```bash
cargo add arch-lint-core arch-lint-rules --dev
```

```rust
use arch_lint_core::Analyzer;
use arch_lint_rules::{
    NoUnwrapExpect, NoSyncIo, HandlerComplexity, RequireTracing,
    TracingEnvInit, AsyncTraitSendCheck, RuntimeMode, PreferFromOverInto,
    NoPanicInLib, RequireDocComments,
};

let analyzer = Analyzer::builder()
    .root("./src")
    .rule(NoUnwrapExpect::new().allow_in_tests(true))
    .rule(NoSyncIo::new())
    .rule(HandlerComplexity::new().max_match_arms(15))
    .rule(RequireTracing::new())
    .rule(TracingEnvInit::new())
    .rule(AsyncTraitSendCheck::new().runtime_mode(RuntimeMode::SingleThread))
    .rule(PreferFromOverInto::new())
    .rule(NoPanicInLib::new())
    .rule(RequireDocComments::new())
    .exclude("**/generated/**")
    .build()?;

let result = analyzer.analyze()?;

if result.has_errors() {
    result.print_report();
    std::process::exit(1);
}
```

## CLI Usage

```bash
cargo install arch-lint-cli

arch-lint init                            # Generate arch-lint.toml
arch-lint init --ts                       # Generate with tree-sitter layers
arch-lint check                           # Run all checks
arch-lint check --rules no-unwrap-expect  # Run specific rules
arch-lint check --format json             # JSON output for CI
arch-lint check --engine ts               # Force tree-sitter engine
arch-lint list-rules                      # Show available rules
```

### Cross-language (tree-sitter engine)

```bash
arch-lint init --ts
# Edit [[layers]] and [dependencies] in arch-lint.toml
arch-lint check              # auto-detects engine from [[layers]]
arch-lint check --engine ts  # explicit engine selection
```

See [docs/tree-sitter-engine.md](docs/tree-sitter-engine.md) for full documentation.

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
| `arch-lint-rules` | Built-in lint rules (syn engine) |
| `arch-lint-ts` | Tree-sitter engine (cross-language layer enforcement) |
| `arch-lint-cli` | Command-line interface |
| `arch-lint-macros` | Procedural macros (`#[arch_lint::allow(...)]`) |

## Comparison with Other Tools

| Tool | Focus | AST-based | Custom Rules | `cargo test` | Cross-language |
|------|-------|-----------|--------------|--------------|----------------|
| **arch-lint** | Architecture patterns | Yes | Easy (TOML or Rust) | Yes (`check!()`) | Yes (Tree-sitter) |
| Clippy | Code quality | Yes | Hard | No | No |
| cargo-deny | Dependencies | No | Config | No | No |
| ArchUnit | Layer enforcement | Yes | Java DSL | N/A | JVM only |
| deptry | Import checking | No | Config | No | Python only |

arch-lint complements Clippy by focusing on **architectural patterns** rather than code style.
The tree-sitter engine extends this to non-Rust languages (Kotlin first, more planned).

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
