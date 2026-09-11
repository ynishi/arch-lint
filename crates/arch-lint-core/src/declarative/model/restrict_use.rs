//! The `[[restrict-use]]` rule entity (ALD001).

use super::{ScopeRef, UsePattern};
use crate::types::Severity;

/// A use-restriction rule: deny certain imports within a scope.
#[derive(Debug, Clone)]
pub struct RestrictUse {
    name: String,
    scope: ScopeRef,
    deny: Vec<UsePattern>,
    except: Vec<UsePattern>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
    check_inline: bool,
}

impl RestrictUse {
    /// Creates a new restrict-use rule.
    ///
    /// Inline qualified-path checking is enabled by default;
    /// use [`Self::with_check_inline`] to disable it.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        deny: Vec<UsePattern>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            deny,
            except: Vec::new(),
            message,
            doc_ref,
            severity,
            check_inline: true,
        }
    }

    /// Sets whether inline qualified paths (e.g. `sqlx::query(...)` outside
    /// a `use` statement) are also checked.
    #[must_use]
    pub fn with_check_inline(mut self, check_inline: bool) -> Self {
        self.check_inline = check_inline;
        self
    }

    /// Sets the exception patterns — a path matching any of them is not
    /// reported even when it matches one of the `deny` patterns.
    #[must_use]
    pub fn with_except(mut self, except: Vec<UsePattern>) -> Self {
        self.except = except;
        self
    }

    /// Returns the exception use patterns.
    #[must_use]
    pub fn except(&self) -> &[UsePattern] {
        &self.except
    }

    /// Returns whether inline qualified paths are checked.
    #[must_use]
    pub fn check_inline(&self) -> bool {
        self.check_inline
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

    /// Returns the denied use patterns.
    #[must_use]
    pub fn deny(&self) -> &[UsePattern] {
        &self.deny
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

    /// Tests whether a use path is denied by this rule.
    ///
    /// A path that matches one of the `except` patterns is never denied.
    #[must_use]
    pub fn is_denied(&self, use_path: &str) -> bool {
        self.deny.iter().any(|p| p.matches(use_path))
            && !self.except.iter().any(|p| p.matches(use_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::ScopeName;

    #[test]
    fn restrict_use_is_denied() {
        let rule = RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![
                UsePattern::new("sqlx::*").unwrap(),
                UsePattern::new("diesel::**").unwrap(),
            ],
            "Domain must be DB-agnostic.".to_string(),
            Some("ARCHITECTURE.md L85".to_string()),
            Severity::Error,
        );
        assert!(rule.is_denied("sqlx::Pool"));
        assert!(rule.is_denied("diesel::connection::PgConnection"));
        assert!(!rule.is_denied("serde::Serialize"));
    }

    #[test]
    fn restrict_use_except_overrides_deny() {
        let rule = RestrictUse::new(
            "no-sqlx-in-domain".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::**").unwrap()],
            "Domain must be DB-agnostic.".to_string(),
            None,
            Severity::Error,
        )
        .with_except(vec![UsePattern::new("sqlx::types::*").unwrap()]);

        assert!(rule.is_denied("sqlx::Pool"));
        assert!(!rule.is_denied("sqlx::types::Json"));
        assert_eq!(rule.except().len(), 1);
    }
}
