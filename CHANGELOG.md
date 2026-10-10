# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`arch-lint check --exit-zero`** reports violations and exits `0`, for
  report-only CI steps
- **`ScopeGraph`**: the dependency graph between declared `[[scopes]]`
  (nodes = scope names, edges = scope-crossing `use`/inline references with
  the `Location` of each site), collected once per `Analyzer::analyze()` by a
  visitor that runs inside the existing per-file pass (no second parse). Path
  resolution is the one ALD003 uses (see below); unlike ALD003 the graph
  records every reference, regardless of `allow` suppressions and of
  `check_inline` — rules built on the graph decide what to filter. Reachable
  through `ProjectContext::scope_graph()` (for a `ProjectRule`), the standalone
  `Analyzer::scope_graph()` (builds the graph without running any rule) and
  `Session::scope_graph()` (the SDK driver). Enabled by the new
  `AnalyzerBuilder::declarative(Arc<DeclarativeConfig>)` builder method,
  paired with the new `declarative::load_config_from_toml` (parses TOML into
  a `DeclarativeConfig` without building rules) and
  `declarative::create_rules_shared` (builds rules from a shared `Arc`, so the
  same config can also be handed to the analyzer). Prerequisite for cycle
  detection, independence and allow-list rules, and `arch-lint graph`
  (separate issues)
- **`deny-scope-dep` (ALD003), `layering` (ALD004) and the `ScopeGraph`** now
  resolve `self::` and `super::` paths relative to the file's module
  (inline `mod` blocks included), not just `crate::`; bare paths and `pub use`
  re-exports remain unresolved

### Changed

- **ALD001 `restrict-use` inline checks now also see `self::` / `super::`
  paths**, matched verbatim against the `deny` / `except` patterns, as `use`
  items already were
- **`ProjectContext` is now `#[non_exhaustive]`** and has a new `scope_graph`
  field; construct it with `ProjectContext::new` plus the `with_*` builders,
  not a struct literal
- **Exit status `2` when the CLI could not run.** Configuration errors,
  unknown `--rules` names, unreadable paths and I/O errors now exit `2`;
  `1` is reserved for "ran, and found violations at or above `fail_on`".
  Previously both cases exited `1`. The table is in `arch-lint check --help`
  and the README "CI Integration" section

### Documentation

- `CONTRIBUTING.md` rewritten around the workflow: label table with branch
  prefixes, issue template (`.github/ISSUE_TEMPLATE/issue.md`: Problem /
  Evidence / Proposal / Acceptance), pull request template with an acceptance
  table, verification through the installed binary, commit subject prefixes.
  New `PUBLIC_DEVELOPMENT.md` states the public-by-default policy and what
  must not appear in a public artifact


### Fixed

- **CI no longer depends on the newest dependency versions.** The workspace
  uses `resolver = "3"` (MSRV-aware, Rust 1.84+) and `Cargo.lock` is now
  committed, so the MSRV (1.88) job checks a fixed, MSRV-buildable tree with
  `--locked` instead of resolving transitive dependencies that have raised
  their `rust-version` (`textwrap`, `tree-sitter-language`). A non-blocking
  `latest deps` job runs `cargo update` on stable so upstream breakage stays
  visible. `cargo install --locked arch-lint-cli` is the documented install
  line
- clippy: `.ok().is_some_and(..)` replaced by `.is_ok_and(..)` in
  `async-trait-send-check`, which newer stable clippy rejects under
  `-D warnings`

## [0.6.0] - 2026-09-11

### Added

- **`arch_lint::Session` - the SDK driver for the syn engine.** The
  "root + config → `LintResult`" pipeline that `check!()` and the CLI each
  reimplemented is now one public, non-panicking API.
  `Session::discover(root)` resolves the config file
  (`<root>/arch-lint.toml` → `<root>/.arch-lint.toml` → `$ARCH_LINT_CONFIG_DIR`
  or `~/.arch-lint/` `config.toml` → defaults) and records the winner in
  `ConfigSource`, reachable via `session.config_source()`.
  `Session::from_config_path(root, path)` and
  `Session::from_config_str(root, toml)` cover explicit and in-memory
  configuration, and `Session::from_source(root, source)` takes an
  already-resolved `ConfigSource`. Overrides are builder-style —
  `with_preset`, `with_fail_on`, `with_rules`, `with_rule_filter` (names or
  codes; unknown names are `SessionError::UnknownRule`) and `with_exclude` —
  alongside the `root()` / `config()` / `config_source()` / `preset()` /
  `fail_on()` accessors. `run()` returns `Result<LintResult, SessionError>`;
  `SessionError` is a `thiserror` enum that preserves the underlying config,
  declarative-rule and analyzer errors as sources.
  `Session::find_workspace_root(path)` exposes the `[workspace]` walk that
  `check!()` uses
