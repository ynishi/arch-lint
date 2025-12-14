//! Sample project demonstrating arch-lint suppression patterns.
//!
//! This crate contains intentional violations to test arch-lint detection.

pub mod startup;
pub mod tracing_test;

// ============================================================================
// CASE 1: Violations WITHOUT suppression (should be reported)
// ============================================================================

/// This function contains unwrap that should be flagged.
pub fn unhandled_unwrap() -> i32 {
    let value: Option<i32> = Some(42);
    value.unwrap() // AL001: Should be reported
}

/// This function contains expect that should be flagged.
pub fn unhandled_expect() -> String {
    let text = "hello";
    text.parse::<String>().expect("parse failed") // AL001: Should be reported
}

// ============================================================================
// CASE 2: Line-level suppression with COMMENT
// ============================================================================

/// This function uses comment-based suppression.
pub fn comment_suppressed_unwrap() -> i32 {
    let value: Option<i32> = Some(42);
    // arch-lint: allow(no-unwrap-expect) reason="Value is guaranteed by constant"
    value.unwrap() // Should NOT be reported
}

// ============================================================================
// CASE 3: Block-level suppression with ATTRIBUTE
// ============================================================================

/// This function uses attribute-based suppression.
#[arch_lint::allow(no_unwrap_expect, reason = "Startup configuration validated externally")]
pub fn attribute_suppressed_unwrap() -> i32 {
    let value: Option<i32> = Some(42);
    let _first = value.unwrap(); // Should NOT be reported

    // Multiple unwraps in same function are all covered
    let another: Option<i32> = Some(100);
    another.unwrap() // Should NOT be reported
}

// ============================================================================
// CASE 4: Block-level suppression on impl block
// ============================================================================

pub struct Config {
    value: Option<String>,
}

#[arch_lint::allow(no_unwrap_expect, reason = "Config is validated at construction time")]
impl Config {
    pub fn get_value(&self) -> &str {
        self.value.as_ref().unwrap() // Should NOT be reported
    }

    pub fn get_or_default(&self) -> String {
        self.value.clone().unwrap_or_default()
    }
}

// ============================================================================
// CASE 5: Suppression WITHOUT reason (should warn for error-severity rules)
// ============================================================================

/// This function uses suppression without reason - should generate warning.
#[arch_lint::allow(no_unwrap_expect)]
pub fn suppressed_without_reason() -> i32 {
    let value: Option<i32> = Some(42);
    value.unwrap() // Suppressed but warning about missing reason
}

// ============================================================================
// CASE 6: Module-level suppression
// ============================================================================

#[arch_lint::allow(no_unwrap_expect, reason = "Legacy module pending refactor")]
mod legacy {
    pub fn old_code() -> i32 {
        let x: Option<i32> = Some(1);
        x.unwrap() // Should NOT be reported
    }

    pub fn more_old_code() -> String {
        "test".parse().unwrap() // Should NOT be reported
    }
}

// ============================================================================
// Tests (should be allowed by default with allow_in_tests = true)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unwrap_in_test() {
        let value: Option<i32> = Some(42);
        // unwrap in tests should be allowed by default
        assert_eq!(value.unwrap(), 42);
    }
}
