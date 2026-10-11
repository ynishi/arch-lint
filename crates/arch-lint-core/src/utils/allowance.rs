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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    for directive in directives_for_line(content, line) {
        if directive.rules.contains(rule_name) || directive.rules.contains("all") {
            return AllowCheck::Allowed {
                reason: directive.reason,
            };
        }
    }

    AllowCheck::Denied
}

/// Every allow directive applicable to `line` (1-indexed): the one on the
/// comment-only line above it (if any), then the one on `line` itself (if
/// any), in that order.
///
/// `pub(crate)`: the one shared lookup both [`check_allow_with_reason`]
/// (above) and
/// [`ScopeEdgeCollector`](crate::declarative::rules::ScopeEdgeCollector)'s
/// `comment_directives_at` use, so the two cannot drift apart.
///
/// The line *above* `line` only counts when it has nothing on it but the
/// directive — `line.trim_start().starts_with("//")` — never when the
/// directive trails code on that line. A trailing directive suppresses
/// *its own* line; if the line above were allowed to supply a directive in
/// trailing form too, a single `// arch-lint: allow(...)` written after
/// code on line N would also suppress a violation on line N+1, which is
/// not what either form is for. `line` itself has no such restriction: it
/// is always the target of its own directive, written as a leading
/// comment or a trailing one.
pub(crate) fn directives_for_line(content: &str, line: usize) -> Vec<AllowDirective> {
    let lines: Vec<&str> = content.lines().collect();
    let mut directives = Vec::new();

    let above = line.saturating_sub(1);
    if above >= 1 && above <= lines.len() {
        let text = lines[above - 1];
        if text.trim_start().starts_with("//") {
            if let Some(directive) = parse_allow_directive(text) {
                directives.push(directive);
            }
        }
    }

    if line >= 1 && line <= lines.len() {
        if let Some(directive) = parse_allow_directive(lines[line - 1]) {
            directives.push(directive);
        }
    }

    directives
}

/// Finds the byte offset of the first `//` in `line` that starts a line
/// comment rather than appearing inside a `"..."` string literal or a
/// `'.'` char literal.
///
/// String literals are tracked with `\"` escapes so a `//` inside one
/// (e.g. `"http://x"`) is skipped. Char literals are recognized
/// conservatively: `'` followed by one char and a closing `'` (`'"'`), or
/// by a backslash escape and a closing `'` (`'\n'`). A `'` that is not
/// followed by either shape — a lifetime like `'a` or `'static` — is left
/// alone rather than treated as opening a literal, so the characters after
/// it (quotes included) are scanned normally.
fn find_comment_start(line: &str) -> Option<usize> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_pos, c) = chars[i];
        match c {
            '"' => {
                i += 1;
                while i < chars.len() {
                    let (_, next) = chars[i];
                    i += 1;
                    if next == '\\' {
                        i += 1; // skip the escaped character
                    } else if next == '"' {
                        break;
                    }
                }
            }
            '\'' if i + 3 < chars.len() && chars[i + 1].1 == '\\' && chars[i + 3].1 == '\'' => {
                i += 4; // '\x' escape, e.g. '\n', '\\', '\''
            }
            '\'' if i + 2 < chars.len() && chars[i + 2].1 == '\'' => {
                i += 3; // 'c' single character, e.g. '"'
            }
            '/' if i + 1 < chars.len() && chars[i + 1].1 == '/' => {
                return Some(byte_pos);
            }
            _ => i += 1,
        }
    }
    None
}

