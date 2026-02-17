//! Pure domain model for declarative architecture rules.
//!
//! This module contains no serde, no syn, no I/O dependencies.
//! All invariants are enforced at construction time via validated newtypes.

use crate::types::Severity;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

// ────────────────────────────────────────────
// Newtypes with validation
// ────────────────────────────────────────────

/// A validated scope name (non-empty, `[a-z0-9-]` only).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeName(String);

impl ScopeName {
    /// Creates a new scope name.
    ///
    /// # Errors
    ///
    /// Returns error if the name is empty or contains invalid characters.
    pub fn new(name: &str) -> Result<Self, ModelError> {
        if name.is_empty() {
            return Err(ModelError::EmptyScopeName);
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ModelError::InvalidScopeName {
                name: name.to_string(),
            });
        }
        Ok(Self(name.to_string()))
    }

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A validated glob pattern for file path matching.
///
/// The glob is compiled once at construction and reused for all match calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern {
    raw: String,
    compiled: glob::Pattern,
}

impl GlobPattern {
    /// Creates a new glob pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or has invalid glob syntax.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyGlobPattern);
        }
        let compiled = glob::Pattern::new(pattern).map_err(|e| ModelError::InvalidGlobPattern {
            pattern: pattern.to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            raw: pattern.to_string(),
            compiled,
        })
    }

    /// Tests whether a relative file path matches this pattern.
    #[must_use]
    pub fn matches(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        if self.compiled.matches(&path_str) {
            return true;
        }
        // Fallback: `glob::Pattern` treats `**` and `*` equivalently by default
        // (both match `/`). For `dir/**` patterns, also check prefix + boundary
        // to handle edge cases where the glob crate doesn't match as expected.
        if let Some(prefix) = self.raw.strip_suffix("/**") {
            let normalized = prefix.trim_end_matches('/');
            if path_str.starts_with(normalized)
                && path_str
                    .as_bytes()
                    .get(normalized.len())
                    .is_some_and(|&b| b == b'/')
            {
                return true;
            }
        }
        false
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// A validated use-path pattern for matching Rust import paths.
///
/// Supports `::` separated segments with `*` (one segment) and `**` (any segments).
/// Examples: `sqlx::*`, `std::fs::**`, `tokio::fs::read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsePattern(String);

impl UsePattern {
    /// Creates a new use-path pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyUsePattern);
        }
        Ok(Self(pattern.to_string()))
    }

    /// Tests whether a Rust use path matches this pattern.
    ///
    /// Uses `::` separated segment matching with `*` and `**` wildcards.
    #[must_use]
    pub fn matches(&self, use_path: &str) -> bool {
        crate::utils::paths::path_matches(use_path, &self.0)
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ────────────────────────────────────────────
// Domain entities
// ────────────────────────────────────────────

/// A named file scope — a set of files matched by glob patterns.
#[derive(Debug, Clone)]
pub struct Scope {
    name: ScopeName,
    patterns: Vec<GlobPattern>,
}

impl Scope {
    /// Creates a new scope.
    #[must_use]
    pub fn new(name: ScopeName, patterns: Vec<GlobPattern>) -> Self {
        Self { name, patterns }
    }

    /// Returns the scope name.
    #[must_use]
    pub fn name(&self) -> &ScopeName {
        &self.name
    }

    /// Returns the glob patterns.
    #[must_use]
    pub fn patterns(&self) -> &[GlobPattern] {
        &self.patterns
    }

    /// Tests whether a relative file path belongs to this scope.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
    }
}

/// Reference to a scope — either by name or inline patterns.
#[derive(Debug, Clone)]
pub enum ScopeRef {
    /// Reference to a named scope defined in `[[scopes]]`.
    Named(ScopeName),
    /// Inline file patterns (no named scope required).
    Inline(Vec<GlobPattern>),
}

