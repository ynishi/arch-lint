//! The `[[allow-dep]]` rule entity (ALD011).

use super::{ModelError, ScopeName};
use crate::types::Severity;

/// A project-wide check that a scope depends on nothing outside an explicit
/// allow-list of other declared scopes.
///
/// Unlike [`Independent`](super::Independent) (a symmetric "these scopes
/// must not depend on each other at all"), `[[allow-dep]]` is directional: a
/// deny-by-default allow-list on `from`'s dependencies. Like
/// [`Independent`](super::Independent), it carries no `message`: the
/// violation message is generated from the offending edge itself (see
/// [`AllowDepRule`](crate::declarative::rules::AllowDepRule)).
#[derive(Debug, Clone)]
pub struct AllowDep {
    name: Option<String>,
    from: ScopeName,
    to: Vec<ScopeName>,
    doc_ref: Option<String>,
    severity: Severity,
}

impl AllowDep {
    /// Creates a new allow-list dependency rule.
    ///
    /// `to` may be empty: that forbids every declared-scope dependency from
    /// `from`.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::AllowDepToContainsFrom`] if `to` contains
    /// `from` itself, or [`ModelError::DuplicateAllowDepTarget`] if `to`
    /// repeats a scope name.
    pub fn new(
        name: Option<String>,
        from: ScopeName,
        to: Vec<ScopeName>,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Result<Self, ModelError> {
        if to.contains(&from) {
            return Err(ModelError::AllowDepToContainsFrom { name: from });
        }
        for (i, scope) in to.iter().enumerate() {
            if to[..i].contains(scope) {
                return Err(ModelError::DuplicateAllowDepTarget {
                    name: scope.clone(),
                });
            }
        }
        Ok(Self {
            name,
            from,
            to,
            doc_ref,
            severity,
        })
    }

    /// Returns the rule name, or the fallback `"allow-dep:{from}"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("allow-dep:{}", self.from))
    }

    /// Returns the source scope.
    #[must_use]
    pub fn from_scope(&self) -> &ScopeName {
        &self.from
    }

    /// Returns the complete list of scopes `from` may depend on.
    #[must_use]
    pub fn to(&self) -> &[ScopeName] {
        &self.to
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

    /// Checks if a target scope is allowed.
    #[must_use]
    pub fn is_allowed(&self, target: &ScopeName) -> bool {
        self.to.contains(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_dep_display_name_fallback() {
        let rule = AllowDep::new(
            None,
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("shared").unwrap()],
            None,
            Severity::Error,
        )
        .unwrap();
        assert_eq!(rule.display_name(), "allow-dep:domain");
        assert_eq!(rule.to().len(), 1);
        assert!(rule.doc_ref().is_none());
    }

    #[test]
    fn allow_dep_display_name_explicit() {
        let rule = AllowDep::new(
            Some("domain-allow".to_string()),
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("shared").unwrap()],
            Some("ARCHITECTURE.md L30".to_string()),
            Severity::Warning,
        )
        .unwrap();
        assert_eq!(rule.display_name(), "domain-allow");
        assert_eq!(rule.from_scope().as_str(), "domain");
        assert_eq!(rule.to()[0].as_str(), "shared");
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L30"));
        assert_eq!(rule.severity(), Severity::Warning);
    }

    #[test]
    fn allow_dep_accepts_an_empty_to_list() {
        let rule = AllowDep::new(
            None,
            ScopeName::new("domain").unwrap(),
            vec![],
            None,
            Severity::Error,
        )
        .unwrap();
        assert!(rule.to().is_empty());
        assert!(!rule.is_allowed(&ScopeName::new("shared").unwrap()));
    }

    #[test]
    fn allow_dep_rejects_to_containing_from() {
        assert!(matches!(
            AllowDep::new(
                None,
                ScopeName::new("domain").unwrap(),
                vec![
                    ScopeName::new("shared").unwrap(),
                    ScopeName::new("domain").unwrap(),
                ],
                None,
                Severity::Error,
            ),
            Err(ModelError::AllowDepToContainsFrom { name }) if name.as_str() == "domain"
        ));
    }

    #[test]
    fn allow_dep_rejects_a_duplicate_target() {
        assert!(matches!(
            AllowDep::new(
                None,
                ScopeName::new("domain").unwrap(),
                vec![
                    ScopeName::new("shared").unwrap(),
                    ScopeName::new("shared").unwrap(),
                ],
                None,
                Severity::Error,
            ),
            Err(ModelError::DuplicateAllowDepTarget { name }) if name.as_str() == "shared"
        ));
    }

    #[test]
    fn allow_dep_is_allowed() {
        let rule = AllowDep::new(
            None,
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("shared").unwrap()],
            None,
            Severity::Error,
        )
        .unwrap();
        assert!(rule.is_allowed(&ScopeName::new("shared").unwrap()));
        assert!(!rule.is_allowed(&ScopeName::new("infra").unwrap()));
    }
}
