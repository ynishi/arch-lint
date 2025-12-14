//! # arch-lint-rules
//!
//! Built-in lint rules for arch-lint.
//!
//! This crate provides a collection of general-purpose architecture lint rules
//! that can be used across different Rust projects.
//!
//! ## Available Rules
//!
//! | Code | Name | Description |
//! |------|------|-------------|
//! | AL001 | `no-unwrap-expect` | Forbids `.unwrap()` and `.expect()` in production code |
//! | AL002 | `no-sync-io` | Forbids blocking I/O in async contexts |
//! | AL003 | `no-error-swallowing` | Forbids catching errors without propagation |
//! | AL004 | `handler-complexity` | Limits complexity of handler functions |
//! | AL005 | `require-thiserror` | Requires `thiserror` derive for error types |
//! | AL006 | `require-tracing` | Requires `tracing` crate instead of `log` crate |
//! | AL007 | `tracing-env-init` | Prevents hardcoded log levels in tracing initialization |
//! | AL009 | `async-trait-send-check` | Checks proper usage of `async_trait` Send bounds |
//! | AL010 | `prefer-from-over-into` | Prefers `From` trait implementation over `Into` |
//!
//! ## Usage
//!
//! ```ignore
//! use arch_lint_core::Analyzer;
//! use arch_lint_rules::{NoUnwrapExpect, NoSyncIo};
//!
//! let analyzer = Analyzer::builder()
//!     .root("./src")
//!     .rule(NoUnwrapExpect::new())
//!     .rule(NoSyncIo::new())
//!     .build()?;
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod async_trait_send_check;
mod handler_complexity;
mod no_error_swallowing;
mod no_sync_io;
mod no_unwrap_expect;
mod prefer_from_over_into;
mod prefer_utoipa;
mod presets;
mod require_thiserror;
mod require_tracing;
mod require_tracing_v2;
mod tracing_env_init;

pub use async_trait_send_check::{AsyncTraitSendCheck, RuntimeMode};
pub use handler_complexity::{HandlerComplexity, HandlerComplexityConfig};
pub use no_error_swallowing::NoErrorSwallowing;
pub use no_sync_io::NoSyncIo;
pub use no_unwrap_expect::NoUnwrapExpect;
pub use prefer_from_over_into::PreferFromOverInto;
pub use presets::{all_rules, recommended_rules, strict_rules, Preset};
pub use require_thiserror::RequireThiserror;
pub use require_tracing::RequireTracing;
pub use tracing_env_init::TracingEnvInit;

/// Re-export core types for convenience.
pub use arch_lint_core::{Rule, Severity, Violation};
