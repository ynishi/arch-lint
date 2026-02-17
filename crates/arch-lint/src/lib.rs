//! # arch-lint
//!
//! AST-based architecture linter for Rust projects.
//!
//! This is the main facade crate that re-exports core functionality, macros, and rules.
//!
//! ## Quick Start — `cargo test` Integration
//!
//! ```toml
//! [dev-dependencies]
//! arch-lint = "0.4"
//! ```
//!
//! ```rust,ignore
//! // tests/architecture.rs
//! arch_lint::check!();
//! ```
//!
//! This runs arch-lint as part of `cargo test`. Configure via `arch-lint.toml`.
//!
//! ## Suppression Attributes
//!
//! Use `#[arch_lint::allow(...)]` to suppress rules:
//!
//! ```rust,ignore
//! #[arch_lint::allow(no_unwrap_expect, reason = "Validated at startup")]
//! fn load_config() -> Config {
//!     CONFIG.get().unwrap().clone()
//! }
//! ```
//!
//! ## Programmatic Usage
//!
//! ```rust,ignore
//! use arch_lint::Analyzer;
//! use arch_lint::rules::presets::Preset;
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

// Re-export the check macro for arch_lint::check!()
pub use arch_lint_macros::check;

/// Built-in rules and presets.
pub mod rules {
    pub use arch_lint_rules::*;
}

mod runner;

#[doc(hidden)]
pub mod __internal {
    pub use crate::runner::run_check;
}
