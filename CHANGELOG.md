# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/ynishi/arch-lint/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ynishi/arch-lint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ynishi/arch-lint/releases/tag/v0.1.0
