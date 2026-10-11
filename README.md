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
cargo install --locked arch-lint-cli
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

### Cycles between scopes

```toml
[[acyclic]]
scopes = ["domain", "app", "infra"]   # omitted = every declared scope
severity = "error"                    # optional, default error
```

Project-wide, not per-file: after every file has been analyzed, `[[acyclic]]`
(ALD009) looks at the dependency graph between `[[scopes]]` and reports one
violation per cycle, however many scopes it passes through — the message
names every scope in the cycle plus the concrete path in order, and the text
report shows the other edge(s) of the cycle as `= note:` lines (the JSON
report carries the same information as `Violation.labels`), e.g.:

```text
ALD009 acyclic at src/domain/service.rs:1:23
  error: Cycle between scopes domain, infra: domain → infra → domain
  = note: infra → domain at src/infra/db.rs:1:29
```

`scopes` restricts the check to a subset of the declared scopes (a cycle that
only passes through an excluded scope is not reported); omit it to check
every declared scope. Overlapping `[[acyclic]]` blocks each report the same
cycle once — one violation per block that includes it, not a single merged
one.

An `// arch-lint: allow(acyclic) reason="..."` comment on the line, or an
`#[arch_lint::allow(acyclic, reason = "...")]` attribute on the `use` item
itself or on an enclosing `mod`/`fn`/`impl`/`trait`, removes that one site
from the graph before cycles are computed — same as `[[deny-scope-dep]]`
honors `allow` directives. Whether a `reason` is required depends on where
the directive is: an attribute never needs one, on an item or an enclosing
scope, same as everywhere else in arch-lint; a comment does, on an
error-severity rule. A comment directive without a reason that suppresses a
site on an edge lying on a cycle (computed before suppression) reports the
usual missing-reason warning at that site instead of silently dropping the
violation, even if another, reasoned `allow` anywhere else on the cycle also
breaks it.

### Independent scopes

```toml
[[independent]]
scopes = ["billing", "shipping", "catalog"]
severity = "error"   # optional, default error
```

Project-wide, like `[[acyclic]]`: after every file has been analyzed,
`[[independent]]` (ALD010) looks at the dependency graph between
`[[scopes]]` and reports one violation per offending edge — any reference
from one of `scopes` to another, in either direction — rather than only a
cycle. This is the "these N scopes must not depend on each other at all"
rule teams otherwise write as `N × (N - 1)` `[[deny-scope-dep]]` entries
that drift as soon as a scope is added; `[[independent]]` needs no other
config change when a scope joins the set. `scopes` is required and needs at
least two entries — unlike `[[acyclic]]`, there is no "every declared scope"
default, since independence is only meaningful for a named set.

```text
ALD010 independent at src/billing/service.rs:1:29
  error: Independent scopes billing and shipping must not depend on each other: billing → shipping
```

Each direction is reported separately: `billing → shipping` and
`shipping → billing` are two violations, one per edge, each at the first
non-suppressed site of that edge, with the edge's other sites attached as
`= note:` lines (same as `[[acyclic]]`'s other-edge labels, but here they
are other occurrences of the *same* edge).

An `// arch-lint: allow(independent) reason="..."` comment on the line, or
an `#[arch_lint::allow(independent, reason = "...")]` attribute on the `use`
item itself or on an enclosing `mod`/`fn`/`impl`/`trait`, removes that one
site before edges are computed — same mechanism, same reason requirement,
as `[[acyclic]]` (unlike `[[acyclic]]`, the missing-reason warning does not
depend on the site lying on a cycle: every in-scope edge is an offence).

### Allow-list dependencies

```toml
[[allow-dep]]
from = "domain"
to = ["shared"]          # the complete list of declared scopes `domain` may depend on
severity = "error"       # optional, default error
```

Project-wide, like `[[acyclic]]` and `[[independent]]`: after every file has
been analyzed, `[[allow-dep]]` (ALD011) looks at the dependency graph
between `[[scopes]]` and reports one violation per edge `from → X`
where `X` is a declared scope not in `to` — the inverse of
`[[deny-scope-dep]]`'s deny-list, for teams that would rather enumerate what
`from` *may* depend on than what it must not. A dependency on a path that
resolves to no declared scope (an external crate, an unscoped file) is
never a violation, the same as `[[deny-scope-dep]]`. `to = []` forbids
every declared-scope dependency from `from`. Several `[[allow-dep]]` blocks
may share the same `from`; each is checked independently against its own
`to`, so one block's allow-list never widens another's. Give each a `name`
when they do — otherwise they share the display name `allow-dep:<from>`,
and an `allow(allow-dep:<from>)` comment suppresses all of them at once.

```text
ALD011 allow-dep:domain at src/domain/b.rs:1:23
  error: Scope `domain` may only depend on [shared]: domain → infra
```

Each offending edge is reported at its first non-suppressed site, with the
edge's other non-suppressed sites attached as `= note:` lines, the same as
`[[independent]]`. An `// arch-lint: allow(allow-dep) reason="..."` comment
on the line, or an `#[arch_lint::allow(allow_dep, reason = "...")]`
attribute on the `use` item itself or on an enclosing
`mod`/`fn`/`impl`/`trait` (attributes take the `snake_case` spelling;
comments accept either), removes that one site before edges are computed
— same mechanism, same reason requirement, as `[[independent]]`: every
in-scope edge is an offence, so the missing-reason warning does not depend
on the site lying on a cycle.

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
| ALD009 | `[[acyclic]]` | Project-wide: forbids a cycle between scopes in the dependency graph |
| ALD010 | `[[independent]]` | Project-wide: forbids any dependency between a set of scopes, in either direction |
| ALD011 | `[[allow-dep]]` | Project-wide: forbids a scope from depending on anything outside an explicit allow-list |

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
# Extra type-name suffixes treated as error types, on top of the built-in "Error"
patterns = ["Failure"]
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

