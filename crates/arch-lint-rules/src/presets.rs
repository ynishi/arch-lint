//! Rule presets for common configurations.

use crate::{HandlerComplexity, NoErrorSwallowing, NoSyncIo, NoUnwrapExpect, RequireThiserror};
use arch_lint_core::RuleBox;

/// Preset configurations for arch-lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Recommended rules with sensible defaults.
    Recommended,
    /// Strict rules for maximum safety.
    Strict,
    /// Minimal rules for gradual adoption.
    Minimal,
}

impl Preset {
    /// Returns the rules for this preset.
    #[must_use]
    pub fn rules(self) -> Vec<RuleBox> {
        match self {
            Self::Recommended => recommended_rules(),
            Self::Strict => strict_rules(),
            Self::Minimal => minimal_rules(),
        }
    }
}

/// Returns the recommended set of rules.
///
/// Includes:
/// - `no-unwrap-expect` (AL001) - Forbids `.unwrap()/.expect()`
/// - `no-sync-io` (AL002) - Forbids blocking I/O
/// - `no-error-swallowing` (AL003) - Forbids silent error handling
/// - `require-thiserror` (AL005) - Requires thiserror for error types
#[must_use]
pub fn recommended_rules() -> Vec<RuleBox> {
    vec![
        Box::new(NoUnwrapExpect::new()),
        Box::new(NoSyncIo::new()),
        Box::new(NoErrorSwallowing::new()),
        Box::new(RequireThiserror::new()),
    ]
}

/// Returns the strict set of rules.
///
/// Includes all recommended rules plus:
/// - Stricter `no-unwrap-expect` (no exceptions in tests)
/// - `handler-complexity` (AL004) - Limits handler complexity
#[must_use]
pub fn strict_rules() -> Vec<RuleBox> {
    vec![
        Box::new(
            NoUnwrapExpect::new()
                .allow_in_tests(false)
                .allow_expect(false),
        ),
        Box::new(NoSyncIo::new()),
        Box::new(NoErrorSwallowing::new()),
        Box::new(RequireThiserror::new()),
        Box::new(HandlerComplexity::new()),
    ]
}

/// Returns the minimal set of rules.
///
/// For gradual adoption, only includes:
/// - `no-unwrap-expect` (allowing `.expect()`)
#[must_use]
pub fn minimal_rules() -> Vec<RuleBox> {
    vec![Box::new(NoUnwrapExpect::new().allow_expect(true))]
}

/// Returns all available rules.
#[must_use]
pub fn all_rules() -> Vec<RuleBox> {
    vec![
        Box::new(NoUnwrapExpect::new()),
        Box::new(NoSyncIo::new()),
        Box::new(NoErrorSwallowing::new()),
        Box::new(HandlerComplexity::new()),
        Box::new(RequireThiserror::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_rules() {
        assert!(!Preset::Recommended.rules().is_empty());
        assert!(!Preset::Strict.rules().is_empty());
        assert!(!Preset::Minimal.rules().is_empty());
    }
}
