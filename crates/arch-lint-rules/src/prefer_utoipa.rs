//! Example rule to prefer `utoipa` over other `OpenAPI` crates.
//!
//! This demonstrates how easy it is to create custom preferred crate rules.

use arch_lint_core::RequiredCrateRule;

/// Rule code for prefer-utoipa.
#[allow(dead_code)]
pub const CODE: &str = "PROJ001";

/// Rule name for prefer-utoipa.
#[allow(dead_code)]
pub const NAME: &str = "prefer-utoipa";

/// Creates a new prefer-utoipa rule.
///
/// # Example
///
/// This rule detects:
/// ```ignore
/// // BAD
/// paperclip::path!("/api/users");
/// okapi::openapi!();
///
/// // GOOD
/// utoipa::path!("/api/users");
/// utoipa::openapi!();
/// ```
#[allow(dead_code)]
#[must_use]
pub fn new_prefer_utoipa() -> RequiredCrateRule {
    RequiredCrateRule::new(CODE, NAME)
        .prefer("utoipa")
        .over(&["paperclip", "okapi", "rweb"])
        .detect_macro_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arch_lint_core::{FileContext, Rule};
    use std::path::Path;

    fn check_code(code: &str) -> Vec<arch_lint_core::Violation> {
        let rule = new_prefer_utoipa();
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
    fn test_detects_paperclip() {
        let violations = check_code(
            r#"
fn api() {
    paperclip::path!("/api/users");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("utoipa::path"));
    }

    #[test]
    fn test_detects_okapi() {
        let violations = check_code(
            r#"
fn api() {
    okapi::openapi!();
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("utoipa::openapi"));
    }

    #[test]
    fn test_allows_utoipa() {
        let violations = check_code(
            r#"
fn api() {
    utoipa::path!("/api/users");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_multiple_violations() {
        let violations = check_code(
            r#"
fn api() {
    paperclip::path!("/api/users");
    okapi::openapi!();
    rweb::get!("/api");
}
"#,
        );
        assert_eq!(violations.len(), 3);
    }
}