impl ScopeRef {
    /// Tests whether a relative file path matches this scope reference.
    ///
    /// For `Named`, requires the scope registry to resolve. Use
    /// [`DeclarativeConfig::scope_contains`] instead.
    /// For `Inline`, matches directly against the patterns.
    #[must_use]
    pub fn matches_inline(&self, path: &Path) -> bool {
        match self {
            Self::Named(_) => false, // Must resolve via DeclarativeConfig
            Self::Inline(patterns) => patterns.iter().any(|p| p.matches(path)),
        }
    }
}

/// A use-restriction rule: deny certain imports within a scope.
#[derive(Debug, Clone)]
pub struct RestrictUse {
    name: String,
    scope: ScopeRef,
    deny: Vec<UsePattern>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RestrictUse {
    /// Creates a new restrict-use rule.
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
    #[must_use]
    pub fn is_denied(&self, use_path: &str) -> bool {
        self.deny.iter().any(|p| p.matches(use_path))
    }
}

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

/// A scope dependency constraint: deny imports from one scope to others.
#[derive(Debug, Clone)]
pub struct ScopeDep {
    from: ScopeName,
    to: Vec<ScopeName>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl ScopeDep {
    /// Creates a new scope dependency rule.
    #[must_use]
    pub fn new(
        from: ScopeName,
        to: Vec<ScopeName>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            from,
            to,
            message,
            doc_ref,
            severity,
        }
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

// ────────────────────────────────────────────
// Aggregate root
// ────────────────────────────────────────────

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
}

impl DeclarativeConfig {
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
        let scope_map: HashMap<ScopeName, Scope> =
            scopes.into_iter().map(|s| (s.name.clone(), s)).collect();
        let mut errors = Vec::new();

        // Validate restrict-use scope refs
        for rule in &restrict_uses {
            if let ScopeRef::Named(ref name) = rule.scope {
                if !scope_map.contains_key(name) {
                    errors.push(ModelError::UnknownScope {
                        context: format!("restrict-use '{}'", rule.name),
                        name: name.clone(),
                    });
                }
            }
        }

        // Validate require-use scope refs
        for rule in &require_uses {
            if let ScopeRef::Named(ref name) = rule.scope {
                if !scope_map.contains_key(name) {
                    errors.push(ModelError::UnknownScope {
                        context: format!("require-use '{}'", rule.name),
                        name: name.clone(),
                    });
                }
            }
        }

        // Validate scope-dep refs
        for dep in &scope_deps {
            if !scope_map.contains_key(&dep.from) {
                errors.push(ModelError::UnknownScope {
                    context: "deny-scope-dep.from".to_string(),
                    name: dep.from.clone(),
                });
            }
            for target in &dep.to {
                if !scope_map.contains_key(target) {
                    errors.push(ModelError::UnknownScope {
                        context: format!("deny-scope-dep.to (from '{}')", dep.from),
                        name: target.clone(),
                    });
                }
            }
        }

        if errors.is_empty() {
            Ok(Self {
                scopes: scope_map,
                restrict_uses,
                require_uses,
                scope_deps,
            })
        } else {
            Err(errors)
        }
    }

    /// Creates an empty declarative config (no declarative rules).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            scopes: HashMap::new(),
            restrict_uses: Vec::new(),
            require_uses: Vec::new(),
            scope_deps: Vec::new(),
        }
    }

    /// Returns true if no declarative rules are defined.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.restrict_uses.is_empty() && self.require_uses.is_empty() && self.scope_deps.is_empty()
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

// ────────────────────────────────────────────
// Errors
// ────────────────────────────────────────────

/// Errors in domain model construction.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ModelError {
    /// Scope name is empty.
    #[error("scope name must not be empty")]
    EmptyScopeName,

    /// Scope name contains invalid characters.
    #[error("invalid scope name `{name}`: must be [a-z0-9-]")]
    InvalidScopeName {
        /// The invalid name.
        name: String,
    },

    /// Glob pattern is empty.
    #[error("glob pattern must not be empty")]
    EmptyGlobPattern,

    /// Glob pattern has invalid syntax.
    #[error("invalid glob pattern `{pattern}`: {reason}")]
    InvalidGlobPattern {
        /// The invalid pattern.
        pattern: String,
        /// Why it's invalid.
        reason: String,
    },

    /// Use pattern is empty.
    #[error("use pattern must not be empty")]
    EmptyUsePattern,

    /// A scope reference points to an undefined scope.
    #[error("{context}: unknown scope `{name}`")]
    UnknownScope {
        /// Where the reference was found.
        context: String,
        /// The undefined scope name.
        name: ScopeName,
    },
}

