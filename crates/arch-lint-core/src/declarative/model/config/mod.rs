//! The aggregate root: [`DeclarativeConfig`] and its builder.
//!
//! Every rule entity is reached through here, and every cross-reference
//! (a `[[scopes]]` name referenced by a rule) is validated once, in
//! [`DeclarativeConfigBuilder::build`].

mod builder;

pub use builder::DeclarativeConfigBuilder;

use super::{
    Acyclic, DenyAttr, Independent, Layering, ModelError, Naming, RequireAttr, RequireReturn,
    RequireUse, RestrictUse, Scope, ScopeDep, ScopeName, ScopeRef,
};
use std::collections::HashMap;
use std::path::Path;

/// Validated declarative configuration.
///
/// All cross-references are verified at construction time.
/// This is the aggregate root — all queries go through here.
#[derive(Debug, Clone)]
pub struct DeclarativeConfig {
    scopes: HashMap<ScopeName, Scope>,
    restrict_uses: Vec<RestrictUse>,
    require_uses: Vec<RequireUse>,
    scope_deps: Vec<ScopeDep>,
    layerings: Vec<Layering>,
    deny_attrs: Vec<DenyAttr>,
    require_attrs: Vec<RequireAttr>,
    namings: Vec<Naming>,
    require_returns: Vec<RequireReturn>,
    acyclics: Vec<Acyclic>,
    independents: Vec<Independent>,
}

impl DeclarativeConfig {
    /// Returns a builder for configs that use the newer rule kinds.
    #[must_use]
    pub fn builder() -> DeclarativeConfigBuilder {
        DeclarativeConfigBuilder::default()
    }

    /// Creates a new declarative config with full validation.
    ///
    /// # Errors
    ///
    /// Returns errors if any cross-references are invalid
    /// (e.g., named scope ref that doesn't exist).
    pub fn new(
        scopes: Vec<Scope>,
        restrict_uses: Vec<RestrictUse>,
        require_uses: Vec<RequireUse>,
        scope_deps: Vec<ScopeDep>,
    ) -> Result<Self, Vec<ModelError>> {
        Self::builder()
            .scopes(scopes)
            .restrict_uses(restrict_uses)
            .require_uses(require_uses)
            .scope_deps(scope_deps)
            .build()
    }

