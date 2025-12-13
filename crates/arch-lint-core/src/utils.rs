//! Utility functions for rule implementations.

pub mod allowance;
pub mod attributes;
pub mod paths;

// Re-export commonly used utilities for rule implementations
#[doc(inline)]
pub use allowance::{check_allow_comment, AllowState};
#[doc(inline)]
pub use attributes::{has_allow_attr, has_cfg_test, has_test_attr};
#[doc(inline)]
pub use paths::path_to_string;
