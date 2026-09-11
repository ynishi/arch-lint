//! Incremental construction of the aggregate root, with cross-reference
//! validation.
//!
//! The builder reads the rule entities through their public accessors only —
//! their fields stay private to their own modules.

use super::DeclarativeConfig;
use crate::declarative::model::{
    DenyAttr, Layering, ModelError, Naming, RequireAttr, RequireReturn, RequireUse, RestrictUse,
    Scope, ScopeDep, ScopeName, ScopeRef,
};
use std::collections::HashMap;

/// Incremental builder for [`DeclarativeConfig`].
///
/// Cross-reference validation runs once, in [`Self::build`].
#[derive(Debug, Clone, Default)]
pub struct DeclarativeConfigBuilder {
    scopes: Vec<Scope>,
    restrict_uses: Vec<RestrictUse>,
    require_uses: Vec<RequireUse>,
    scope_deps: Vec<ScopeDep>,
    layerings: Vec<Layering>,
    deny_attrs: Vec<DenyAttr>,
    require_attrs: Vec<RequireAttr>,
    namings: Vec<Naming>,
    require_returns: Vec<RequireReturn>,
}

impl DeclarativeConfigBuilder {
    /// Sets the named scopes.
    #[must_use]
    pub fn scopes(mut self, scopes: Vec<Scope>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Sets the `[[restrict-use]]` rules.
    #[must_use]
    pub fn restrict_uses(mut self, rules: Vec<RestrictUse>) -> Self {
        self.restrict_uses = rules;
        self
    }

    /// Sets the `[[require-use]]` rules.
    #[must_use]
    pub fn require_uses(mut self, rules: Vec<RequireUse>) -> Self {
        self.require_uses = rules;
        self
    }

    /// Sets the `[[deny-scope-dep]]` rules.
    #[must_use]
    pub fn scope_deps(mut self, rules: Vec<ScopeDep>) -> Self {
        self.scope_deps = rules;
        self
    }

    /// Sets the `[[layering]]` rules.
    #[must_use]
    pub fn layerings(mut self, rules: Vec<Layering>) -> Self {
        self.layerings = rules;
        self
    }

    /// Sets the `[[deny-attr]]` rules.
    #[must_use]
    pub fn deny_attrs(mut self, rules: Vec<DenyAttr>) -> Self {
        self.deny_attrs = rules;
        self
    }

    /// Sets the `[[require-attr]]` rules.
    #[must_use]
    pub fn require_attrs(mut self, rules: Vec<RequireAttr>) -> Self {
        self.require_attrs = rules;
        self
    }

    /// Sets the `[[naming]]` rules.
    #[must_use]
    pub fn namings(mut self, rules: Vec<Naming>) -> Self {
        self.namings = rules;
        self
    }

    /// Sets the `[[require-return]]` rules.
    #[must_use]
    pub fn require_returns(mut self, rules: Vec<RequireReturn>) -> Self {
        self.require_returns = rules;
        self
    }

    /// Validates all cross-references and builds the config.
    ///
    /// # Errors
    ///
    /// Returns every cross-reference error found (e.g., a named scope ref
    /// that doesn't exist).
    pub fn build(self) -> Result<DeclarativeConfig, Vec<ModelError>> {
        let scope_map: HashMap<ScopeName, Scope> = self
            .scopes
            .into_iter()
            .map(|s| (s.name().clone(), s))
            .collect();
        let mut errors = Vec::new();

        let mut check_ref = |scope: &ScopeRef, context: String| {
            if let ScopeRef::Named(name) = scope {
                if !scope_map.contains_key(name) {
                    errors.push(ModelError::UnknownScope {
                        context,
                        name: name.clone(),
                    });
                }
            }
        };

        for rule in &self.restrict_uses {
            check_ref(rule.scope(), format!("restrict-use '{}'", rule.name()));
        }
        for rule in &self.require_uses {
            check_ref(rule.scope(), format!("require-use '{}'", rule.name()));
        }
        for rule in &self.deny_attrs {
            check_ref(rule.scope(), format!("deny-attr '{}'", rule.name()));
        }
        for rule in &self.require_attrs {
            check_ref(rule.scope(), format!("require-attr '{}'", rule.name()));
        }
        for rule in &self.namings {
            check_ref(rule.scope(), format!("naming '{}'", rule.name()));
        }
        for rule in &self.require_returns {
            check_ref(rule.scope(), format!("require-return '{}'", rule.name()));
        }

        // Validate scope-dep refs
        for dep in &self.scope_deps {
            if !scope_map.contains_key(dep.from_scope()) {
                errors.push(ModelError::UnknownScope {
                    context: "deny-scope-dep.from".to_string(),
                    name: dep.from_scope().clone(),
                });
            }
            for target in dep.denied_targets() {
                if !scope_map.contains_key(target) {
                    errors.push(ModelError::UnknownScope {
                        context: format!("deny-scope-dep.to (from '{}')", dep.from_scope()),
                        name: target.clone(),
                    });
                }
            }
        }

        // Validate layer names
        for layering in &self.layerings {
            for layer in layering.order() {
                if !scope_map.contains_key(layer) {
                    errors.push(ModelError::UnknownScope {
                        context: format!("layering '{}' order", layering.display_name()),
                        name: layer.clone(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(DeclarativeConfig {
                scopes: scope_map,
                restrict_uses: self.restrict_uses,
                require_uses: self.require_uses,
                scope_deps: self.scope_deps,
                layerings: self.layerings,
                deny_attrs: self.deny_attrs,
                require_attrs: self.require_attrs,
                namings: self.namings,
                require_returns: self.require_returns,
            })
        } else {
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::test_support::make_layering;
    use crate::declarative::model::{GlobPattern, ItemKind, RegexPattern, UsePattern};
    use crate::types::Severity;

    #[test]
    fn declarative_config_rejects_unknown_scope_ref() {
        let scopes = vec![]; // No scopes defined
        let restrict = vec![RestrictUse::new(
            "no-sqlx".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "No DB in domain.".to_string(),
            None,
            Severity::Error,
        )];

        let result = DeclarativeConfig::new(scopes, restrict, vec![], vec![]);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ModelError::UnknownScope { .. }));
    }

    #[test]
    fn scope_dep_rejects_unknown_from() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let deps = vec![ScopeDep::new(
            None,
            ScopeName::new("unknown").unwrap(),
            vec![ScopeName::new("domain").unwrap()],
            "msg".to_string(),
            None,
            Severity::Error,
        )];

        let result = DeclarativeConfig::new(scopes, vec![], vec![], deps);
        assert!(result.is_err());
    }

    #[test]
    fn config_rejects_unknown_layer_scope() {
        let result = DeclarativeConfig::builder()
            .scopes(vec![Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            )])
            .layerings(vec![make_layering(false)])
            .build();
        let errors = result.unwrap_err();
        // handler + infra are undefined
        assert_eq!(errors.len(), 2);
        assert!(matches!(&errors[0], ModelError::UnknownScope { .. }));
    }

    #[test]
    fn config_rejects_unknown_scope_for_new_rule_kinds() {
        let result = DeclarativeConfig::builder()
            .namings(vec![Naming::new(
                "domain-ports".to_string(),
                ScopeRef::Named(ScopeName::new("domain").unwrap()),
                ItemKind::Trait,
                RegexPattern::new("^.*Port$").unwrap(),
                "msg".to_string(),
                None,
                Severity::Warning,
            )])
            .build();
        assert!(result.is_err());
    }
}