Unknown keys inside `[analyzer]`, inside any declarative rule table
(`[[scopes]]`, `[[restrict-use]]`, ..., `[[require-return]]`) and inside any
per-rule option table (`[rules.<name>]`) are a config error, so a typo such as
`denyy = [...]` or `allow_in_test = true` cannot silently disable a rule or be
ignored. Each rule declares its own option keys; `enabled`, `severity` and
`exclude_files` are accepted next to them for every rule.

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

### JSON Schema

`arch-lint schema` prints a JSON Schema (draft 2020-12) for the whole
configuration file — the top-level keys, `[analyzer]`, every built-in rule's
option keys with their documentation, and the declarative sections:

```bash
arch-lint schema > arch-lint.schema.json
arch-lint schema --out arch-lint.schema.json   # same, written directly
```

Point your editor at the generated file with the schema directive that
[taplo](https://taplo.tamasfe.dev/) and the
[Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml)
extension understand — the first line of `arch-lint.toml`:

```toml
#:schema ./arch-lint.schema.json

preset = "recommended"

[rules.no-unwrap-expect]
allow_in_tests = true
```

You then get completion for rule names and option keys, and a warning on a
misspelled key before the linter ever runs. `arch-lint list-rules --json`
prints the same per-rule option schemas on their own, for tooling that wants
the rule registry rather than the config file shape.

Tables under `rules` whose name is not a built-in rule are left open, so
configuration for custom rules still validates. Tree-sitter configuration
(`[[layers]]`, `[dependencies]`, `[[constraints]]`) is not described by this
schema; those keys are accepted because the top level of the document stays
open.

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
declarative rules from `arch-lint.toml` (ALD001-ALD011); see
[Declarative rules](#declarative-rules-ald001-ald011) below for the names a
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

### Declarative rules (ALD001-ALD011)

The declarative rules honor the same two directives, with the same semantics.
A declarative rule answers to three names, each in kebab-case or `snake_case`:

| Name | Example | Suppresses |
|------|---------|------------|
| Rule kind | `restrict-use` / `restrict_use` | every `[[restrict-use]]` rule |
| TOML `name` | `no-sqlx-in-domain` | only that one TOML rule |
| Rule code | `ALD001` | every rule reported under that code |

`allow(all)` suppresses everything, as it does for the built-in rules.

`[[acyclic]]` (ALD009), `[[independent]]` (ALD010) and `[[allow-dep]]`
(ALD011) are the three project-wide exceptions: an `allow` directive — a
comment on the line, an attribute on the `use` item, or an attribute on an
enclosing `mod`/`fn`/`impl`/`trait` — removes that one site from the scope
graph before cycles (or offending edges) are computed, rather than
suppressing a violation already found on that line (see
[Cycles between scopes](#cycles-between-scopes),
[Independent scopes](#independent-scopes) and
[Allow-list dependencies](#allow-list-dependencies)). Reporting an
`allow(acyclic)`, `allow(independent)` or `allow(allow-dep)` directive that
no longer has anything to suppress as unused is not implemented.

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

## CLI Usage

```bash
cargo install --locked arch-lint-cli

arch-lint init                            # Generate arch-lint.toml
arch-lint init --ts                       # Generate with tree-sitter layers
arch-lint check                           # Run all checks
arch-lint check --rules no-unwrap-expect  # Run specific rules
arch-lint check --format json             # JSON output for CI
arch-lint check --engine ts               # Force tree-sitter engine
arch-lint check --exit-zero               # Report, but never fail the step
arch-lint list-rules                      # Show available rules and their options
arch-lint list-rules --json               # Same, as JSON (option schemas included)
arch-lint schema                          # JSON Schema of arch-lint.toml
arch-lint schema --out arch-lint.schema.json
```

Logs go to stderr, never stdout; a successful run prints nothing there (a
source file that fails to parse still logs a WARN line, an invalid
`RUST_LOG` directive prints `ignoring ...`, and exit `2` prints the `Error:`
line). `--verbose` turns on `INFO`/`DEBUG` logs; a non-empty `RUST_LOG`
overrides `--verbose` — invalid directives are reported on stderr
(`ignoring ...`) and skipped, and if none remain the default applies.

### Cross-language (tree-sitter engine)

```bash
arch-lint init --ts
# Edit [[layers]] and [dependencies] in arch-lint.toml
arch-lint check              # auto-detects engine from [[layers]]
arch-lint check --engine ts  # explicit engine selection
```

See [docs/tree-sitter-engine.md](docs/tree-sitter-engine.md) for full documentation.

## CI Integration

### Exit status

| Code | Meaning |
|------|---------|
| `0` | Ran; no violation at or above `fail_on` (or `--exit-zero` was given) |
| `1` | Ran; violations at or above `fail_on` were found |
| `2` | Could not run: configuration error, unknown rule, unreadable path, I/O error |

A CI step can therefore fail on violations and still tell a misconfigured
linter (`2`) apart from a real finding (`1`). `--exit-zero` turns the step
into a report-only one.

`--format json` and `--format compact` on stdout are clean machine-readable
output with no log lines mixed in — tracing diagnostics go to stderr, so
there is no need to redirect it away (e.g. `2>/dev/null`) before piping
stdout to `jq` or writing it to a file.

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
      - run: cargo install --locked arch-lint-cli
      - run: arch-lint check   # exit 1 on violations, 2 if it could not run
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

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) has the
workflow (issues, branches, checks, pull requests) and
[PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md) says what may appear in a
public artifact.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
