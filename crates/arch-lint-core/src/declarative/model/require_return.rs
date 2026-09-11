//! The `[[require-return]]` rule entity (ALD008).

use super::{RegexPattern, ScopeRef, VisibilityFilter};
use crate::types::Severity;

/// A return-type rule: functions in a scope must return a matching type.
#[derive(Debug, Clone)]
pub struct RequireReturn {
    name: String,
    scope: ScopeRef,
    visibility: VisibilityFilter,
    asyncness: Option<bool>,
    pattern: RegexPattern,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RequireReturn {
    /// Creates a new require-return rule checking every `pub` function.
    ///
    /// Use [`Self::with_visibility`] and [`Self::with_asyncness`] to narrow it.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        pattern: RegexPattern,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            visibility: VisibilityFilter::Pub,
            asyncness: None,
            pattern,
            message,
            doc_ref,
            severity,
        }
    }

    /// Sets which functions are checked by visibility.
    #[must_use]
    pub fn with_visibility(mut self, visibility: VisibilityFilter) -> Self {
        self.visibility = visibility;
        self
    }

    /// Restricts the rule to `async` (`Some(true)`) or non-`async`
    /// (`Some(false)`) functions; `None` checks every function.
    #[must_use]
    pub fn with_asyncness(mut self, asyncness: Option<bool>) -> Self {
        self.asyncness = asyncness;
        self
    }

    /// Returns the rule name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the scope reference.
    #[must_use]
    pub fn scope(&self) -> &ScopeRef {
        &self.scope
    }

    /// Returns the visibility filter.
    #[must_use]
    pub fn visibility(&self) -> VisibilityFilter {
        self.visibility
    }

    /// Returns the asyncness filter.
    #[must_use]
    pub fn asyncness(&self) -> Option<bool> {
        self.asyncness
    }

    /// Returns the return-type pattern.
    #[must_use]
    pub fn pattern(&self) -> &RegexPattern {
        &self.pattern
    }

    /// Returns whether a function with the given asyncness is checked.
    #[must_use]
    pub fn matches_asyncness(&self, is_async: bool) -> bool {
        self.asyncness.is_none_or(|want| want == is_async)
    }

    /// Returns the violation message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the document reference.
    #[must_use]
    pub fn doc_ref(&self) -> Option<&str> {
        self.doc_ref.as_deref()
    }

    /// Returns the severity.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::GlobPattern;

    #[test]
    fn require_return_asyncness_filter() {
        let rule = |asyncness| {
            RequireReturn::new(
                "r".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
                RegexPattern::new("^Result<").unwrap(),
                "msg".to_string(),
                None,
                Severity::Error,
            )
            .with_asyncness(asyncness)
        };
        assert!(rule(None).matches_asyncness(true));
        assert!(rule(None).matches_asyncness(false));
        assert!(rule(Some(true)).matches_asyncness(true));
        assert!(!rule(Some(true)).matches_asyncness(false));
    }
}