// ────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- ScopeName --

    #[test]
    fn scope_name_valid() {
        assert!(ScopeName::new("domain").is_ok());
        assert!(ScopeName::new("my-scope-123").is_ok());
    }

    #[test]
    fn scope_name_empty_rejected() {
        assert!(matches!(
            ScopeName::new(""),
            Err(ModelError::EmptyScopeName)
        ));
    }

    #[test]
    fn scope_name_invalid_chars_rejected() {
        assert!(matches!(
            ScopeName::new("Domain"),
            Err(ModelError::InvalidScopeName { .. })
        ));
        assert!(matches!(
            ScopeName::new("my_scope"),
            Err(ModelError::InvalidScopeName { .. })
        ));
    }

    // -- GlobPattern --

    #[test]
    fn glob_pattern_valid() {
        assert!(GlobPattern::new("src/domain/**").is_ok());
        assert!(GlobPattern::new("src/**/*.rs").is_ok());
    }

    #[test]
    fn glob_pattern_empty_rejected() {
        assert!(matches!(
            GlobPattern::new(""),
            Err(ModelError::EmptyGlobPattern)
        ));
    }

    #[test]
    fn glob_pattern_matches_file() {
        let pat = GlobPattern::new("src/domain/**").unwrap();
        assert!(pat.matches(Path::new("src/domain/entity.rs")));
        assert!(pat.matches(Path::new("src/domain/sub/deep.rs")));
        assert!(!pat.matches(Path::new("src/infra/db.rs")));
    }

    // -- UsePattern --

    #[test]
    fn use_pattern_valid() {
        assert!(UsePattern::new("sqlx::*").is_ok());
        assert!(UsePattern::new("std::fs::**").is_ok());
    }

    #[test]
    fn use_pattern_empty_rejected() {
        assert!(matches!(
            UsePattern::new(""),
            Err(ModelError::EmptyUsePattern)
        ));
    }

    #[test]
    fn use_pattern_matches() {
        let pat = UsePattern::new("sqlx::*").unwrap();
        assert!(pat.matches("sqlx::Pool"));
        assert!(pat.matches("sqlx::query"));
        assert!(!pat.matches("sqlx::pool::Pool")); // * = one segment only
        assert!(!pat.matches("diesel::Pool"));
    }

    #[test]
    fn use_pattern_globstar_matches() {
        let pat = UsePattern::new("std::fs::**").unwrap();
        assert!(pat.matches("std::fs::read"));
        assert!(pat.matches("std::fs::read_to_string"));
        assert!(pat.matches("std::fs"));
        assert!(!pat.matches("std::io::read"));
    }

    // -- Scope --

    #[test]
    fn scope_contains_file() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![
                GlobPattern::new("src/domain/**").unwrap(),
                GlobPattern::new("src/core/**").unwrap(),
            ],
        );
        assert!(scope.contains(Path::new("src/domain/entity.rs")));
        assert!(scope.contains(Path::new("src/core/types.rs")));
        assert!(!scope.contains(Path::new("src/infra/db.rs")));
    }

    // -- RestrictUse --

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

    // -- ScopeDep --

    #[test]
    fn scope_dep_is_denied() {
        let dep = ScopeDep::new(
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

    // -- DeclarativeConfig (aggregate root validation) --

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

    #[test]
    fn scope_dep_rejects_unknown_from() {
        let scopes = vec![Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/domain/**").unwrap()],
        )];
        let deps = vec![ScopeDep::new(
            ScopeName::new("unknown").unwrap(),
            vec![ScopeName::new("domain").unwrap()],
            "msg".to_string(),
            None,
            Severity::Error,
        )];

        let result = DeclarativeConfig::new(scopes, vec![], vec![], deps);
        assert!(result.is_err());
    }
}
