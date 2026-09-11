//! Named file scopes and the references rules use to point at them.

use super::{GlobPattern, ScopeName};
use std::path::Path;

/// A named file scope — a set of files matched by glob patterns.
#[derive(Debug, Clone)]
pub struct Scope {
    name: ScopeName,
    patterns: Vec<GlobPattern>,
    excludes: Vec<GlobPattern>,
}

impl Scope {
    /// Creates a new scope with no exclusions.
    ///
    /// Use [`Self::with_excludes`] to subtract files from the scope.
    #[must_use]
    pub fn new(name: ScopeName, patterns: Vec<GlobPattern>) -> Self {
        Self {
            name,
            patterns,
            excludes: Vec::new(),
        }
    }

    /// Sets the exclusion patterns — files matching any of these are not in
    /// the scope even when they match one of the `paths` patterns.
    #[must_use]
    pub fn with_excludes(mut self, excludes: Vec<GlobPattern>) -> Self {
        self.excludes = excludes;
        self
    }

    /// Returns the scope name.
    #[must_use]
    pub fn name(&self) -> &ScopeName {
        &self.name
    }

    /// Returns the glob patterns.
    #[must_use]
    pub fn patterns(&self) -> &[GlobPattern] {
        &self.patterns
    }

    /// Returns the exclusion glob patterns.
    #[must_use]
    pub fn excludes(&self) -> &[GlobPattern] {
        &self.excludes
    }

    /// Tests whether a relative file path belongs to this scope.
    ///
    /// A path belongs to the scope when it matches at least one `paths`
    /// pattern and no `exclude` pattern.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
            && !self.excludes.iter().any(|p| p.matches(path))
    }
}

/// Reference to a scope — either by name or inline patterns.
#[derive(Debug, Clone)]
pub enum ScopeRef {
    /// Reference to a named scope defined in `[[scopes]]`.
    Named(ScopeName),
    /// Inline file patterns (no named scope required).
    Inline(Vec<GlobPattern>),
}

impl ScopeRef {
    /// Tests whether a relative file path matches this scope reference.
    ///
    /// For `Named`, requires the scope registry to resolve. Use
    /// [`crate::declarative::model::DeclarativeConfig::scope_ref_contains`]
    /// instead. For `Inline`, matches directly against the patterns.
    #[must_use]
    pub fn matches_inline(&self, path: &Path) -> bool {
        match self {
            Self::Named(_) => false, // Must resolve via DeclarativeConfig
            Self::Inline(patterns) => patterns.iter().any(|p| p.matches(path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_contains_file() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![
                GlobPattern::new("src/domain/**").unwrap(),
                GlobPattern::new("src/core/**").unwrap(),
            ],
        );
        assert!(scope.contains(Path::new("src/domain/entity.rs")));
        assert!(scope.contains(Path::new("src/core/types.rs")));
        assert!(!scope.contains(Path::new("src/infra/db.rs")));
    }

    #[test]
    fn scope_exclude_removes_matching_file() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/**").unwrap()],
        )
        .with_excludes(vec![GlobPattern::new("src/**/tests/**").unwrap()]);

        assert!(scope.contains(Path::new("src/domain/entity.rs")));
        assert!(!scope.contains(Path::new("src/domain/tests/entity_test.rs")));
        assert_eq!(scope.excludes().len(), 1);
    }

    #[test]
    fn scope_without_excludes_matches_everything_in_paths() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/**").unwrap()],
        );
        assert!(scope.contains(Path::new("src/domain/tests/entity_test.rs")));
    }
}
