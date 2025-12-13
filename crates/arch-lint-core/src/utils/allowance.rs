//! Comment-based allowance directives.
//!
//! Supports directives like:
//! ```text
//! // arch-lint: allow(no-unwrap-expect) reason="startup initialization"
//! ```

use std::collections::HashSet;

/// State of allowance for a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowState {
    /// Rule is not allowed (default).
    Denied,
    /// Rule is explicitly allowed.
    Allowed,
}

impl AllowState {
    /// Returns true if allowed.
    #[must_use]
    pub fn is_allowed(self) -> bool {
        self == Self::Allowed
    }
}

/// Result of checking for allow directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowCheck {
    /// Rule is not allowed.
    Denied,
    /// Rule is allowed with optional reason.
    Allowed {
        /// The reason provided (if any).
        reason: Option<String>,
    },
}

impl AllowCheck {
    /// Returns true if allowed.
    #[must_use]
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }

    /// Returns the reason if allowed.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Allowed { reason } => reason.as_deref(),
            Self::Denied => None,
        }
    }
}

/// Parsed allowance directive.
#[derive(Debug, Clone)]
pub struct AllowDirective {
    /// Rule names that are allowed.
    pub rules: HashSet<String>,
    /// Optional reason for the allowance.
    pub reason: Option<String>,
}

/// Checks source code for allowance comments (legacy API).
///
/// Looks for comments in the format:
/// ```text
/// // arch-lint: allow(rule1, rule2) reason="explanation"
/// ```
///
/// # Arguments
///
/// * `content` - Source code content
/// * `line` - Line number to check (1-indexed)
/// * `rule_name` - Name of the rule to check for
///
/// # Returns
///
/// `AllowState::Allowed` if an allowance directive is found for the rule.
#[must_use]
pub fn check_allow_comment(content: &str, line: usize, rule_name: &str) -> AllowState {
    match check_allow_with_reason(content, line, rule_name) {
        AllowCheck::Allowed { .. } => AllowState::Allowed,
        AllowCheck::Denied => AllowState::Denied,
    }
}

/// Checks source code for allowance comments with reason.
///
/// Looks for comments in the format:
/// ```text
/// // arch-lint: allow(rule1, rule2) reason="explanation"
/// ```
///
/// # Arguments
///
/// * `content` - Source code content
/// * `line` - Line number to check (1-indexed)
/// * `rule_name` - Name of the rule to check for
///
/// # Returns
///
/// `AllowCheck::Allowed` with optional reason if an allowance directive is found.
#[must_use]
pub fn check_allow_with_reason(content: &str, line: usize, rule_name: &str) -> AllowCheck {
    // Check the line itself and the line before
    let lines: Vec<&str> = content.lines().collect();

    for check_line in [line.saturating_sub(1), line] {
        if check_line == 0 || check_line > lines.len() {
            continue;
        }

        let line_content = lines[check_line - 1];
        if let Some(directive) = parse_allow_directive(line_content) {
            if directive.rules.contains(rule_name) || directive.rules.contains("all") {
                return AllowCheck::Allowed {
                    reason: directive.reason,
                };
            }
        }
    }

    AllowCheck::Denied
}

/// Parses an allowance directive from a comment line.
fn parse_allow_directive(line: &str) -> Option<AllowDirective> {
    let line = line.trim();

    // Check for // or /// comment
    let comment_content = if let Some(rest) = line.strip_prefix("///") {
        rest.trim()
    } else if let Some(rest) = line.strip_prefix("//") {
        rest.trim()
    } else {
        return None;
    };

    // Check for arch-lint: allow(...) directive
    let directive = comment_content.strip_prefix("arch-lint:")?.trim();
    let allow_content = directive.strip_prefix("allow(")?.trim();

    // Find closing paren
    let paren_end = allow_content.find(')')?;
    let rules_str = &allow_content[..paren_end];

    // Parse rules
    let rules: HashSet<String> = rules_str
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if rules.is_empty() {
        return None;
    }

    // Parse optional reason
    let rest = &allow_content[paren_end + 1..].trim();
    let reason = if let Some(reason_part) = rest.strip_prefix("reason=") {
        let reason_part = reason_part.trim();
        if reason_part.starts_with('"') && reason_part.len() > 1 {
            let end = reason_part[1..].find('"').map(|i| i + 1)?;
            Some(reason_part[1..end].to_string())
        } else {
            None
        }
    } else {
        None
    };

    Some(AllowDirective { rules, reason })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_allow_directive() {
        let directive = parse_allow_directive("// arch-lint: allow(no-unwrap-expect)");
        assert!(directive.is_some());
        let directive = directive.unwrap();
        assert!(directive.rules.contains("no-unwrap-expect"));
        assert!(directive.reason.is_none());
    }

    #[test]
    fn test_parse_allow_directive_with_reason() {
        let directive =
            parse_allow_directive("// arch-lint: allow(no-sync-io) reason=\"startup only\"");
        assert!(directive.is_some());
        let directive = directive.unwrap();
        assert!(directive.rules.contains("no-sync-io"));
        assert_eq!(directive.reason, Some("startup only".to_string()));
    }

    #[test]
    fn test_parse_multiple_rules() {
        let directive = parse_allow_directive("// arch-lint: allow(rule1, rule2, rule3)");
        assert!(directive.is_some());
        let directive = directive.unwrap();
        assert!(directive.rules.contains("rule1"));
        assert!(directive.rules.contains("rule2"));
        assert!(directive.rules.contains("rule3"));
    }

    #[test]
    fn test_check_allow_comment() {
        let content = r#"fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    value.unwrap();
}"#;

        assert_eq!(
            check_allow_comment(content, 3, "no-unwrap-expect"),
            AllowState::Allowed
        );
        assert_eq!(
            check_allow_comment(content, 3, "other-rule"),
            AllowState::Denied
        );
    }

    #[test]
    fn test_check_allow_with_reason() {
        let content = r#"fn foo() {
    // arch-lint: allow(no-unwrap-expect) reason="Guaranteed by loop invariant"
    value.unwrap();
}"#;

        let result = check_allow_with_reason(content, 3, "no-unwrap-expect");
        assert!(result.is_allowed());
        assert_eq!(result.reason(), Some("Guaranteed by loop invariant"));
    }

    #[test]
    fn test_check_allow_without_reason() {
        let content = r#"fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    value.unwrap();
}"#;

        let result = check_allow_with_reason(content, 3, "no-unwrap-expect");
        assert!(result.is_allowed());
        assert_eq!(result.reason(), None);
    }

    #[test]
    fn test_check_allow_denied() {
        let content = r#"fn foo() {
    value.unwrap();
}"#;

        let result = check_allow_with_reason(content, 2, "no-unwrap-expect");
        assert!(!result.is_allowed());
        assert_eq!(result.reason(), None);
    }
}
