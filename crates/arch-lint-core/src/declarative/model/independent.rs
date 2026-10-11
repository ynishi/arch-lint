//! The `[[independent]]` rule entity (ALD010).

use super::{ModelError, ScopeName};
use crate::types::Severity;

/// A project-wide check that no two of the given scopes depend on each
/// other, in either direction.
///
/// Unlike every other declarative rule, `[[independent]]` carries no
/// `message`: the violation message is generated from the offending edge
/// itself (see
/// [`IndependentRule`](crate::declarative::rules::IndependentRule)).
#[derive(Debug, Clone)]
pub struct Independent {
    name: Option<String>,
    scopes: Vec<ScopeName>,
    doc_ref: Option<String>,
    severity: Severity,
}

impl Independent {
    /// Creates a new independent-scopes rule.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::IndependentTooFewScopes`] if `scopes` has fewer
    /// than two entries, or [`ModelError::DuplicateIndependentScope`] if it
    /// repeats a scope name — otherwise the repeated name collapses to one
    /// graph node and the rule silently checks nothing against it.
    pub fn new(
        name: Option<String>,
        scopes: Vec<ScopeName>,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Result<Self, ModelError> {
        if scopes.len() < 2 {
            return Err(ModelError::IndependentTooFewScopes {
                count: scopes.len(),
            });
        }
        for (i, scope) in scopes.iter().enumerate() {
            if scopes[..i].contains(scope) {
                return Err(ModelError::DuplicateIndependentScope {
                    name: scope.clone(),
                });
            }
        }
        Ok(Self {
            name,
            scopes,
            doc_ref,
            severity,
        })
    }

    /// Returns the rule name, or the fallback `"independent"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| "independent".to_string())
    }

    /// Returns the scopes that must not depend on each other.
    #[must_use]
    pub fn scopes(&self) -> &[ScopeName] {
        &self.scopes
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

    #[test]
    fn independent_display_name_fallback() {
        let rule = Independent::new(
            None,
            vec![ScopeName::new("a").unwrap(), ScopeName::new("b").unwrap()],
            None,
            Severity::Error,
        )
        .unwrap();
        assert_eq!(rule.display_name(), "independent");
        assert_eq!(rule.scopes().len(), 2);
        assert!(rule.doc_ref().is_none());
    }

    #[test]
    fn independent_display_name_explicit() {
        let rule = Independent::new(
            Some("no-shared-deps".to_string()),
            vec![
                ScopeName::new("billing").unwrap(),
                ScopeName::new("shipping").unwrap(),
            ],
            Some("ARCHITECTURE.md L20".to_string()),
            Severity::Warning,
        )
        .unwrap();
        assert_eq!(rule.display_name(), "no-shared-deps");
        assert_eq!(rule.scopes()[0].as_str(), "billing");
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L20"));
        assert_eq!(rule.severity(), Severity::Warning);
    }

    #[test]
    fn independent_rejects_fewer_than_two_scopes() {
        assert!(matches!(
            Independent::new(
                None,
                vec![ScopeName::new("a").unwrap()],
                None,
                Severity::Error,
            ),
            Err(ModelError::IndependentTooFewScopes { count: 1 })
        ));
        assert!(matches!(
            Independent::new(None, vec![], None, Severity::Error),
            Err(ModelError::IndependentTooFewScopes { count: 0 })
        ));
    }

    #[test]
    fn independent_rejects_a_duplicate_scope() {
        assert!(matches!(
            Independent::new(
                None,
                vec![
                    ScopeName::new("billing").unwrap(),
                    ScopeName::new("billing").unwrap(),
                ],
                None,
                Severity::Error,
            ),
            Err(ModelError::DuplicateIndependentScope { name }) if name.as_str() == "billing"
        ));
        assert!(matches!(
            Independent::new(
                None,
                vec![
                    ScopeName::new("a").unwrap(),
                    ScopeName::new("b").unwrap(),
                    ScopeName::new("a").unwrap(),
                ],
                None,
                Severity::Error,
            ),
            Err(ModelError::DuplicateIndependentScope { name }) if name.as_str() == "a"
        ));
    }
}