- **`arch_lint::report::render`** - renders a `LintResult` as text, JSON or
  compact output, the same three formats the CLI emits. ANSI colours are
  opt-in through `Format::Text { color }` and default to off, so library
  callers and captured output stay plain; the JSON is the same
  `serde_json::to_string_pretty` document as `--format json`
- **`arch_lint_ts::TsSession`** - the same driver shape for the tree-sitter
  engine. `TsSession::new(config, root)` starts with the Kotlin extractor
  registered, `.with_extractor(..)` adds languages, `.respect_gitignore(bool)`
  overrides the config, and `.run()` validates the config, discovers files,
  runs the engine and returns a sorted `LintResult` or a `TsSessionError`
- **`[analyzer] respect_gitignore` for the tree-sitter engine** (default
  `true`), matching the syn analyzer. File discovery uses the same ignore
  sources with `require_git(false)`, so the rules apply outside a git
  repository too
- **`arch_lint_core::AnalyzerError` is re-exported** from the crate root, so
  callers can match on analyzer failures rather than only print them
- **`[analyzer] respect_gitignore` and `include` are now honored** - file
  discovery walks the root with the `ignore` crate instead of a bare
  `**/*.rs` glob. With the default `respect_gitignore = true`, gitignored
  files are skipped (`.gitignore` at any level, `.git/info/exclude`, global
  gitignore); ignore rules apply even outside a git repository, so scaffolded
  projects and fixture directories are filtered the same way. A non-empty
  `include` restricts analysis to matching files, and `Cargo.toml` discovery
  for project-wide rules follows the same gitignore and `exclude` settings.
  `include` / `exclude` patterns now match the path **relative to the root**
  as well as the absolute path, so `exclude = ["src/generated/**"]` works as
  written. New `AnalyzerBuilder::respect_gitignore(bool)` and
  `AnalyzerBuilder::includes(..)` methods; an explicit builder call overrides
  the configured `respect_gitignore`. `[analyzer] parallelism` remains parsed
  but unused
- **Suppression for declarative rules** - ALD001-ALD008 now honor the same
  directives as the built-in rules: `#[arch_lint::allow(<name>, reason = "...")]`
  on the offending item or an enclosing `mod` / `fn` / `impl`, and
  `// arch-lint: allow(<name>) reason="..."` on the violating line or the line
  above it. `<name>` may be the rule kind (`restrict-use`), the TOML rule name
  (`no-sqlx-in-domain`) or the code (`ALD001`), in kebab-case or `snake_case`;
  a TOML name suppresses only that rule. Error-severity rules require a reason
  on the comment path, reporting the same "missing required reason" warning as
  the built-in rules. Declarative rules keep firing inside `#[cfg(test)]`
- **ALD004 `[[layering]]`** - declare an ordered stack of scopes
  (`order = ["handler", "domain", "infra"]`); a layer may only depend on layers
  below it, and `adjacent_only = true` also forbids skipping a layer. Replaces
  the hand-written `[[deny-scope-dep]]` matrix
- **ALD005 `[[deny-attr]]` / ALD006 `[[require-attr]]`** - forbid or require
  attributes on items of a scope (`attrs = ["derive(sqlx::FromRow)"]`,
  `attrs = ["tracing::instrument"]`), narrowed by `on = ["struct", "fn", ...]`
  and, for `require-attr`, by `visibility`. `deny-attr` defaults to `error`,
  `require-attr` to `warning`
- **ALD007 `[[naming]]`** - require identifiers of one item kind to match a
  regex (`item = "trait"`, `pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"`);
  defaults to `warning`
- **ALD008 `[[require-return]]`** - require functions to return a type matching
  a regex, with optional `async` and `visibility` filters; the return type is
  normalized (`Result < T , E >` → `Result<T, E>`, no return type → `()`)
  before matching
