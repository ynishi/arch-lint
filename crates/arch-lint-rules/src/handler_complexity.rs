//! Rule to limit complexity of handler functions.
//!
//! # Rationale
//!
//! Handler functions (especially in TEA/Elm architecture) tend to grow large
//! with many match arms. This rule enforces limits to encourage decomposition
//! into smaller, focused functions.
//!
//! # Detected Patterns
//!
//! - Functions named `handle_*`, `process_*`, `on_*` with too many lines
//! - Match expressions with too many arms
//! - Action/Message enums with too many variants
//!
//! # Configuration
//!
//! - `max_handler_lines`: Maximum lines in handler body (default: 150)
//! - `max_match_arms`: Maximum arms in a match expression (default: 20)
//! - `max_enum_variants`: Maximum variants in Action enum (default: 30)

use arch_lint_core::{FileContext, Location, Rule, Severity, Suggestion, Violation};
use syn::visit::Visit;
use syn::{Expr, ExprMatch, ItemEnum, ItemFn};

/// Rule code for handler-complexity.
pub const CODE: &str = "AL004";

/// Rule name for handler-complexity.
pub const NAME: &str = "handler-complexity";

/// Handler function name patterns.
const HANDLER_PATTERNS: &[&str] = &["handle_", "process_", "on_", "update"];

/// Action enum name patterns.
const ACTION_PATTERNS: &[&str] = &["Action", "Message", "Msg", "Event", "Command", "Cmd"];

/// Configuration for handler complexity limits.
#[derive(Debug, Clone)]
pub struct HandlerComplexityConfig {
    /// Maximum lines in a handler function body.
    pub max_handler_lines: usize,
    /// Maximum arms in a match expression.
    pub max_match_arms: usize,
    /// Maximum variants in an Action enum.
    pub max_enum_variants: usize,
}

impl Default for HandlerComplexityConfig {
    fn default() -> Self {
        Self {
            max_handler_lines: 150,
            max_match_arms: 20,
            max_enum_variants: 30,
        }
    }
}

/// Limits complexity of handler functions.
#[derive(Debug, Clone)]
pub struct HandlerComplexity {
    config: HandlerComplexityConfig,
    severity: Severity,
}

impl Default for HandlerComplexity {
    fn default() -> Self {
        Self::new()
    }
}

impl HandlerComplexity {
    /// Creates a new rule with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: HandlerComplexityConfig::default(),
            severity: Severity::Warning,
        }
    }

    /// Sets maximum handler lines.
    #[must_use]
    pub fn max_handler_lines(mut self, max: usize) -> Self {
        self.config.max_handler_lines = max;
        self
    }

    /// Sets maximum match arms.
    #[must_use]
    pub fn max_match_arms(mut self, max: usize) -> Self {
        self.config.max_match_arms = max;
        self
    }

    /// Sets maximum enum variants.
    #[must_use]
    pub fn max_enum_variants(mut self, max: usize) -> Self {
        self.config.max_enum_variants = max;
        self
    }

    /// Sets the severity level.
    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

impl Rule for HandlerComplexity {
    fn name(&self) -> &'static str {
        NAME
    }

    fn code(&self) -> &'static str {
        CODE
    }

    fn description(&self) -> &'static str {
        "Limits complexity of handler functions"
    }

    fn default_severity(&self) -> Severity {
        self.severity
    }

    fn check(&self, ctx: &FileContext, ast: &syn::File) -> Vec<Violation> {
        let mut visitor = ComplexityVisitor {
            ctx,
            rule: self,
            violations: Vec::new(),
            current_fn: None,
        };

        visitor.visit_file(ast);
        visitor.violations
    }
}

struct ComplexityVisitor<'a> {
    ctx: &'a FileContext<'a>,
    rule: &'a HandlerComplexity,
    violations: Vec<Violation>,
    current_fn: Option<String>,
}

