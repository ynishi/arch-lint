//! # arch-lint
//!
//! AST-based architecture linter for Rust projects.
//!
//! This is the main facade crate that re-exports core functionality and macros.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! arch-lint = "0.1"
//! ```
//!
//! ## Suppression Attributes
//!
//! Use `#[arch_lint::allow(...)]` to suppress rules:
//!
//! ```rust,ignore
//! // Function-level
//! #[arch_lint::allow(no_unwrap_expect, reason = "Validated at startup")]
//! fn load_config() -> Config {
//!     CONFIG.get().unwrap().clone()
//! }
//!
//! // File-level (inner attribute at top of file)
//! #![arch_lint::allow(no_sync_io, reason = "CLI tool")]
//! ```
//!
//! ## Programmatic Usage
//!
//! ```rust,ignore
//! use arch_lint::Analyzer;
//!
//! let analyzer = Analyzer::builder()
//!     .root("./src")
//!     .build()?;
//!
//! let result = analyzer.analyze()?;
//! ```

#![forbid(unsafe_code)]

// Re-export core types and traits
pub use arch_lint_core::*;

// Re-export the allow macro for #[arch_lint::allow(...)]
pub use arch_lint_macros::allow;
