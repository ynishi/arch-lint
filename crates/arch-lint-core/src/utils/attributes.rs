//! Attribute parsing utilities for rule implementations.

use super::allowance::{AllowCheck, AllowDirective};
use std::collections::HashSet;
use syn::{Attribute, Meta};

/// Checks if attributes contain an `#[allow(...)]` for specific lint names.
///
/// # Arguments
///
/// * `attrs` - Slice of attributes to check
/// * `lint_names` - Lint names to look for (e.g., `"clippy::unwrap_used"`)
///
/// # Returns
///
/// `true` if any of the specified lints are allowed.
#[must_use]
pub fn has_allow_attr(attrs: &[Attribute], lint_names: &[&str]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("allow") {
            continue;
        }

        // Convert the entire attribute to string for matching
        let attr_str = quote::quote!(#attr).to_string();
        // Normalize whitespace
        let attr_str = attr_str.replace(' ', "");

        for lint_name in lint_names {
            // Normalize the lint name too
            let normalized_name = lint_name.replace(' ', "");
            if attr_str.contains(&normalized_name) {
                return true;
            }
        }
    }

    false
}

/// Checks if attributes contain a `#[test]` attribute.
#[must_use]
pub fn has_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

/// Checks if attributes contain a `#[cfg(test)]` attribute.
#[must_use]
pub fn has_cfg_test(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("cfg") {
            continue;
        }

        // Convert to string and check for "test"
        let attr_str = quote::quote!(#attr).to_string();
        if attr_str.contains("test") {
            return true;
        }
    }

    false
}

/// Checks if attributes contain a specific custom attribute.
///
/// # Arguments
///
/// * `attrs` - Slice of attributes to check
/// * `name` - Attribute name to look for (without `#[]`)
#[must_use]
pub fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident(name))
}

/// Extracts the value from an attribute like `#[attr = "value"]`.
#[must_use]
pub fn get_attr_value(attrs: &[Attribute], name: &str) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident(name) {
            continue;
        }

        if let Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(lit) = &nv.value {
                if let syn::Lit::Str(s) = &lit.lit {
                    return Some(s.value());
                }
            }
        }
    }

    None
}

/// Checks if attributes contain `#[arch_lint::allow(...)]` for a specific rule.
///
/// Recognizes both:
/// - `#[arch_lint::allow(rule_name, reason = "...")]`
/// - `#[arch_lint_macros::allow(rule_name, reason = "...")]`
///
/// # Arguments
///
/// * `attrs` - Slice of attributes to check
/// * `rule_name` - Rule name to look for (e.g., `"no_unwrap_expect"` or `"no-unwrap-expect"`)
///
/// # Returns
///
/// `AllowCheck::Allowed` with optional reason if the rule is allowed.
#[must_use]
pub fn check_arch_lint_allow(attrs: &[Attribute], rule_name: &str) -> AllowCheck {
    for attr in attrs {
        if let Some(directive) = parse_arch_lint_allow_attr(attr) {
            // Normalize rule names (support both kebab-case and snake_case)
            let normalized_rule = rule_name.replace('-', "_");
            let has_rule = directive.rules.iter().any(|r| {
                let normalized_r = r.replace('-', "_");
                normalized_r == normalized_rule || r == "all"
            });

            if has_rule {
                return AllowCheck::Allowed {
                    reason: directive.reason,
                };
            }
        }
    }

    AllowCheck::Denied
}

/// Checks if any attribute is an `#[arch_lint::allow(...)]`.
fn is_arch_lint_allow_path(attr: &Attribute) -> bool {
    let path = attr.path();
    let segments: Vec<_> = path.segments.iter().collect();

    match segments.as_slice() {
        // #[arch_lint::allow(...)]
        [first, second] => {
            (first.ident == "arch_lint" || first.ident == "arch_lint_macros")
                && second.ident == "allow"
        }
        // #[allow(...)] after `use arch_lint::allow;` - can't distinguish, skip
        _ => false,
    }
}

/// Parses `#[arch_lint::allow(rule1, rule2, reason = "...")]` attribute.
fn parse_arch_lint_allow_attr(attr: &Attribute) -> Option<AllowDirective> {
    if !is_arch_lint_allow_path(attr) {
        return None;
    }

    // Parse the attribute arguments
    let Meta::List(list) = &attr.meta else {
        return None;
    };

    // Convert tokens to string for parsing
    let tokens_str = list.tokens.to_string();
    parse_allow_attr_tokens(&tokens_str)
}

