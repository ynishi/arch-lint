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
//! arch-lint = "0.7"
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
//! [`Session`] is the SDK entry point: it performs the same
//! "root + config → [`LintResult`]" pipeline that `check!()` and the CLI use,
//! without panicking or exiting the process.
//!
//! ```no_run
//! use arch_lint::report::{render, Format};
//! use arch_lint::Session;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let session = Session::discover(".")?;
//! let fail_on = session.fail_on();
//!
//! let result = session.run()?;
//! print!("{}", render(&result, Format::default())?);
//!
//! if result.has_violations_at(fail_on) {
//!     // decide what a failure means for your tool
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Tree-sitter based analysis of non-Rust languages has its own driver,
//! `arch_lint_ts::TsSession`.

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

pub mod baseline;
pub mod graph;
pub mod report;
pub mod schema;
pub mod session;

pub use session::{ConfigSource, Session, SessionError};

mod runner;

#[doc(hidden)]
pub mod __internal {
    pub use crate::runner::{run_check, run_check_at, run_check_with_baseline};
}
