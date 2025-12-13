//! Path utilities for AST analysis.

use syn::Path;

/// Converts a syn Path to a string representation.
///
/// # Example
///
/// ```ignore
/// // For path `std::fs::read`
/// let s = path_to_string(&path);
/// assert_eq!(s, "std::fs::read");
/// ```
#[must_use]
pub fn path_to_string(path: &Path) -> String {
    path.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Checks if a path matches a pattern.
///
/// Supports wildcards:
/// - `*` matches any single segment
/// - `**` matches any number of segments
///
/// # Examples
///
/// ```ignore
/// assert!(path_matches("std::fs::read", "std::fs::*"));
/// assert!(path_matches("std::fs::read", "std::**"));
/// assert!(!path_matches("std::fs::read", "tokio::*"));
/// ```
#[must_use]
pub fn path_matches(path: &str, pattern: &str) -> bool {
    let path_parts: Vec<&str> = path.split("::").collect();
    let pattern_parts: Vec<&str> = pattern.split("::").collect();

    match_parts(&path_parts, &pattern_parts)
}

fn match_parts(path: &[&str], pattern: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }

    let (first_pattern, rest_pattern) = (pattern[0], &pattern[1..]);

    match first_pattern {
        "**" => {
            // Try matching zero or more segments
            for i in 0..=path.len() {
                if match_parts(&path[i..], rest_pattern) {
                    return true;
                }
            }
            false
        }
        "*" => {
            // Match exactly one segment
            if path.is_empty() {
                false
            } else {
                match_parts(&path[1..], rest_pattern)
            }
        }
        literal => {
            // Match literal segment
            if path.is_empty() || path[0] != literal {
                false
            } else {
                match_parts(&path[1..], rest_pattern)
            }
        }
    }
}

/// Extracts the last segment from a path string.
#[must_use]
pub fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// Checks if a path is from a specific crate/module.
#[must_use]
pub fn is_from_module(path: &str, module: &str) -> bool {
    path.starts_with(module) && path.get(module.len()..module.len() + 2) == Some("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches_literal() {
        assert!(path_matches("std::fs::read", "std::fs::read"));
        assert!(!path_matches("std::fs::read", "std::fs::write"));
    }

    #[test]
    fn test_path_matches_wildcard() {
        assert!(path_matches("std::fs::read", "std::fs::*"));
        assert!(path_matches("std::fs::write", "std::fs::*"));
        assert!(!path_matches("std::io::read", "std::fs::*"));
    }

    #[test]
    fn test_path_matches_globstar() {
        assert!(path_matches("std::fs::read", "std::**"));
        assert!(path_matches("std::fs::read", "std::fs::**"));
        assert!(path_matches(
            "std::collections::hash_map::HashMap",
            "std::**"
        ));
    }

    #[test]
    fn test_last_segment() {
        assert_eq!(last_segment("std::fs::read"), "read");
        assert_eq!(last_segment("read"), "read");
    }

    #[test]
    fn test_is_from_module() {
        assert!(is_from_module("std::fs::read", "std"));
        assert!(is_from_module("std::fs::read", "std::fs"));
        assert!(!is_from_module("tokio::fs::read", "std"));
    }
}
