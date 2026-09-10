# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

### Changed

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

### Removed

- Unused dependencies (`tokio`, `walkdir`, `insta`, and per-crate `serde` / `miette` /
  `tracing` / `ignore` where unused)
- Dead, unexported modules `prefer_utoipa` and `require_tracing_v2` in `arch-lint-rules`

### Fixed

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

[Unreleased]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.5.0...HEAD
[0.5.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.3...arch-lint-v0.5.0
[0.4.3]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.2...arch-lint-v0.4.3
[0.4.2]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.1...arch-lint-v0.4.2
[0.4.1]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.0...arch-lint-v0.4.1
[0.4.0]: https://github.com/ynishi/arch-lint/compare/v0.2.0...arch-lint-v0.4.0
[0.3.0]: https://crates.io/crates/arch-lint/0.3.0
[0.2.0]: https://github.com/ynishi/arch-lint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ynishi/arch-lint/releases/tag/v0.1.0
