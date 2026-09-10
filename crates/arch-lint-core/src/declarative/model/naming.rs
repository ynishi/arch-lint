//! The `[[naming]]` rule entity (ALD007).

use super::{ItemKind, RegexPattern, ScopeRef, VisibilityFilter};
use crate::types::Severity;

/// A naming rule: identifiers of one item kind must match a regex.
#[derive(Debug, Clone)]
pub struct Naming {
    name: String,
    scope: ScopeRef,
    item: ItemKind,
    pattern: RegexPattern,
    visibility: VisibilityFilter,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl Naming {
    /// Creates a new naming rule checking `pub` items.
    ///
    /// Use [`Self::with_visibility`] to widen it to every item.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        item: ItemKind,
        pattern: RegexPattern,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            item,
            pattern,
            visibility: VisibilityFilter::Pub,
            message,
            doc_ref,
            severity,
        }
    }

    /// Sets which items are checked by visibility.
    #[must_use]
    pub fn with_visibility(mut self, visibility: VisibilityFilter) -> Self {
        self.visibility = visibility;
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

    /// Returns the targeted item kind.
    #[must_use]
    pub fn item(&self) -> ItemKind {
        self.item
    }

    /// Returns the identifier pattern.
    #[must_use]
    pub fn pattern(&self) -> &RegexPattern {
        &self.pattern
    }

    /// Returns the visibility filter.
    #[must_use]
    pub fn visibility(&self) -> VisibilityFilter {
        self.visibility
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
