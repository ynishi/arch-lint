//! The `[[deny-scope-dep]]` rule entity (ALD003).

use super::ScopeName;
use crate::types::Severity;

/// A scope dependency constraint: deny imports from one scope to others.
#[derive(Debug, Clone)]
pub struct ScopeDep {
    name: Option<String>,
    from: ScopeName,
    to: Vec<ScopeName>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
    check_inline: bool,
}

impl ScopeDep {
    /// Creates a new scope dependency rule.
    ///
    /// Inline qualified-path checking is enabled by default;
    /// use [`Self::with_check_inline`] to disable it.
    #[must_use]
    pub fn new(
        name: Option<String>,
        from: ScopeName,
        to: Vec<ScopeName>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            from,
            to,
            message,
            doc_ref,
            severity,
            check_inline: true,
        }
    }

    /// Sets whether inline qualified paths (e.g. `crate::infra::Db::new()`
    /// outside a `use` statement) are also checked.
    #[must_use]
    pub fn with_check_inline(mut self, check_inline: bool) -> Self {
        self.check_inline = check_inline;
        self
    }

    /// Returns whether inline qualified paths are checked.
    #[must_use]
    pub fn check_inline(&self) -> bool {
        self.check_inline
    }

    /// Returns the rule name, or a generated fallback (`"deny-scope-dep:{from}"`).
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("deny-scope-dep:{}", self.from))
    }

    /// Returns the source scope.
    #[must_use]
    pub fn from_scope(&self) -> &ScopeName {
        &self.from
    }

    /// Returns the denied target scopes.
    #[must_use]
    pub fn denied_targets(&self) -> &[ScopeName] {
        &self.to
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

    /// Checks if a target scope is denied.
    #[must_use]
    pub fn is_denied(&self, target: &ScopeName) -> bool {
        self.to.contains(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_dep_is_denied() {
        let dep = ScopeDep::new(
            Some("no-domain-to-infra".to_string()),
            ScopeName::new("domain").unwrap(),
            vec![
                ScopeName::new("infrastructure").unwrap(),
                ScopeName::new("presentation").unwrap(),
            ],
            "Domain must not depend on infra.".to_string(),
            None,
            Severity::Error,
        );
        assert!(dep.is_denied(&ScopeName::new("infrastructure").unwrap()));
        assert!(!dep.is_denied(&ScopeName::new("application").unwrap()));
    }

    #[test]
    fn scope_dep_display_name_with_explicit_name() {
        let dep = ScopeDep::new(
            Some("no-domain-to-infra".to_string()),
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "msg".to_string(),
            None,
            Severity::Error,
        );
        assert_eq!(dep.display_name(), "no-domain-to-infra");
    }

    #[test]
    fn scope_dep_display_name_fallback() {
        let dep = ScopeDep::new(
            None,
            ScopeName::new("domain").unwrap(),
            vec![ScopeName::new("infra").unwrap()],
            "msg".to_string(),
            None,
            Severity::Error,
        );
        assert_eq!(dep.display_name(), "deny-scope-dep:domain");
    }
}
