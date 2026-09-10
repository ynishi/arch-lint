//! The `[[require-use]]` rule entity (ALD002).

use super::ScopeRef;
use crate::types::Severity;

/// A use-requirement rule: prefer one crate over alternatives.
#[derive(Debug, Clone)]
pub struct RequireUse {
    name: String,
    scope: ScopeRef,
    prefer: String,
    over: Vec<String>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RequireUse {
    /// Creates a new require-use rule.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        prefer: String,
        over: Vec<String>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            prefer,
            over,
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

    /// Returns the preferred crate name.
    #[must_use]
    pub fn prefer(&self) -> &str {
        &self.prefer
    }

    /// Returns the alternative (discouraged) crate names.
    #[must_use]
    pub fn over(&self) -> &[String] {
        &self.over
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