- **`except` on `[[restrict-use]]`** - a path matching it is not reported even
  when it matches `deny`
- **`exclude` on `[[scopes]]`** - a file matching it is not part of the scope
  even when it matches `paths`; applies to every rule kind using that scope

- **Per-rule options are now honored** - `[rules.<name>]` keys documented in the README
  (`allow_in_tests`, `allow_expect`, `allow_patterns`, `max_handler_lines`, `max_match_arms`,
  `max_enum_variants`, `patterns`, `runtime_mode`, `require_*_docs`, `allow_ok`,
  `allow_let_underscore`) are applied when building preset rules, both in `check!()` and the CLI.
  Previously they were parsed and ignored
- **`exclude_files`** per-rule option - skip a rule for files matching the given globs
  (relative to the analyzer root)
- **CLI `check` parity with `check!()`** - reads `preset` and `fail_on` from `arch-lint.toml`,
  loads declarative rules (`[[restrict-use]]`, `[[require-use]]`, `[[deny-scope-dep]]`),
  and exits non-zero on violations at or above `fail_on`
- Rule registry (`arch_lint_rules::rule_by_name` / `rule_from_config` / `canonical_rule_name`)
  and `FromStr` for `Preset` / `Severity`
- CI workflow (rustfmt, clippy, per-crate tests, MSRV, rustdoc, cargo-deny), `deny.toml`,
  `CONTRIBUTING.md`
- **`arch-lint schema [--out <path>]`** - prints a JSON Schema (draft 2020-12) of
  `arch-lint.toml`: the top-level keys, `[analyzer]`, every built-in rule's
  `[rules.<name>]` table (common `enabled` / `severity` / `exclude_files` plus the
  rule's own option keys, each with its documentation and preset-dependent
  default) and the declarative sections. Point taplo / Even Better TOML at the
  generated file with `#:schema ./arch-lint.schema.json` for completion and
  typo warnings. Built as `arch_lint::schema::config_json_schema()` for SDK use
- **Typed per-rule options** - each built-in rule now has an options struct
  (`arch_lint_rules::options::*Options`, `JsonSchema`-derived), exposed through
  `rule_option_schemas()` / `rule_option_schema(name_or_code)` and
  `validate_rule_tables(&Config)`. `arch-lint list-rules` prints every rule's
  option keys with type and default; `list-rules --json` prints the registry
  with each rule's option schema
- `JsonSchema` derives on `Config`, `AnalyzerConfig`, `RuleConfig`, `Severity`
  and the declarative config DTOs (new `schemars` dependency)

### Changed

- **CLI colours are emitted only when stdout is a terminal.** `--format text`
  severity labels and the summary line, and the `tracing` log lines, are plain
  when the output is piped or redirected. Terminal output is unchanged
- **The CLI is now a wrapper over the facade.** `arch-lint-cli` depends on
  `arch-lint` and delegates to `Session` and `report::render`;
  `src/config_resolver.rs` and `src/commands/output.rs` moved into
  `arch_lint::session` and `arch_lint::report`, and `commands/check_ts.rs`
  delegates to `arch_lint_ts::TsSession`. Behaviour, exit codes and output are
  unchanged apart from the colour policy above; error messages may now carry
  additional `SessionError` context
- **The tree-sitter `exclude` patterns are matched as globs** against the
  root-relative path (`src/test/**` now works as written). The previous
  substring behaviour is kept as a fallback, so existing configs such as
  `**/generated/**` keep matching
- **`strict` preset now includes every rule** (AL009-AL012 were missing); `list-rules` and
  `--rules` now accept AL009-AL012 as well
- `--rules` with an unknown name is an error instead of a silent warning
- Engine auto-detection parses the config as TOML; a commented-out `[[layers]]` no longer
  switches to the tree-sitter engine
- `LanguageExtractor::analyze` returns `Result<FileAnalysis, ExtractError>` instead of
  panicking when the grammar fails to load
- `rust-version` raised to 1.88 (required by current `ignore` / `home` releases)
- `arch-lint init` template: fixed docs URL and `max_handler_lines` key, added `preset` /
  `fail_on` / `exclude_files` examples
