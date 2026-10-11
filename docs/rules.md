# Rule Reference

Built-in rules (AL001-AL013), declarative rules (ALD001-ALD011), and presets. [Back to README](../README.md).

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

Declarative rules defined in `arch-lint.toml` (see [Declarative Rules](declarative-rules.md)):

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

The message names the offending item only (`` Handler `handle_action` exceeds
the line limit `` / `` Match expression in `handle_action` exceeds the arm
limit `` — or `Match expression exceeds the arm limit` when the match is not
directly inside a free function whose name starts with `handle_`, `process_`,
`on_` or `update` (the rule's own handler-name patterns); a match inside an
`impl` method, a differently-named free function or a closure is unnamed the
same way / `` Enum `Action` exceeds the variant limit ``); the measured count
and the configured limit are not in the message, so the baseline fingerprint
(`code`, `file`, `message`) stays stable across an edit that only changes the
size. Unnamed matches in one file share a single baseline entry (`count =
N`), the same as any rule with a constant message. The count and limit are
reported instead in the suggestion (`= help:` line in `--format text`,
`suggestion.message` in `--format json`), e.g. `61 lines, limit 50 — Split
into smaller functions or extract match arms to separate handlers`.

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

