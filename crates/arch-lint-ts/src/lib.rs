//! # arch-lint-ts
//!
//! Tree-sitter based cross-language architecture linter.
//!
//! This crate extends arch-lint with Tree-sitter powered analysis for
//! non-Rust languages (Kotlin, and more to come). It reuses
//! `arch-lint-core` types (`Violation`, `Severity`, `Location`) and adds:
//!
//! - [`LanguageExtractor`] trait for pluggable language support
//! - [`KotlinExtractor`] for Kotlin import/class extraction
//! - [`LayerResolver`] for package-to-layer mapping
//! - [`ArchRuleEngine`] for layer dependency and pattern constraint checks
//! - [`ArchConfig`] for TOML-based layer/dependency/constraint definitions
//! - [`TsSession`] as the end-to-end driver (discover files, analyze, collect)
//!
//! ```no_run
//! use arch_lint_ts::{ArchConfig, TsSession};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ArchConfig::from_file(Path::new("arch-lint.toml"))?;
//! let result = TsSession::new(config, ".").run()?;
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod engine;
pub mod extractor;
pub mod kotlin;
pub mod layer;
pub mod session;

pub use config::ArchConfig;
pub use engine::ArchRuleEngine;
pub use extractor::{ExtractError, FileAnalysis, LanguageExtractor};
pub use kotlin::KotlinExtractor;
pub use layer::LayerResolver;
pub use session::{TsSession, TsSessionError};