/// Parses an allowance directive from a comment line.
///
/// The directive may open a leading-comment line (`// arch-lint: ...`) or
/// follow code on the same line (`use x; // arch-lint: ...`): this looks
/// for the first `//` that is not inside a `"..."` string literal or a
/// `'.'` char literal (see [`find_comment_start`]) and parses the
/// directive from there, so both forms reach the same logic below.
///
/// Raw strings (`r"..."`, `r#"..."#`) and block comments (`/* ... */`) are
/// out of scope: a `//` inside either of those can be misread as the start
/// of a line comment. This is rare in practice (an allowance comment next
/// to a raw string or a block comment on the same line) and is accepted as
/// a known limitation rather than handled.
///
/// Parses one line in isolation — it does not decide whether that line is
/// the right one to consult for a given violation's line number; that
/// policy (comment-only above, either form on the line itself) lives in
/// [`directives_for_line`], the only caller, in this module and out of it.
fn parse_allow_directive(line: &str) -> Option<AllowDirective> {
    let comment = &line[find_comment_start(line)?..];

    // Check for // or /// comment
    let comment_content = comment
        .strip_prefix("///")
        .or_else(|| comment.strip_prefix("//"))?
        .trim();

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
        let content = r"fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    value.unwrap();
}";

        assert_eq!(
            check_allow_comment(content, 3, "no-unwrap-expect"),
            AllowState::Allowed
        );
        assert_eq!(
            check_allow_comment(content, 3, "other-rule"),
            AllowState::Denied
        );
    }

    /// A trailing directive is scoped to its own line: a directive on
    /// line N must not also suppress line N+1 — only a comment-only line
    /// above the target line supplies a directive to it; a trailing one
    /// never does.
    #[test]
    fn trailing_directive_on_one_line_does_not_suppress_the_next_line() {
        let content = r#"fn foo() {
    a().unwrap(); // arch-lint: allow(no-unwrap-expect) reason="x"
    b().unwrap();
}"#;

        // Line 2 (the trailing directive's own line) is suppressed.
        assert!(check_allow_with_reason(content, 2, "no-unwrap-expect").is_allowed());
        // Line 3 is not: the line above it is a trailing form, not a
        // comment-only line, so it supplies nothing to line 3.
        assert!(!check_allow_with_reason(content, 3, "no-unwrap-expect").is_allowed());
    }

    /// A comment-only line above the target still works as before: it is
    /// the leading form's whole reason for existing.
    #[test]
    fn comment_only_line_above_still_suppresses_the_next_line() {
        let content = r#"fn foo() {
    // arch-lint: allow(no-unwrap-expect) reason="x"
    a().unwrap();
}"#;
        assert!(check_allow_with_reason(content, 3, "no-unwrap-expect").is_allowed());
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
        let content = r"fn foo() {
    // arch-lint: allow(no-unwrap-expect)
    value.unwrap();
}";

        let result = check_allow_with_reason(content, 3, "no-unwrap-expect");
        assert!(result.is_allowed());
        assert_eq!(result.reason(), None);
    }

    #[test]
    fn test_check_allow_denied() {
        let content = r"fn foo() {
    value.unwrap();
}";

        let result = check_allow_with_reason(content, 2, "no-unwrap-expect");
        assert!(!result.is_allowed());
        assert_eq!(result.reason(), None);
    }

    // ── `parse_allow_directive` / `find_comment_start` ──

    #[test]
    fn leading_comment_is_parsed() {
        let directive = parse_allow_directive("// arch-lint: allow(no-unwrap-expect)");
        assert!(directive.is_some());
    }

    #[test]
    fn trailing_comment_after_code_is_parsed() {
        let directive = parse_allow_directive(
            "use crate::infra::db::Pool; // arch-lint: allow(deny-scope-dep) reason=\"x\"",
        );
        let directive = directive.expect("trailing form should parse");
        assert!(directive.rules.contains("deny-scope-dep"));
        assert_eq!(directive.reason, Some("x".to_string()));
    }

    #[test]
    fn double_slash_inside_a_string_literal_does_not_open_a_comment() {
        // No line comment at all: the only `//` is inside the string, so
        // there is nothing to parse.
        assert!(parse_allow_directive(r#"let s = "http://x";"#).is_none());

        // The `//` inside the string is skipped, so the real trailing
        // comment after it is still found and parsed.
        let directive = parse_allow_directive(
            r#"let s = "http://x"; // arch-lint: allow(no-unwrap-expect) reason="y""#,
        );
        let directive = directive.expect("the real trailing comment should still parse");
        assert_eq!(directive.reason, Some("y".to_string()));
    }

    #[test]
    fn char_literal_containing_a_quote_does_not_open_a_string() {
        // `'"'` must not be mistaken for the start of a `"..."` string
        // literal, which would otherwise swallow the rest of the line.
        let directive = parse_allow_directive(
            "let c = '\"'; // arch-lint: allow(no-unwrap-expect) reason=\"z\"",
        );
        let directive =
            directive.expect("the trailing comment after the char literal should parse");
        assert_eq!(directive.reason, Some("z".to_string()));
    }

    #[test]
    fn lifetime_before_the_comment_does_not_open_a_char_literal() {
        // `'a` is a lifetime, not a char literal opening: the characters
        // after it must still be scanned normally.
        let directive = parse_allow_directive(
            "fn f<'a>(x: &'a str) {} // arch-lint: allow(no-unwrap-expect) reason=\"w\"",
        );
        let directive = directive.expect("the trailing comment after the lifetimes should parse");
        assert_eq!(directive.reason, Some("w".to_string()));
    }

    #[test]
    fn doc_comment_form_is_still_parsed() {
        let directive =
            parse_allow_directive("/// arch-lint: allow(no-unwrap-expect) reason=\"doc\"");
        let directive = directive.expect("`///` should still parse");
        assert_eq!(directive.reason, Some("doc".to_string()));
    }
}
