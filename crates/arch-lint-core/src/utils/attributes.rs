//! Attribute parsing utilities for rule implementations.

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
}
