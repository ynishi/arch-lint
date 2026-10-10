//! Validated vocabulary newtypes shared by every rule kind.
//!
//! Each type validates its input once, at construction, so the rule entities
//! and the aggregate root only ever hold well-formed values.

use super::ModelError;
use std::fmt;
use std::path::Path;

/// A validated scope name (non-empty, `[a-z0-9-]` only).
///
/// Orders lexically, so scope names can key a `BTreeMap` and graph output is
/// deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeName(String);

impl ScopeName {
    /// Creates a new scope name.
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty or contains invalid characters.
    pub fn new(name: &str) -> Result<Self, ModelError> {
        if name.is_empty() {
            return Err(ModelError::EmptyScopeName);
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ModelError::InvalidScopeName {
                name: name.to_string(),
            });
        }
        Ok(Self(name.to_string()))
    }

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A validated glob pattern for file path matching.
///
/// The glob is compiled once at construction and reused for all match calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern {
    raw: String,
    compiled: glob::Pattern,
}

impl GlobPattern {
    /// Creates a new glob pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or has invalid glob syntax.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyGlobPattern);
        }
        let compiled = glob::Pattern::new(pattern).map_err(|e| ModelError::InvalidGlobPattern {
            pattern: pattern.to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            raw: pattern.to_string(),
            compiled,
        })
    }

    /// Tests whether a relative file path matches this pattern.
    #[must_use]
    pub fn matches(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        if self.compiled.matches(&path_str) {
            return true;
        }
        // Fallback: `glob::Pattern` treats `**` and `*` equivalently by default
        // (both match `/`). For `dir/**` patterns, also check prefix + boundary
        // to handle edge cases where the glob crate doesn't match as expected.
        if let Some(prefix) = self.raw.strip_suffix("/**") {
            let normalized = prefix.trim_end_matches('/');
            if path_str.starts_with(normalized)
                && path_str
                    .as_bytes()
                    .get(normalized.len())
                    .is_some_and(|&b| b == b'/')
            {
                return true;
            }
        }
        false
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// A validated use-path pattern for matching Rust import paths.
///
/// Supports `::` separated segments with `*` (one segment) and `**` (any segments).
/// Examples: `sqlx::*`, `std::fs::**`, `tokio::fs::read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsePattern(String);

impl UsePattern {
    /// Creates a new use-path pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyUsePattern);
        }
        Ok(Self(pattern.to_string()))
    }

    /// Tests whether a Rust use path matches this pattern.
    ///
    /// Uses `::` separated segment matching with `*` and `**` wildcards.
    #[must_use]
    pub fn matches(&self, use_path: &str) -> bool {
        crate::utils::paths::path_matches(use_path, &self.0)
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated regular expression, used by the naming and return-type rules.
///
/// The regex is compiled once at construction and reused for all match calls.
#[derive(Debug, Clone)]
pub struct RegexPattern {
    raw: String,
    compiled: regex::Regex,
}

impl RegexPattern {
    /// Creates a new regex pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or is not a valid regex.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyRegexPattern);
        }
        let compiled = regex::Regex::new(pattern).map_err(|e| ModelError::InvalidRegexPattern {
            pattern: pattern.to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            raw: pattern.to_string(),
            compiled,
        })
    }

    /// Tests whether the regex matches anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.compiled.is_match(text)
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// A validated attribute pattern such as `tracing::instrument`,
/// `serde::Serialize` or `derive(sqlx::FromRow)`.
#[derive(Debug, Clone)]
pub struct AttrPattern {
    raw: String,
    derive_inner: Option<String>,
}