/// Parses the tokens inside `allow(...)`.
///
/// Expected formats:
/// - `rule1, rule2`
/// - `rule1, reason = "explanation"`
/// - `rule1, rule2, reason = "explanation"`
fn parse_allow_attr_tokens(tokens: &str) -> Option<AllowDirective> {
    let mut rules = HashSet::new();
    let mut reason = None;

    // Split by comma, but be careful with reason="..." containing commas
    let mut remaining = tokens.trim();

    while !remaining.is_empty() {
        remaining = remaining.trim_start_matches(',').trim();
        if remaining.is_empty() {
            break;
        }

        // Check for reason = "..."
        if remaining.starts_with("reason") {
            if let Some(rest) = remaining.strip_prefix("reason") {
                let rest = rest.trim();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim();
                    if let Some(rest) = rest.strip_prefix('"') {
                        if let Some(end) = rest.find('"') {
                            reason = Some(rest[..end].to_string());
                            remaining = rest[end + 1..].trim();
                            continue;
                        }
                    }
                }
            }
        }

        // Otherwise, it's a rule name
        let end = remaining
            .find(|c: char| c == ',' || c.is_whitespace())
            .unwrap_or(remaining.len());
        let rule = remaining[..end].trim();
        if !rule.is_empty() && rule != "reason" {
            rules.insert(rule.to_string());
        }
        remaining = &remaining[end..];
    }

    if rules.is_empty() {
        return None;
    }

    Some(AllowDirective { rules, reason })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_has_allow_attr() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[allow(clippy::unwrap_used)])];
        assert!(has_allow_attr(&attrs, &["clippy::unwrap_used"]));
        assert!(!has_allow_attr(&attrs, &["clippy::expect_used"]));
    }

    #[test]
    fn test_has_test_attr() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[test])];
        assert!(has_test_attr(&attrs));

        let attrs: Vec<Attribute> = vec![parse_quote!(#[inline])];
        assert!(!has_test_attr(&attrs));
    }

    #[test]
    fn test_has_cfg_test() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[cfg(test)])];
        assert!(has_cfg_test(&attrs));

        let attrs: Vec<Attribute> = vec![parse_quote!(#[cfg(feature = "foo")])];
        assert!(!has_cfg_test(&attrs));
    }

    #[test]
    fn test_check_arch_lint_allow_simple() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect)])];
        let result = check_arch_lint_allow(&attrs, "no_unwrap_expect");
        assert!(result.is_allowed());
        assert_eq!(result.reason(), None);
    }

    #[test]
    fn test_check_arch_lint_allow_with_reason() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect, reason = "validated input")])];
        let result = check_arch_lint_allow(&attrs, "no_unwrap_expect");
        assert!(result.is_allowed());
        assert_eq!(result.reason(), Some("validated input"));
    }

    #[test]
    fn test_check_arch_lint_allow_multiple_rules() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect, no_sync_io)])];
        assert!(check_arch_lint_allow(&attrs, "no_unwrap_expect").is_allowed());
        assert!(check_arch_lint_allow(&attrs, "no_sync_io").is_allowed());
        assert!(!check_arch_lint_allow(&attrs, "other_rule").is_allowed());
    }

    #[test]
    fn test_check_arch_lint_allow_kebab_case_normalization() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect)])];
        // Both kebab-case and snake_case should match
        assert!(check_arch_lint_allow(&attrs, "no-unwrap-expect").is_allowed());
        assert!(check_arch_lint_allow(&attrs, "no_unwrap_expect").is_allowed());
    }

    #[test]
    fn test_check_arch_lint_allow_not_found() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_sync_io)])];
        let result = check_arch_lint_allow(&attrs, "no_unwrap_expect");
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_check_arch_lint_allow_wrong_attr() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[allow(unused)])];
        let result = check_arch_lint_allow(&attrs, "no_unwrap_expect");
        assert!(!result.is_allowed());
    }

    #[test]
    fn test_parse_allow_attr_tokens() {
        let directive = parse_allow_attr_tokens("no_unwrap_expect").unwrap();
        assert!(directive.rules.contains("no_unwrap_expect"));
        assert!(directive.reason.is_none());

        let directive =
            parse_allow_attr_tokens("no_unwrap_expect, reason = \"test reason\"").unwrap();
        assert!(directive.rules.contains("no_unwrap_expect"));
        assert_eq!(directive.reason, Some("test reason".to_string()));

        let directive = parse_allow_attr_tokens("rule1, rule2, reason = \"multi\"").unwrap();
        assert!(directive.rules.contains("rule1"));
        assert!(directive.rules.contains("rule2"));
        assert_eq!(directive.reason, Some("multi".to_string()));
    }
}
