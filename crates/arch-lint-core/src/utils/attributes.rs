//! Attribute parsing utilities for rule implementations.

use super::allowance::{AllowCheck, AllowDirective};
use proc_macro2::{TokenStream, TokenTree};
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
///
/// `pub(crate)`: reused outside this module by
/// [`ScopeEdgeCollector`](crate::declarative::rules::ScopeEdgeCollector),
/// which records every directive on an item (not just one named rule).
pub(crate) fn parse_arch_lint_allow_attr(attr: &Attribute) -> Option<AllowDirective> {
    if !is_arch_lint_allow_path(attr) {
        return None;
    }

    // Parse the attribute arguments
    let Meta::List(list) = &attr.meta else {
        return None;
    };

    parse_allow_attr_tokens(list.tokens.clone())
}

/// Parses the token stream inside `allow(...)`.
///
/// Expected arguments, comma-separated:
/// - a rule name: one or more `Ident`/`Punct('-')`/`Punct(':')`/digit-leading
///   `Literal` tokens concatenated in order (see [`parse_rule_name_arg`]),
///   e.g. `deny_scope_dep`, `deny-scope-dep`, `ALD003`, `allow-dep:domain`
///   or `allow-dep:core-2`
/// - `reason = "explanation"`
///
/// Walks the tokens structurally (`list.tokens.into_iter()`) rather than
/// rendering them with `to_string()` first: a kebab-case name like
/// `deny-scope-dep` is five tokens (`deny`, `-`, `scope`, `-`, `dep`), and
/// `to_string()` renders it `deny - scope - dep`, which splitting on
/// whitespace turns into five separate "rule names" that never match
/// anything. Rule-name normalization (kebab-case vs. `snake_case`) happens
/// at the comparison/storage sites ([`check_arch_lint_allow`] and
/// `EdgeSite::with_allow_directive`), not here: this function hands back
/// whatever spelling the source used.
///
/// A bare `reason` with no `=` (so [`parse_reason_arg`] does not recognize
/// it as `reason = "..."`) is excluded even though it would otherwise parse
/// as a one-`Ident` rule name — `reason` is never a rule name, matching the
/// string-based parser this one replaced. Any other argument that is not a
/// recognized rule name and not a `reason = "..."` pair (stray punctuation,
/// a group, a quoted or non-alphanumeric literal on its own, `foo::bar`'s
/// double colon) is dropped silently rather than rejecting the whole
/// attribute.
fn parse_allow_attr_tokens(tokens: TokenStream) -> Option<AllowDirective> {
    let mut rules = HashSet::new();
    let mut reason = None;

    for arg in split_on_commas(tokens) {
        match parse_reason_arg(&arg) {
            ReasonArg::Reason(value) => reason = Some(value),
            ReasonArg::Unsupported => {}
            ReasonArg::NotAReason => {
                if let Some(name) = parse_rule_name_arg(&arg) {
                    // A bare `reason` (no `=`) is not a rule name.
                    if name != "reason" {
                        rules.insert(name);
                    }
                }
                // Anything else (stray punctuation, a group, a
                // non-alphanumeric literal) names neither a rule nor
                // `reason = "..."`; drop it.
            }
        }
    }

    if rules.is_empty() {
        return None;
    }

    Some(AllowDirective { rules, reason })
}