    /// Creates an empty declarative config (no declarative rules).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            scopes: HashMap::new(),
            restrict_uses: Vec::new(),
            require_uses: Vec::new(),
            scope_deps: Vec::new(),
            layerings: Vec::new(),
            deny_attrs: Vec::new(),
            require_attrs: Vec::new(),
            namings: Vec::new(),
            require_returns: Vec::new(),
            acyclics: Vec::new(),
            independents: Vec::new(),
        }
    }

    /// Returns true if no declarative rules are defined.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.restrict_uses.is_empty()
            && self.require_uses.is_empty()
            && self.scope_deps.is_empty()
            && self.layerings.is_empty()
            && self.deny_attrs.is_empty()
            && self.require_attrs.is_empty()
            && self.namings.is_empty()
            && self.require_returns.is_empty()
            && self.acyclics.is_empty()
            && self.independents.is_empty()
    }

    /// Returns all `[[layering]]` rules.
    #[must_use]
    pub fn layerings(&self) -> &[Layering] {
        &self.layerings
    }

    /// Returns all `[[deny-attr]]` rules.
    #[must_use]
    pub fn deny_attrs(&self) -> &[DenyAttr] {
        &self.deny_attrs
    }

    /// Returns all `[[require-attr]]` rules.
    #[must_use]
    pub fn require_attrs(&self) -> &[RequireAttr] {
        &self.require_attrs
    }

    /// Returns all `[[naming]]` rules.
    #[must_use]
    pub fn namings(&self) -> &[Naming] {
        &self.namings
    }

    /// Returns all `[[require-return]]` rules.
    #[must_use]
    pub fn require_returns(&self) -> &[RequireReturn] {
        &self.require_returns
    }

    /// Returns all `[[acyclic]]` rules.
    #[must_use]
    pub fn acyclics(&self) -> &[Acyclic] {
        &self.acyclics
    }

    /// Returns all `[[independent]]` rules.
    #[must_use]
    pub fn independents(&self) -> &[Independent] {
        &self.independents
    }

    /// Returns all defined scopes.
    #[must_use]
    pub fn scopes(&self) -> &HashMap<ScopeName, Scope> {
        &self.scopes
    }

    /// Returns all restrict-use rules.
    #[must_use]
    pub fn restrict_uses(&self) -> &[RestrictUse] {
        &self.restrict_uses
    }

    /// Returns all require-use rules.
    #[must_use]
    pub fn require_uses(&self) -> &[RequireUse] {
        &self.require_uses
    }

    /// Returns all scope dependency rules.
    #[must_use]
    pub fn scope_deps(&self) -> &[ScopeDep] {
        &self.scope_deps
    }

    /// Gets a scope by name.
    #[must_use]
    pub fn scope(&self, name: &ScopeName) -> Option<&Scope> {
        self.scopes.get(name)
    }

    /// Resolves which scopes a file path belongs to.
    #[must_use]
    pub fn scopes_for_path(&self, path: &Path) -> Vec<&ScopeName> {
        self.scopes
            .values()
            .filter(|s| s.contains(path))
            .map(Scope::name)
            .collect()
    }

    /// Tests whether a file path is within a scope reference.
    #[must_use]
    pub fn scope_ref_contains(&self, scope_ref: &ScopeRef, path: &Path) -> bool {
        match scope_ref {
            ScopeRef::Named(name) => self
                .scopes
                .get(name)
                .is_some_and(|scope| scope.contains(path)),
            ScopeRef::Inline(patterns) => patterns.iter().any(|p| p.matches(path)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::{GlobPattern, RegexPattern, UsePattern};
    use crate::types::Severity;

    #[test]
    fn declarative_config_valid() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let restrict = vec![RestrictUse::new(
            "no-sqlx".to_string(),
            ScopeRef::Named(ScopeName::new("domain").unwrap()),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "No DB in domain.".to_string(),
            None,
            Severity::Error,
        )];

        let config = DeclarativeConfig::new(scopes, restrict, vec![], vec![]);
        assert!(config.is_ok());
    }

    #[test]
    fn declarative_config_inline_scope_needs_no_registration() {
        let restrict = vec![RestrictUse::new(
            "no-sqlx".to_string(),
            ScopeRef::Inline(vec![GlobPattern::new("src/domain/**").unwrap()]),
            vec![UsePattern::new("sqlx::*").unwrap()],
            "No DB in domain.".to_string(),
            None,
            Severity::Error,
        )];

        let config = DeclarativeConfig::new(vec![], restrict, vec![], vec![]);
        assert!(config.is_ok());
    }

    #[test]
    fn declarative_config_scopes_for_path() {
        let config = DeclarativeConfig::new(
            vec![
                Scope::new(
                    ScopeName::new("domain").unwrap(),
                    vec![GlobPattern::new("src/domain/**").unwrap()],
                ),
                Scope::new(
                    ScopeName::new("infra").unwrap(),
                    vec![GlobPattern::new("src/infra/**").unwrap()],
                ),
            ],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();

        let scopes = config.scopes_for_path(Path::new("src/domain/entity.rs"));
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].as_str(), "domain");

        let scopes = config.scopes_for_path(Path::new("src/handlers/api.rs"));
        assert!(scopes.is_empty());
    }

    #[test]
    fn declarative_config_scope_ref_contains() {
        let config = DeclarativeConfig::new(
            vec![Scope::new(
                ScopeName::new("domain").unwrap(),
                vec![GlobPattern::new("src/domain/**").unwrap()],
            )],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();

        let named_ref = ScopeRef::Named(ScopeName::new("domain").unwrap());
        assert!(config.scope_ref_contains(&named_ref, Path::new("src/domain/entity.rs")));
        assert!(!config.scope_ref_contains(&named_ref, Path::new("src/infra/db.rs")));

        let inline_ref = ScopeRef::Inline(vec![GlobPattern::new("src/handlers/**").unwrap()]);
        assert!(config.scope_ref_contains(&inline_ref, Path::new("src/handlers/api.rs")));
        assert!(!config.scope_ref_contains(&inline_ref, Path::new("src/domain/entity.rs")));
    }

    #[test]
    fn empty_config() {
        let config = DeclarativeConfig::empty();
        assert!(config.is_empty());
        assert!(config.scopes().is_empty());
    }

    // -- Overlapping scopes --

    #[test]
    fn overlapping_scopes_file_matches_both() {
        let config = DeclarativeConfig::new(
            vec![
                Scope::new(
                    ScopeName::new("domain").unwrap(),
                    vec![GlobPattern::new("src/domain/**").unwrap()],
                ),
                Scope::new(
                    ScopeName::new("core").unwrap(),
                    vec![
                        GlobPattern::new("src/domain/**").unwrap(),
                        GlobPattern::new("src/core/**").unwrap(),
                    ],
                ),
            ],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();

        let scopes = config.scopes_for_path(Path::new("src/domain/entity.rs"));
        assert_eq!(scopes.len(), 2);
        let names: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();
        assert!(names.contains(&"domain"));
        assert!(names.contains(&"core"));
    }

    #[test]
    fn overlapping_scopes_rules_from_both_applied() {
        // A file in overlapping scopes should be subject to rules from ALL matching scopes
        let config = DeclarativeConfig::new(
            vec![
                Scope::new(
                    ScopeName::new("shared").unwrap(),
                    vec![GlobPattern::new("src/**").unwrap()],
                ),
                Scope::new(
                    ScopeName::new("domain").unwrap(),
                    vec![GlobPattern::new("src/domain/**").unwrap()],
                ),
            ],
            vec![
                RestrictUse::new(
                    "no-sqlx-in-shared".to_string(),
                    ScopeRef::Named(ScopeName::new("shared").unwrap()),
                    vec![UsePattern::new("sqlx::*").unwrap()],
                    "Shared scope denies sqlx.".to_string(),
                    None,
                    Severity::Warning,
                ),
                RestrictUse::new(
                    "no-diesel-in-domain".to_string(),
                    ScopeRef::Named(ScopeName::new("domain").unwrap()),
                    vec![UsePattern::new("diesel::*").unwrap()],
                    "Domain denies diesel.".to_string(),
                    None,
                    Severity::Error,
                ),
            ],
            vec![],
            vec![],
        )
        .unwrap();

        // A file in src/domain/ belongs to both "shared" and "domain"
        let path = Path::new("src/domain/service.rs");
        assert!(
            config.scope_ref_contains(&ScopeRef::Named(ScopeName::new("shared").unwrap()), path,)
        );
        assert!(
            config.scope_ref_contains(&ScopeRef::Named(ScopeName::new("domain").unwrap()), path,)
        );

        // Both restrict-use rules apply to this path
        let applicable: Vec<&str> = config
            .restrict_uses()
            .iter()
            .filter(|r| config.scope_ref_contains(r.scope(), path))
            .map(RestrictUse::name)
            .collect();
        assert_eq!(applicable.len(), 2);
        assert!(applicable.contains(&"no-sqlx-in-shared"));
        assert!(applicable.contains(&"no-diesel-in-domain"));
    }

    #[test]
    fn config_is_empty_covers_new_rule_kinds() {
        let config = DeclarativeConfig::builder()
            .scopes(vec![Scope::new(
                ScopeName::new("handler").unwrap(),
                vec![GlobPattern::new("src/handler/**").unwrap()],
            )])
            .require_returns(vec![RequireReturn::new(
                "handlers-return-app-result".to_string(),
                ScopeRef::Named(ScopeName::new("handler").unwrap()),
                RegexPattern::new("^Result<").unwrap(),
                "msg".to_string(),
                None,
                Severity::Error,
            )
            .with_asyncness(Some(true))])
            .build()
            .unwrap();
        assert!(!config.is_empty());
        assert_eq!(config.require_returns().len(), 1);
    }

    #[test]
    fn config_is_empty_covers_acyclic() {
        use crate::declarative::model::Acyclic;

        let config = DeclarativeConfig::builder()
            .scopes(vec![
                Scope::new(
                    ScopeName::new("domain").unwrap(),
                    vec![GlobPattern::new("src/domain/**").unwrap()],
                ),
                Scope::new(
                    ScopeName::new("infra").unwrap(),
                    vec![GlobPattern::new("src/infra/**").unwrap()],
                ),
            ])
            .acyclics(vec![Acyclic::new(None, None, None, Severity::Error)])
            .build()
            .unwrap();
        assert!(!config.is_empty());
        assert_eq!(config.acyclics().len(), 1);
    }

    #[test]
    fn config_is_empty_covers_independent() {
        use crate::declarative::model::Independent;

        let config = DeclarativeConfig::builder()
            .scopes(vec![
                Scope::new(
                    ScopeName::new("billing").unwrap(),
                    vec![GlobPattern::new("src/billing/**").unwrap()],
                ),
                Scope::new(
                    ScopeName::new("shipping").unwrap(),
                    vec![GlobPattern::new("src/shipping/**").unwrap()],
                ),
            ])
            .independents(vec![Independent::new(
                None,
                vec![
                    ScopeName::new("billing").unwrap(),
                    ScopeName::new("shipping").unwrap(),
                ],
                None,
                Severity::Error,
            )
            .unwrap()])
            .build()
            .unwrap();
        assert!(!config.is_empty());
        assert_eq!(config.independents().len(), 1);
    }
}