impl AttrPattern {
    /// Creates a new attribute pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or is a `derive()` with no
    /// path inside.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        let raw = pattern.trim();
        if raw.is_empty() {
            return Err(ModelError::EmptyAttrPattern);
        }
        let derive_inner = raw
            .strip_prefix("derive(")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(str::trim);
        if let Some(inner) = derive_inner {
            if inner.is_empty() {
                return Err(ModelError::InvalidAttrPattern {
                    pattern: raw.to_string(),
                    reason: "`derive(..)` needs a path inside the parentheses".to_string(),
                });
            }
        }
        Ok(Self {
            raw: raw.to_string(),
            derive_inner: derive_inner.map(ToString::to_string),
        })
    }

    /// Returns the pattern as written in the TOML.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Tests whether this pattern matches an attribute.
    ///
    /// `attr_path` is the attribute's own path rendered with `::`
    /// (`tracing::instrument`, `derive`, ...); `derive_paths` holds the paths
    /// inside a `#[derive(..)]` list (empty for any other attribute).
    ///
    /// A `derive(P)` pattern matches only derive lists. A bare pattern matches
    /// either the attribute path itself or an entry of a derive list. A derive
    /// entry matches when it equals the pattern in full, or when its last
    /// segment equals the pattern — so `FromRow` matches both
    /// `#[derive(FromRow)]` and `#[derive(sqlx::FromRow)]`, while
    /// `sqlx::FromRow` matches only the latter.
    #[must_use]
    pub fn matches(&self, attr_path: &str, derive_paths: &[String]) -> bool {
        let needle = self.derive_inner.as_deref().unwrap_or(&self.raw);
        if self.derive_inner.is_none() && attr_path == needle {
            return true;
        }
        attr_path == "derive"
            && derive_paths
                .iter()
                .any(|d| d == needle || last_path_segment(d) == needle)
    }
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- ScopeName --

    #[test]
    fn scope_name_valid() {
        assert!(ScopeName::new("domain").is_ok());
        assert!(ScopeName::new("my-scope-123").is_ok());
    }

    #[test]
    fn scope_name_empty_rejected() {
        assert!(matches!(
            ScopeName::new(""),
            Err(ModelError::EmptyScopeName)
        ));
    }

    #[test]
    fn scope_name_invalid_chars_rejected() {
        assert!(matches!(
            ScopeName::new("Domain"),
            Err(ModelError::InvalidScopeName { .. })
        ));
        assert!(matches!(
            ScopeName::new("my_scope"),
            Err(ModelError::InvalidScopeName { .. })
        ));
    }

    // -- GlobPattern --

    #[test]
    fn glob_pattern_valid() {
        assert!(GlobPattern::new("src/domain/**").is_ok());
        assert!(GlobPattern::new("src/**/*.rs").is_ok());
    }

    #[test]
    fn glob_pattern_empty_rejected() {
        assert!(matches!(
            GlobPattern::new(""),
            Err(ModelError::EmptyGlobPattern)
        ));
    }

    #[test]
    fn glob_pattern_matches_file() {
        let pat = GlobPattern::new("src/domain/**").unwrap();
        assert!(pat.matches(Path::new("src/domain/entity.rs")));
        assert!(pat.matches(Path::new("src/domain/sub/deep.rs")));
        assert!(!pat.matches(Path::new("src/infra/db.rs")));
    }

    // -- UsePattern --

    #[test]
    fn use_pattern_valid() {
        assert!(UsePattern::new("sqlx::*").is_ok());
        assert!(UsePattern::new("std::fs::**").is_ok());
    }

    #[test]
    fn use_pattern_empty_rejected() {
        assert!(matches!(
            UsePattern::new(""),
            Err(ModelError::EmptyUsePattern)
        ));
    }

    #[test]
    fn use_pattern_matches() {
        let pat = UsePattern::new("sqlx::*").unwrap();
        assert!(pat.matches("sqlx::Pool"));
        assert!(pat.matches("sqlx::query"));
        assert!(!pat.matches("sqlx::pool::Pool")); // * = one segment only
        assert!(!pat.matches("diesel::Pool"));
    }

    #[test]
    fn use_pattern_globstar_matches() {
        let pat = UsePattern::new("std::fs::**").unwrap();
        assert!(pat.matches("std::fs::read"));
        assert!(pat.matches("std::fs::read_to_string"));
        assert!(pat.matches("std::fs"));
        assert!(!pat.matches("std::io::read"));
    }

    // -- RegexPattern --

    #[test]
    fn regex_pattern_valid_and_matching() {
        let pat = RegexPattern::new("^[A-Z][A-Za-z0-9]*(Repository|Port)$").unwrap();
        assert!(pat.is_match("UserRepository"));
        assert!(!pat.is_match("UserService"));
        assert_eq!(pat.as_str(), "^[A-Z][A-Za-z0-9]*(Repository|Port)$");
    }

    #[test]
    fn regex_pattern_rejects_empty_and_invalid() {
        assert!(matches!(
            RegexPattern::new(""),
            Err(ModelError::EmptyRegexPattern)
        ));
        assert!(matches!(
            RegexPattern::new("([a-z"),
            Err(ModelError::InvalidRegexPattern { .. })
        ));
    }

    // -- AttrPattern --

    #[test]
    fn attr_pattern_matches_plain_path() {
        let pat = AttrPattern::new("tracing::instrument").unwrap();
        assert!(pat.matches("tracing::instrument", &[]));
        assert!(!pat.matches("instrument", &[]));
    }

    #[test]
    fn attr_pattern_matches_derive_entry_by_full_path() {
        let pat = AttrPattern::new("sqlx::FromRow").unwrap();
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
        // last-segment-only spelling in the code does not match a qualified pattern
        assert!(!pat.matches("derive", &["FromRow".to_string()]));
    }

    #[test]
    fn attr_pattern_bare_name_matches_both_derive_spellings() {
        let pat = AttrPattern::new("FromRow").unwrap();
        assert!(pat.matches("derive", &["FromRow".to_string()]));
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
    }

    #[test]
    fn attr_pattern_derive_wrapper_only_matches_derives() {
        let pat = AttrPattern::new("derive(sqlx::FromRow)").unwrap();
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
        assert!(!pat.matches("sqlx::FromRow", &[]));
    }

    #[test]
    fn attr_pattern_rejects_empty_and_empty_derive() {
        assert!(matches!(
            AttrPattern::new("  "),
            Err(ModelError::EmptyAttrPattern)
        ));
        assert!(matches!(
            AttrPattern::new("derive()"),
            Err(ModelError::InvalidAttrPattern { .. })
        ));
    }
}