impl<'ast> Visit<'ast> for ComplexityVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let fn_name = node.sig.ident.to_string();

        // Check if this is a handler function
        if is_handler_function(&fn_name) {
            self.current_fn = Some(fn_name.clone());

            // Check line count
            let line_count = count_block_lines(&node.block);
            if line_count > self.rule.config.max_handler_lines {
                let span = node.sig.ident.span();
                let start = span.start();
                let location =
                    Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

                self.violations.push(
                    Violation::new(
                        CODE,
                        NAME,
                        self.rule.severity,
                        location,
                        format!(
                            "Handler `{}` has {} lines (max: {})",
                            fn_name, line_count, self.rule.config.max_handler_lines
                        ),
                    )
                    .with_suggestion(Suggestion::new(
                        "Split into smaller functions or extract match arms to separate handlers",
                    )),
                );
            }
        }

        syn::visit::visit_item_fn(self, node);
        self.current_fn = None;
    }

    fn visit_expr_match(&mut self, node: &'ast ExprMatch) {
        let arm_count = node.arms.len();

        if arm_count > self.rule.config.max_match_arms {
            let span = node.match_token.span;
            let start = span.start();
            let location =
                Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

            let context = self
                .current_fn
                .as_ref()
                .map(|f| format!(" in `{f}`"))
                .unwrap_or_default();

            self.violations.push(
                Violation::new(
                    CODE,
                    NAME,
                    self.rule.severity,
                    location,
                    format!(
                        "Match expression{} has {} arms (max: {})",
                        context, arm_count, self.rule.config.max_match_arms
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "Group related arms or split into separate match expressions",
                )),
            );
        }

        syn::visit::visit_expr_match(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        let enum_name = node.ident.to_string();

        // Check if this is an Action/Message enum
        if is_action_enum(&enum_name) {
            let variant_count = node.variants.len();

            if variant_count > self.rule.config.max_enum_variants {
                let span = node.ident.span();
                let start = span.start();
                let location =
                    Location::new(self.ctx.relative_path.clone(), start.line, start.column + 1);

                self.violations.push(
                    Violation::new(
                        CODE,
                        NAME,
                        self.rule.severity,
                        location,
                        format!(
                            "Enum `{}` has {} variants (max: {})",
                            enum_name, variant_count, self.rule.config.max_enum_variants
                        ),
                    )
                    .with_suggestion(Suggestion::new(
                        "Split into nested enums (e.g., Action::User(UserAction))",
                    )),
                );
            }
        }

        syn::visit::visit_item_enum(self, node);
    }
}

/// Checks if a function name matches handler patterns.
fn is_handler_function(name: &str) -> bool {
    HANDLER_PATTERNS
        .iter()
        .any(|pattern| name.starts_with(pattern))
}

/// Checks if an enum name matches action patterns.
fn is_action_enum(name: &str) -> bool {
    ACTION_PATTERNS.iter().any(|pattern| name.contains(pattern))
}

/// Counts lines in a block using span information.
fn count_block_lines(block: &syn::Block) -> usize {
    if block.stmts.is_empty() {
        return 0;
    }

    let first_span = block.stmts.first().map(stmt_span);
    let last_span = block.stmts.last().map(stmt_span);

    match (first_span, last_span) {
        (Some(first), Some(last)) => {
            let start = first.start().line;
            let end = last.end().line;
            end.saturating_sub(start) + 1
        }
        _ => 0,
    }
}

/// Gets the span of a statement.
fn stmt_span(stmt: &syn::Stmt) -> proc_macro2::Span {
    match stmt {
        syn::Stmt::Local(local) => local.let_token.span,
        syn::Stmt::Item(item) => item_span(item),
        syn::Stmt::Expr(expr, _) => expr_span(expr),
        syn::Stmt::Macro(m) => m
            .mac
            .path
            .segments
            .first()
            .map_or_else(proc_macro2::Span::call_site, |s| s.ident.span()),
    }
}

fn item_span(item: &syn::Item) -> proc_macro2::Span {
    match item {
        syn::Item::Fn(f) => f.sig.fn_token.span,
        syn::Item::Struct(s) => s.ident.span(),
        syn::Item::Enum(e) => e.ident.span(),
        _ => proc_macro2::Span::call_site(),
    }
}

fn expr_span(expr: &Expr) -> proc_macro2::Span {
    match expr {
        Expr::Match(m) => m.match_token.span,
        Expr::If(i) => i.if_token.span,
        Expr::Call(c) => expr_span(&c.func),
        Expr::MethodCall(m) => m.method.span(),
        Expr::Path(p) => p
            .path
            .segments
            .first()
            .map_or_else(proc_macro2::Span::call_site, |s| s.ident.span()),
        _ => proc_macro2::Span::call_site(),
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
        HandlerComplexity::new()
            .max_match_arms(3)
            .max_enum_variants(3)
            .check(&ctx, &ast)
    }

    #[test]
    fn test_detects_large_match() {
        let violations = check_code(
            r#"
fn handle_action(action: Action) {
    match action {
        Action::A => {},
        Action::B => {},
        Action::C => {},
        Action::D => {},
        Action::E => {},
    }
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("5 arms"));
    }

    #[test]
    fn test_detects_large_enum() {
        let violations = check_code(
            r#"
enum UserAction {
    Create,
    Update,
    Delete,
    List,
    Get,
}
"#,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("5 variants"));
    }

    #[test]
    fn test_allows_small_match() {
        let violations = check_code(
            r#"
fn handle_action(action: Action) {
    match action {
        Action::A => {},
        Action::B => {},
    }
}
"#,
        );
        assert!(violations.is_empty());
    }
}
