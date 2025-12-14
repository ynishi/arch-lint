//! # arch-lint-core
//!
//! Core framework for architecture linting based on `syn` AST analysis.
//!
//! This crate provides the foundational traits and types for building
//! architecture linters. It includes:
//!
//! - [`Rule`] trait for per-file AST-based rules
//! - [`ProjectRule`] trait for project-wide structural rules
//! - [`Analyzer`] for orchestrating lint execution
//! - [`Violation`] for representing lint findings
//!
//! ## Example
//!
//! ```ignore
//! use arch_lint_core::{Analyzer, Rule, Severity};
//!
//! let analyzer = Analyzer::builder()
//!     .root("./src")
//!     .rule(MyRule::new())
//!     .build()?;
//!
//! let result = analyzer.analyze()?;
//! result.print_report();
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod analyzer;
mod config;
mod context;
mod required_crate;
mod rule;
mod types;

/// Utility modules for rule implementations.
pub mod utils;

pub use analyzer::{Analyzer, AnalyzerBuilder};
pub use config::Config;
pub use context::{FileContext, ProjectContext};
pub use required_crate::{DetectionPattern, RequiredCrateRule};
pub use rule::{ProjectRule, ProjectRuleBox, Rule, RuleBox};
pub use types::{Label, LintResult, Location, Replacement, Severity, Suggestion, Violation};
pub use utils::allowance::{AllowCheck, AllowState};
