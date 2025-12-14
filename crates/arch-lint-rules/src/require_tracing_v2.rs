//! Rule to require `tracing` crate instead of `log` crate (using RequiredCrateRule).
//!
//! This is a simplified version using the RequiredCrateRule builder.

use arch_lint_core::RequiredCrateRule;

/// Rule code for require-tracing.
pub const CODE: &str = "AL006";

/// Rule name for require-tracing.
pub const NAME: &str = "require-tracing";

/// Creates a new require-tracing rule using RequiredCrateRule.
#[must_use]
pub fn new_require_tracing() -> RequiredCrateRule {
    RequiredCrateRule::new(CODE, NAME)
        .prefer("tracing")
        .over(&["log"])
        .detect_macro_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arch_lint_core::{FileContext, Rule};
    use std::path::Path;

    fn check_code(code: &str) -> Vec<arch_lint_core::Violation> {
        let rule = new_require_tracing();
        let ast = syn::parse_file(code).expect("Failed to parse");
        let ctx = FileContext {
            path: Path::new("test.rs"),
            content: code,
            is_test: false,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("test.rs"),
        };
        rule.check(&ctx, &ast)
    }

    #[test]
    fn test_detects_log_info() {
        let violations = check_code(
            r#"
fn foo() {
    log::info!("message");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("tracing::info"));
    }

    #[test]
    fn test_allows_tracing() {
        let violations = check_code(
            r#"
fn foo() {
    tracing::info!("message");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_log_macros() {
        let violations = check_code(
            r#"
fn foo() {
    log::debug!("debug");
    log::info!("info");
    log::warn!("warn");
}
"#,
        );
        assert_eq!(violations.len(), 3);
    }
}
