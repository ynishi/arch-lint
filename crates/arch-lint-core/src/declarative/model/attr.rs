//! The `[[deny-attr]]` (ALD005) and `[[require-attr]]` (ALD006) rule entities.

use super::{AttrPattern, ItemKind, ScopeRef, VisibilityFilter};
use crate::types::Severity;

/// An attribute-denial rule: items in a scope must not carry given attributes.
#[derive(Debug, Clone)]
pub struct DenyAttr {
    name: String,
    scope: ScopeRef,
    attrs: Vec<AttrPattern>,
    on: Vec<ItemKind>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl DenyAttr {
    /// Creates a new deny-attr rule.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        attrs: Vec<AttrPattern>,
        on: Vec<ItemKind>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            attrs,
            on,
            message,
            doc_ref,
            severity,
        }
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

    /// Returns the denied attribute patterns.
    #[must_use]
    pub fn attrs(&self) -> &[AttrPattern] {
        &self.attrs
    }

    /// Returns the item kinds this rule applies to.
    #[must_use]
    pub fn on(&self) -> &[ItemKind] {
        &self.on
    }

    /// Returns whether this rule applies to the given item kind.
    #[must_use]
    pub fn applies_to(&self, kind: ItemKind) -> bool {
        self.on.contains(&kind)
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

/// An attribute-requirement rule: items in a scope must carry one of the
/// given attributes.
#[derive(Debug, Clone)]
pub struct RequireAttr {
    name: String,
    scope: ScopeRef,
    attrs: Vec<AttrPattern>,
    on: Vec<ItemKind>,
    visibility: VisibilityFilter,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RequireAttr {
    /// Creates a new require-attr rule checking `pub` items.
    ///
    /// Use [`Self::with_visibility`] to widen it to every item.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        attrs: Vec<AttrPattern>,
        on: Vec<ItemKind>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            attrs,
            on,
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

    /// Returns the required attribute patterns (at least one must be present).
    #[must_use]
    pub fn attrs(&self) -> &[AttrPattern] {
        &self.attrs
    }

    /// Returns the item kinds this rule applies to.
    #[must_use]
    pub fn on(&self) -> &[ItemKind] {
        &self.on
    }

    /// Returns whether this rule applies to the given item kind.
    #[must_use]
    pub fn applies_to(&self, kind: ItemKind) -> bool {
        self.on.contains(&kind)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::GlobPattern;

    #[test]
    fn attr_rules_applies_to_item_kind() {
        let deny = DenyAttr::new(
            "no-orm".to_string(),
            ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
            vec![AttrPattern::new("sqlx::FromRow").unwrap()],
            vec![ItemKind::Struct, ItemKind::Enum],
            "msg".to_string(),
            None,
            Severity::Error,
        );
        assert!(deny.applies_to(ItemKind::Struct));
        assert!(!deny.applies_to(ItemKind::Fn));
        assert_eq!(deny.attrs().len(), 1);
        assert_eq!(deny.on().len(), 2);
    }
}