/// Splits a token stream into comma-separated argument groups, dropping the
/// commas themselves. A trailing comma produces no empty trailing group.
fn split_on_commas(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut args = Vec::new();
    let mut current = Vec::new();

    for tt in tokens {
        if matches!(&tt, TokenTree::Punct(p) if p.as_char() == ',') {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(tt);
    }
    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Outcome of trying to recognize one argument as `reason = "<literal>"`.
enum ReasonArg {
    /// Not this shape at all; the caller tries it as a rule name instead.
    NotAReason,
    /// This shape, but the literal is not a string `syn` can read the
    /// value of; the caller drops the argument rather than mistaking it
    /// for a rule name.
    Unsupported,
    /// This shape, with the reason's string value.
    Reason(String),
}

/// Recognizes `reason = "<literal>"`: exactly `Ident("reason")`,
/// `Punct('=')`, then one literal token.
fn parse_reason_arg(arg: &[TokenTree]) -> ReasonArg {
    let [TokenTree::Ident(ident), TokenTree::Punct(eq), TokenTree::Literal(lit)] = arg else {
        return ReasonArg::NotAReason;
    };
    if ident != "reason" || eq.as_char() != '=' {
        return ReasonArg::NotAReason;
    }

    let literal_tokens = TokenStream::from(TokenTree::Literal(lit.clone()));
    match syn::parse2::<syn::Lit>(literal_tokens) {
        Ok(syn::Lit::Str(s)) => ReasonArg::Reason(s.value()),
        _ => ReasonArg::Unsupported,
    }
}

/// Builds a rule name by concatenating `arg`'s tokens in order: `Ident`s
/// verbatim, `Punct('-')` and `Punct(':')` as themselves (for kebab-case
/// names and `allow-dep:<scope>` display names), and a `Literal` whose
/// `to_string()` is plain ASCII alphanumerics/`_` (so a segment that starts
/// with a digit, like the `2` in `domain-infra-2` or `allow-dep:core-2`,
/// comes through as part of the name — a scope name may contain `[a-z0-9-]`,
/// so `domain-2` is a real scope name a rustc lexer tokenizes as `Ident
/// "domain"`, `Punct('-')`, `Literal("2")`, not three `Ident`s). A quoted
/// literal (`"x"`) or one containing `.` (`1.5`) fails the alphanumeric
/// check and disqualifies the argument, same as any other punctuation, a
/// group, or a literal that isn't this shape.
///
/// `:` is restricted to at most one occurrence, with a name token on each
/// side (checked after the loop, on the built string): `foo::bar` is
/// dropped like any other malformed argument, rather than silently
/// building the string `foo::bar` and asking the comparison sites to make
/// sense of a double colon that `allow-dep:<scope>` display names never
/// produce.
///
/// A name that starts with a digit cannot always be written at all: `2e`
/// (for a hypothetical scope literally named `2e`) is not expressible as
/// `#[arch_lint::allow(...-2e)]` because rustc's own lexer reads `2e` as an
/// incomplete floating-point exponent and rejects it before this function
/// — or even `arch_lint_macros::allow` — ever sees the tokens. That is a
/// lexer limitation, not something fixable here.
fn parse_rule_name_arg(arg: &[TokenTree]) -> Option<String> {
    if arg.is_empty() {
        return None;
    }

    let mut name = String::new();
    for tt in arg {
        match tt {
            TokenTree::Ident(ident) => name.push_str(&ident.to_string()),
            TokenTree::Punct(p) if p.as_char() == '-' || p.as_char() == ':' => {
                name.push(p.as_char());
            }
            TokenTree::Literal(lit) => {
                let text = lit.to_string();
                if text.is_empty() || !text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return None;
                }
                name.push_str(&text);
            }
            _ => return None,
        }
    }

    let colons = name.matches(':').count();
    if colons > 1 || (colons == 1 && (name.starts_with(':') || name.ends_with(':'))) {
        return None;
    }

    Some(name)
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
        let directive = parse_allow_attr_tokens(tokens("no_unwrap_expect")).unwrap();
        assert!(directive.rules.contains("no_unwrap_expect"));
        assert!(directive.reason.is_none());

        let directive =
            parse_allow_attr_tokens(tokens("no_unwrap_expect, reason = \"test reason\"")).unwrap();
        assert!(directive.rules.contains("no_unwrap_expect"));
        assert_eq!(directive.reason, Some("test reason".to_string()));

        let directive =
            parse_allow_attr_tokens(tokens("rule1, rule2, reason = \"multi\"")).unwrap();
        assert!(directive.rules.contains("rule1"));
        assert!(directive.rules.contains("rule2"));
        assert_eq!(directive.reason, Some("multi".to_string()));
    }

    /// Parses `s` the same way rustc would tokenize the inside of
    /// `#[arch_lint::allow(...)]`, for feeding directly to
    /// [`parse_allow_attr_tokens`] in isolation (no `Attribute` needed).
    fn tokens(s: &str) -> TokenStream {
        s.parse().expect("valid token stream")
    }

    // ── `parse_allow_attr_tokens` / token walker (kebab-attribute-allow) ──

    #[test]
    fn token_walker_accepts_a_kebab_case_name() {
        let directive = parse_allow_attr_tokens(tokens("deny-scope-dep")).unwrap();
        assert!(directive.rules.contains("deny-scope-dep"));
        assert!(directive.reason.is_none());
    }

    #[test]
    fn token_walker_accepts_a_snake_case_name() {
        let directive = parse_allow_attr_tokens(tokens("deny_scope_dep")).unwrap();
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    #[test]
    fn token_walker_accepts_a_rule_code() {
        let directive = parse_allow_attr_tokens(tokens("ALD003")).unwrap();
        assert!(directive.rules.contains("ALD003"));
    }

    #[test]
    fn token_walker_accepts_two_names_and_a_reason() {
        let directive =
            parse_allow_attr_tokens(tokens(r#"deny-scope-dep, ALD009, reason = "x""#)).unwrap();
        assert!(directive.rules.contains("deny-scope-dep"));
        assert!(directive.rules.contains("ALD009"));
        assert_eq!(directive.reason, Some("x".to_string()));
    }

    #[test]
    fn token_walker_with_only_a_reason_and_no_names_is_none() {
        assert!(parse_allow_attr_tokens(tokens(r#"reason = "x""#)).is_none());
    }

    #[test]
    fn token_walker_accepts_allow_dep_display_name_with_colon() {
        let directive = parse_allow_attr_tokens(tokens("allow-dep:domain")).unwrap();
        assert!(directive.rules.contains("allow-dep:domain"));
    }

    // ── review follow-up: digit-leading segments (`domain-infra-2`) ──

    #[test]
    fn token_walker_accepts_a_digit_leading_trailing_segment() {
        // `rule-2` tokenizes as `Ident("rule")`, `Punct('-')`,
        // `Literal("2")` — the literal is not an `Ident`, but a scope name
        // may contain `[a-z0-9-]`, so a segment starting with a digit is a
        // real name that must come through.
        let directive = parse_allow_attr_tokens(tokens("rule-2")).unwrap();
        assert!(directive.rules.contains("rule-2"), "{directive:#?}");
    }

    #[test]
    fn token_walker_accepts_allow_dep_display_name_with_digit_leading_scope() {
        let directive = parse_allow_attr_tokens(tokens("allow-dep:core-2")).unwrap();
        assert!(
            directive.rules.contains("allow-dep:core-2"),
            "{directive:#?}"
        );
    }

    #[test]
    fn token_walker_rejects_a_float_literal_segment() {
        // `1.5` is a single `Literal` token, but its text contains `.`,
        // which fails the alphanumeric/`_` check — unlike `2` or `1e5`,
        // this is not a valid scope-name segment and the argument is
        // dropped.
        assert!(parse_allow_attr_tokens(tokens("rule-1.5")).is_none());
    }

    // ── review follow-up: `:` restricted to one occurrence, named both sides ──

    #[test]
    fn token_walker_ignores_a_double_colon_argument() {
        // `foo::bar` is dropped (stray punctuation, per the function's own
        // doc), not read as the two-colon string `"foo::bar"`.
        let directive = parse_allow_attr_tokens(tokens("foo::bar, deny_scope_dep")).unwrap();
        assert!(!directive.rules.iter().any(|r| r.contains("foo")));
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    // ── review follow-up: a bare `reason` (no `=`) is not a rule name ──

    #[test]
    fn token_walker_bare_reason_without_equals_is_not_a_rule_name() {
        let directive = parse_allow_attr_tokens(tokens("reason, deny_scope_dep")).unwrap();
        assert!(!directive.rules.contains("reason"));
        assert!(directive.rules.contains("deny_scope_dep"));

        // With no other argument, a bare `reason` alone names nothing.
        assert!(parse_allow_attr_tokens(tokens("reason")).is_none());
    }

    // ── review follow-up: more shapes the walker should ignore/handle ──

    #[test]
    fn token_walker_ignores_a_dotted_argument() {
        let directive = parse_allow_attr_tokens(tokens("foo.bar, deny_scope_dep")).unwrap();
        assert!(!directive.rules.iter().any(|r| r.contains("foo")));
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    #[test]
    fn token_walker_ignores_a_bang_suffixed_argument() {
        let directive = parse_allow_attr_tokens(tokens("foo!, deny_scope_dep")).unwrap();
        assert!(!directive.rules.iter().any(|r| r.contains("foo")));
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    #[test]
    fn token_walker_trailing_comma_produces_no_empty_argument() {
        let directive = parse_allow_attr_tokens(tokens("deny_scope_dep,")).unwrap();
        assert_eq!(directive.rules.len(), 1, "{directive:#?}");
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    #[test]
    fn token_walker_spaced_hyphens_are_still_one_name() {
        // Whitespace between tokens is not itself a token, so
        // `deny - scope - dep` tokenizes identically to `deny-scope-dep`.
        let directive = parse_allow_attr_tokens(tokens("deny - scope - dep")).unwrap();
        assert_eq!(directive.rules.len(), 1, "{directive:#?}");
        assert!(directive.rules.contains("deny-scope-dep"));
    }

    #[test]
    fn token_walker_ignores_an_argument_with_unknown_punctuation() {
        // `rule1 + rule2` is not a valid rule-name shape (stray `+`); it is
        // dropped rather than rejecting the whole attribute, so the other,
        // valid argument still comes through.
        let directive = parse_allow_attr_tokens(tokens("rule1 + rule2, deny_scope_dep")).unwrap();
        assert!(!directive.rules.contains("rule1"));
        assert!(!directive.rules.contains("rule2"));
        assert!(directive.rules.contains("deny_scope_dep"));
    }

    #[test]
    fn token_walker_ignores_a_group_argument() {
        // A parenthesized group can't be part of a rule name either.
        assert!(parse_allow_attr_tokens(tokens("(nested)")).is_none());
    }

    #[test]
    fn token_walker_reason_with_raw_string_literal_is_read_via_syn() {
        // `syn::Lit::Str` covers both `"..."` and raw-string literals the
        // same way (both are string literals; a raw string just has no
        // escapes to unescape), so this is not a special case in the
        // implementation — asserted here so a future change that regresses
        // it is caught.
        let directive =
            parse_allow_attr_tokens(tokens(r##"deny_scope_dep, reason = r"x""##)).unwrap();
        assert_eq!(directive.reason, Some("x".to_string()));
    }

    #[test]
    fn token_walker_reason_with_non_string_literal_is_dropped() {
        // `reason = 1` matches the `Ident '=' Literal` shape but the
        // literal is not a string `syn::Lit::Str` can read a value from;
        // the argument is dropped rather than treated as a rule name.
        let directive = parse_allow_attr_tokens(tokens("deny_scope_dep, reason = 1")).unwrap();
        assert!(directive.rules.contains("deny_scope_dep"));
        assert!(directive.reason.is_none());
    }
}