- Makefile `publish` targets include `arch-lint-ts` and the `arch-lint` facade;
  `bump-version` works with GNU sed
- **Built-in rules share their visitor scaffolding** - the `#[arch_lint::allow(...)]` /
  `#[cfg(test)]` / `#[test]` suppression state that nine rules each tracked by hand now lives
  in `arch_lint_core::utils::scope::ScopeTracker`, and their `visit_item_mod` /
  `visit_item_fn` / `visit_item_impl` overrides delegate to it. New
  `arch_lint_core::test_utils` (behind the `test-utils` feature) replaces the `FileContext`
  literal that was copied into every rule's test module. No change to rule behavior, codes,
  messages, severities, or public rule APIs
- **`Preset::rules_with_config` and `rule_from_config` are fallible** - they return
  `Result<_, RuleOptionsError>` so that an invalid `[rules.<name>]` table surfaces
  as an error; `Session` maps it to the new `SessionError::RuleOptions`. An
  unrecognised `runtime_mode` value for `async-trait-send-check` is now an error
  instead of silently falling back to the default

### Removed

- Unused dependencies (`tokio`, `walkdir`, `insta`, and per-crate `serde` / `miette` /
  `tracing` / `ignore` where unused)
- Dead, unexported modules `prefer_utoipa` and `require_tracing_v2` in `arch-lint-rules`

### Fixed

- Unknown keys in `[analyzer]`, in every declarative rule table (`[[scopes]]`,
  `[[restrict-use]]`, ..., `[[require-return]]`) and in every built-in
  `[rules.<name>]` table are now a config error instead of being silently
  ignored, so a typo like `denyy = [...]` or `allow_in_test = true` no longer
  disables a rule. `[rules.<name>]` tables are validated even when the rule is
  not part of the active preset; tables for custom rules stay free-form
- AL002 `no-sync-io` no longer reports `is_file()` / `is_dir()` on `FileType` /
  `Metadata` values (results of `file_type()`, `metadata()`, `symlink_metadata()`,
  `fs::metadata(..)`, also through `?` / `unwrap()`, `let` bindings and closure
  parameters); only the path-receiver form performs I/O
- Documentation drift: README rule table lists AL013 and the declarative ALD001-ALD003 codes;
  ARCHITECTURE.md reflects the facade / macros / declarative rules; broken `CONTRIBUTING.md` /
  `LICENSE` links; facade crate doc pointed at `arch-lint = "0.4"`

## [0.5.0] - 2026-08-22

### Added

- **Inline qualified-path checking** for declarative rules (ALD001 `restrict-use`, ALD003 `deny-scope-dep`)
  - References like `sqlx::query(...)` or `crate::infra::Db::new()` written without a `use` statement are now detected, in expression, type, trait-bound, and macro positions
  - New per-rule option `check-inline` (default: `true`); set `check-inline = false` to restore the previous `use`-only behavior
  - `self::` / `super::` relative paths and single-segment local names remain out of scope (unchanged v1 limitation)

## [0.4.3] - 2026-02-20

### Fixed

- AL007 `tracing-env-init`: false positive on `EnvFilter::try_from_default_env()` fallback patterns

## [0.4.2] - 2026-02-20

### Added

- **AL013: no-silent-result-drop** - Forbids silently discarding `Result` error information
  (`let _ = result`, `.ok()` without handling). Included in the `recommended` preset;
  `strict` also forbids `.ok()` and test exceptions

## [0.4.1] - 2026-02-18

### Changed

- README rewritten around the day-1 adoption flow (`check!()` + `arch-lint.toml`) and
  the declarative rules showcase

## [0.4.0] - 2026-02-17

### Added

- **`arch_lint::check!()` macro** - runs arch-lint as a `#[test]` inside `cargo test`,
  with `preset` / `config` / `fail_on` arguments and workspace-root detection
- **Declarative architecture rules** in `arch-lint.toml`:
  - `[[scopes]]` - named path sets
  - ALD001 `[[restrict-use]]` - forbid `use` paths within a scope
  - ALD002 `[[require-use]]` - require a preferred crate over alternatives
  - ALD003 `[[deny-scope-dep]]` - forbid one scope depending on another
- `arch-lint` facade crate re-exporting core, rules, and macros
- **Global config fallback** - `ConfigResolver` falls back to `~/.arch-lint/arch-lint.toml`
  (or `$ARCH_LINT_CONFIG_DIR`) when no project config is found

