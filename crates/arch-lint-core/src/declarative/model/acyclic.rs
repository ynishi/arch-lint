//! The `[[acyclic]]` rule entity (ALD009).

use super::ScopeName;
use crate::types::Severity;

/// A project-wide check that the scope graph has no cycle.
///
/// Unlike every other rule kind, `[[acyclic]]` carries no `message`: the
/// violation message is generated from the cycle itself (see
/// [`AcyclicRule`](crate::declarative::rules::AcyclicRule)).
#[derive(Debug, Clone)]
pub struct Acyclic {
    name: Option<String>,
    scopes: Option<Vec<ScopeName>>,
    doc_ref: Option<String>,
    severity: Severity,
}

impl Acyclic {
    /// Creates a new acyclic rule.
    ///
    /// `scopes`, when `None`, means every declared scope is part of the
    /// cycle check.
    #[must_use]
    pub fn new(
        name: Option<String>,
        scopes: Option<Vec<ScopeName>>,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scopes,
            doc_ref,
            severity,
        }
    }

    /// Returns the rule name, or the fallback `"acyclic"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| "acyclic".to_string())
    }

    /// Returns the scopes the cycle check is restricted to, or `None` for
    /// every declared scope.
    #[must_use]
    pub fn scopes(&self) -> Option<&[ScopeName]> {
        self.scopes.as_deref()
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
    fn acyclic_display_name_fallback() {
        let rule = Acyclic::new(None, None, None, Severity::Error);
        assert_eq!(rule.display_name(), "acyclic");
        assert!(rule.scopes().is_none());
        assert!(rule.doc_ref().is_none());
    }

    #[test]
    fn acyclic_display_name_explicit() {
        let rule = Acyclic::new(
            Some("no-cycles".to_string()),
            Some(vec![ScopeName::new("domain").unwrap()]),
            Some("ARCHITECTURE.md L10".to_string()),
            Severity::Warning,
        );
        assert_eq!(rule.display_name(), "no-cycles");
        assert_eq!(rule.scopes().unwrap()[0].as_str(), "domain");
        assert_eq!(rule.doc_ref(), Some("ARCHITECTURE.md L10"));
        assert_eq!(rule.severity(), Severity::Warning);
    }
}
