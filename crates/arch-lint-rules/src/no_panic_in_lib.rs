//! Rule to forbid panic macros in library code.
//!
//! # Rationale
//!
//! Library code should never panic. Instead, errors should be returned as
//! `Result` types so that calling code can handle them appropriately.
//! Panicking in libraries leads to poor user experience and crashes.
//!
//! # Detected Patterns
//!
//! - `panic!(...)`
//! - `todo!(...)`
//! - `unimplemented!(...)`
//! - `unreachable!(...)`
//!
//! # Good Patterns
//!
//! ```ignore
//! // Return Result instead of panicking
//! pub fn parse_config(input: &str) -> Result<Config, ParseError> {
//!     let value = input.parse()?;
//!     Ok(Config { value })
//! }
//! ```

use arch_lint_core::utils::allowance::check_allow_with_reason;
use arch_lint_core::utils::{check_arch_lint_allow, has_cfg_test, has_test_attr, path_to_string};
use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{ExprMacro, ItemFn, ItemImpl, ItemMod};

/// Rule code for no-panic-in-lib.
pub const CODE: &str = "AL011";

/// Rule name for no-panic-in-lib.
pub const NAME: &str = "no-panic-in-lib";

/// Forbids panic macros in library code.
#[derive(Debug, Clone)]
pub struct NoPanicInLib {
    /// Allow in test code.
    pub allow_in_tests: bool,
    /// Custom severity.
    pub severity: Severity,
}

impl Default for NoPanicInLib {
    fn default() -> Self {
        Self::new()
    }
}

impl NoPanicInLib {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            allow_in_tests: true,
            severity: Severity::Error,
        }
    }

    /// Sets whether to allow in test code.
    #[must_use]
    pub fn allow_in_tests(mut self, allow: bool) -> Self {
        self.allow_in_tests = allow;
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl Rule for NoPanicInLib {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Forbids panic macros in library code"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        // Skip test files if configured
        if self.allow_in_tests && ctx.is_test {
            return Vec::new();
        }

        let mut visitor = PanicVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            in_test_context: false,
            in_allowed_context: false,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct PanicVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a NoPanicInLib,
    violations: Vec<Violation>,
    in_test_context: bool,
    in_allowed_context: bool,
}

impl PanicVisitor<'_> {
    fn check_panic_macro(&mut self, path: &syn::Path) {
        // Skip if in test context and tests are allowed
        if self.rule.allow_in_tests && self.in_test_context {
            return;
        }

        // Skip if in allowed context
        if self.in_allowed_context {
            return;
        }

        let path_str = path_to_string(path);

        // Check if this is a panic macro (handle both simple and qualified paths)
        let panic_macro = if path_str == "panic" || path_str.ends_with("::panic") {
            Some(("panic!", "Return Result instead of panicking"))
        } else if path_str == "todo" || path_str.ends_with("::todo") {
            Some(("todo!", "Implement the functionality or return Result"))
        } else if path_str == "unimplemented" || path_str.ends_with("::unimplemented") {
            Some((
                "unimplemented!",
                "Implement the functionality or return Result",
            ))
        } else if path_str == "unreachable" || path_str.ends_with("::unreachable") {
            Some((
                "unreachable!",
                "Use Result or proper error handling instead",
            ))
        } else {
            None
        };

        if let Some((macro_name, suggestion_text)) = panic_macro {
            let Some(first_segment) = path.segments.first() else {
                return;
            };
            let span = first_segment.ident.span();
            let start = span.start();

            // Check for inline allow comment
            let allow_check = check_allow_with_reason(self.ctx.content, start.line, NAME);
            if allow_check.is_allowed() {
                // If reason is required but not provided, create a separate violation
                if self.rule.requires_allow_reason() && allow_check.reason().is_none() {
                    let location =
                        Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);
                    self.violations.push(
                        Violation::new(
                            CODE,
                            NAME,
                            Severity::Warning,
                            location,
                            format!("Allow directive for '{NAME}' is missing required reason"),
                        )
                        .with_suggestion(Suggestion::new(
                            "Add reason=\"...\" to explain why this exception is necessary",
                        )),
                    );
                }
                return;
            }

            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!("`{macro_name}` is forbidden in library code"),
                )
                .with_suggestion(Suggestion::new(suggestion_text)),
            );
        }
    }
}

impl<'ast> Visit<'ast> for PanicVisitor<'_> {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let was_in_test = self.in_test_context;
        let was_allowed = self.in_allowed_context;

        if has_cfg_test(&node.attrs) {
            self.in_test_context = true;
        }

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_mod(self, node);

        self.in_test_context = was_in_test;
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let was_in_test = self.in_test_context;
        let was_allowed = self.in_allowed_context;

        if has_test_attr(&node.attrs) {
            self.in_test_context = true;
        }

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_fn(self, node);

        self.in_test_context = was_in_test;
        self.in_allowed_context = was_allowed;
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let was_allowed = self.in_allowed_context;

        if check_arch_lint_allow(&node.attrs, NAME).is_allowed() {
            self.in_allowed_context = true;
        }

        syn::visit::visit_item_impl(self, node);

        self.in_allowed_context = was_allowed;
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.check_panic_macro(&node.path);
        syn::visit::visit_macro(self, node);
    }

    fn visit_expr_macro(&mut self, node: &'ast ExprMacro) {
        self.check_panic_macro(&node.mac.path);
        syn::visit::visit_expr_macro(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn check_code(code: &str) -> Vec<Violation> {
        let ast = syn::parse_file(code).expect("Failed to parse");
        let ctx = FileContext {
            path: Path::new("test.rs"),
            content: code,
            is_test: false,
            module_path: vec![],
            relative_path: std::path::PathBuf::from("test.rs"),
        };
        NoPanicInLib::new().check(&ctx, &ast)
    }

    #[test]
    fn test_detects_panic() {
        let violations = check_code(
            r#"
pub fn foo() {
    panic!("error");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, CODE);
        assert!(violations[0].message.contains("panic!"));
    }

    #[test]
    fn test_detects_todo() {
        let violations = check_code(
            r#"
pub fn foo() {
    todo!("implement this");
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("todo!"));
    }

    #[test]
    fn test_detects_unimplemented() {
        let violations = check_code(
            r#"
pub fn foo() {
    unimplemented!();
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unimplemented!"));
    }

    #[test]
    fn test_detects_unreachable() {
        let violations = check_code(
            r#"
pub fn foo() {
    unreachable!();
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("unreachable!"));
    }

    #[test]
    fn test_allows_in_test_fn() {
        let violations = check_code(
            r#"
#[test]
fn test_foo() {
    panic!("test panic");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_in_cfg_test_mod() {
        let violations = check_code(
            r#"
#[cfg(test)]
mod tests {
    fn helper() {
        panic!("test panic");
    }
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_attribute() {
        let violations = check_code(
            r#"
#[arch_lint::allow(no_panic_in_lib)]
pub fn foo() {
    panic!("allowed");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_allows_with_reason() {
        let violations = check_code(
            r#"
pub fn foo() {
    // arch-lint: allow(no-panic-in-lib) reason="Initialization code, guaranteed to succeed"
    panic!("critical error");
}
"#,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn test_detects_multiple_panic_macros() {
        let violations = check_code(
            r#"
pub fn foo() {
    panic!("error");
    todo!("implement");
    unimplemented!();
    unreachable!();
}
"#,
        );
        assert_eq!(violations.len(), 4);
    }
}