### Fixed

- `ScopeDep.name`, `mod.rs` fallback for module paths, overlapping scope handling
- Workspace detection uses TOML parsing instead of string matching

## [0.3.0] - 2026-02-05

### Added

- **Tree-sitter engine** (`arch-lint-ts` crate) - Cross-language architecture layer enforcement
  - `LanguageExtractor` trait for pluggable language support
  - `KotlinExtractor` for Kotlin `.kt`/`.kts` files (package, imports, declarations)
  - `LayerResolver` with longest-prefix-match package-to-layer mapping
  - `ArchRuleEngine` with three rule types:
    - LAYER001 (layer dependency) - forbidden cross-layer imports
    - PATTERN001 (import pattern) - forbidden import path patterns per layer
    - NAMING001 (naming rule) - naming convention-based access control
  - `ArchConfig` TOML parser with `[[layers]]`, `[dependencies]`, `[[constraints]]`
  - Config validation (unknown layers, self-dependency, missing entries)
  - 30 unit tests

- **CLI dual engine support**
  - `--engine ts|syn` flag for explicit engine selection
  - Auto-detection from config: `[[layers]]` present triggers tree-sitter engine
  - `arch-lint init --ts` generates tree-sitter config template
  - `arch-lint check` dispatches to correct engine automatically

- **Documentation**
  - `docs/tree-sitter-engine.md` - Comprehensive tree-sitter engine guide

## [0.2.0] - 2025-12-14

### Added

- **AL009: async-trait-send-check** - Checks proper usage of `async_trait` Send bounds
  - Detects `#[async_trait]` without `?Send` in single-threaded contexts
  - Suggests using `#[async_trait(?Send)]` for single-threaded environments
  - Configurable runtime mode (single-thread/multi-thread)

- **AL010: prefer-from-over-into** - Prefers `From` trait implementation over `Into`
  - Detects `impl Into<T> for U` patterns
  - Recommends implementing `From` instead (automatically provides `Into`)
  - Follows Rust conventions and best practices

- **AL011: no-panic-in-lib** - Forbids panic macros in library code
  - Detects `panic!`, `todo!`, `unimplemented!`, `unreachable!` in library code
  - Allows panic macros in test code (configurable)
  - Suggests proper error handling with `Result` types
  - Default severity: Error

- **AL012: require-doc-comments** - Requires documentation comments on public items
  - Requires `///` documentation on public functions, structs, and enums
  - Configurable per item type (functions/structs/enums)
  - Improves API documentation quality

- **Issue Templates**
  - Added GitHub Issue template for rule proposals
  - Added GitHub Issue template for bug reports
  - Added configuration for community discussions

### Changed

- Bumped version to 0.2.0
- Improved code quality with clippy fixes

## [0.1.0] - 2025-12-14

### Added

- Initial release of arch-lint
- Core architecture linting framework
- **AL001: no-unwrap-expect** - Forbids `.unwrap()` and `.expect()` in production code
- **AL002: no-sync-io** - Forbids blocking I/O in async contexts
- **AL003: no-error-swallowing** - Forbids catching errors without propagation
- **AL004: handler-complexity** - Limits complexity of handler functions
- **AL005: require-thiserror** - Requires `thiserror` derive for error types
- **AL006: require-tracing** - Requires `tracing` crate instead of `log` crate
- **AL007: tracing-env-init** - Prevents hardcoded log levels in tracing initialization
- CLI tool with configuration support
- Allow directives via attributes and inline comments
- Comprehensive test coverage

[Unreleased]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.6.0...HEAD
[0.6.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.5.0...arch-lint-v0.6.0
[0.5.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.3...arch-lint-v0.5.0
[0.4.3]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.2...arch-lint-v0.4.3
[0.4.2]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.1...arch-lint-v0.4.2
[0.4.1]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.0...arch-lint-v0.4.1
[0.4.0]: https://github.com/ynishi/arch-lint/compare/v0.2.0...arch-lint-v0.4.0
[0.3.0]: https://crates.io/crates/arch-lint/0.3.0
[0.2.0]: https://github.com/ynishi/arch-lint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ynishi/arch-lint/releases/tag/v0.1.0
