//! Rule traits for defining lint rules.

use crate::context::{FileContext, ProjectContext};
use crate::types::{Severity, Violation};

/// A per-file lint rule based on `syn` AST analysis.
///
/// Implement this trait to create rules that analyze individual source files.
/// Rules receive the parsed AST and can use the visitor pattern to traverse it.
///
/// # Example
///
/// ```ignore
/// use arch_lint_core::{Rule, FileContext, Violation, Severity};
/// use syn::visit::Visit;
///
/// pub struct NoTodoComments;
///
/// impl Rule for NoTodoComments {
///     fn name(&self) -> &'static str { "no-todo-comments" }
///     fn code(&self) -> &'static str { "AL008" }
///
///     fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
///         let mut visitor = TodoVisitor::new(ctx);
///         visitor.visit_file(ast);
///         visitor.violations
///     }
/// }
/// ```
pub trait Rule: Send + Sync {
    /// Returns the kebab-case name of this rule (e.g., "no-unwrap-expect").
    fn name(&self) -> &'static str;

    /// Returns the rule code (e.g., "AL001").
    fn code(&self) -> &'static str;

    /// Returns a brief description of what this rule checks.
    fn description(&self) -> &'static str {
        ""
    }

    /// Returns the default severity for violations from this rule.
    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    /// Whether this rule requires a reason when using allow directives.
    ///
    /// By default, rules with `Severity::Error` require a reason.
    /// Override this to customize the requirement.
    fn requires_allow_reason(&self) -> bool {
        self.default_severity() == Severity::Error
    }

    /// Checks a single file and returns any violations found.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Context about the file being checked
    /// * `ast` - The parsed syntax tree of the file
    ///
    /// # Returns
    ///
    /// A vector of violations found in this file.
    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation>;
}

/// Type alias for boxed Rule trait objects.
pub type RuleBox = Box<dyn Rule>;

/// A project-wide lint rule based on file structure analysis.
///
/// Implement this trait to create rules that analyze the project structure
/// rather than individual file contents. Useful for enforcing conventions
/// like "every module must have a test file".
///
/// # Example
///
/// ```ignore
/// use arch_lint_core::{ProjectRule, ProjectContext, Violation, Severity};
///
/// pub struct RequireReadme;
///
/// impl ProjectRule for RequireReadme {
///     fn name(&self) -> &'static str { "require-readme" }
///     fn code(&self) -> &'static str { "AL100" }
///
///     fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation> {
///         if !ctx.root.join("README.md").exists() {
///             vec![Violation::new(
///                 self.code(),
///                 self.name(),
///                 Severity::Warning,
///                 Location::new(ctx.root.to_path_buf(), 0, 0),
///                 "Project should have a README.md",
///             )]
///         } else {
///             vec![]
///         }
///     }
/// }
/// ```
pub trait ProjectRule: Send + Sync {
    /// Returns the kebab-case name of this rule.
    fn name(&self) -> &'static str;

    /// Returns the rule code (e.g., "AL100").
    fn code(&self) -> &'static str;

    /// Returns a brief description of what this rule checks.
    fn description(&self) -> &'static str {
        ""
    }

    /// Returns the default severity for violations from this rule.
    fn default_severity(&self) -> Severity {
        Severity::Error
    }

    /// Whether this rule requires a reason when using allow directives.
    ///
    /// By default, rules with `Severity::Error` require a reason.
    /// Override this to customize the requirement.
    fn requires_allow_reason(&self) -> bool {
        self.default_severity() == Severity::Error
    }

    /// Checks the project structure and returns any violations found.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Context about the project being checked
    ///
    /// # Returns
    ///
    /// A vector of violations found in the project.
    fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation>;
}

/// Type alias for boxed `ProjectRule` trait objects.
pub type ProjectRuleBox = Box<dyn ProjectRule>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Location;

    struct TestRule;

    impl Rule for TestRule {
        fn name(&self) -> &'static str {
            "test-rule"
        }
        fn code(&self) -> &'static str {
            "TEST001"
        }
        fn description(&self) -> &'static str {
            "A test rule"
        }

        fn check(&self, ctx: &FileContext, _ast: &syn::File) -> Vec<Violation> {
            vec![Violation::new(
                self.code(),
                self.name(),
                self.default_severity(),
                Location::new(ctx.path.to_path_buf(), 1, 1),
                "Test violation",
            )]
        }
    }

    #[test]
    fn test_rule_trait() {
        let rule = TestRule;
        assert_eq!(rule.name(), "test-rule");
        assert_eq!(rule.code(), "TEST001");
        assert_eq!(rule.default_severity(), Severity::Error);
    }
}
