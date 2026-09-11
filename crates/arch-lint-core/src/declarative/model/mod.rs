//! Pure domain model for declarative architecture rules.
//!
//! This module contains no serde, no syn, no I/O dependencies.
//! All invariants are enforced at construction time via validated newtypes.
//!
//! # Layout
//!
//! - Validated newtypes shared by every rule kind ([`ScopeName`],
//!   [`GlobPattern`], [`UsePattern`], [`RegexPattern`], [`AttrPattern`]) and
//!   the item selectors ([`ItemKind`], [`VisibilityFilter`]).
//! - [`Scope`] / [`ScopeRef`] — named file scopes and references to them.
//! - One entity per rule kind: [`RestrictUse`] (ALD001), [`RequireUse`]
//!   (ALD002), [`ScopeDep`] (ALD003), [`Layering`] (ALD004), [`DenyAttr`]
//!   (ALD005), [`RequireAttr`] (ALD006), [`Naming`] (ALD007) and
//!   [`RequireReturn`] (ALD008).
//! - [`DeclarativeConfig`] / [`DeclarativeConfigBuilder`] — the aggregate
//!   root, which validates cross-references between rules and scopes.
//!
//! Rule entities keep their fields private; the aggregate root and the rule
//! implementations read them through the entities' accessors.

mod attr;
mod config;
mod item;
mod layering;
mod naming;
mod require_return;
mod require_use;
mod restrict_use;
mod scope;
mod scope_dep;
mod vocab;

pub use attr::{DenyAttr, RequireAttr};
pub use config::{DeclarativeConfig, DeclarativeConfigBuilder};
pub use item::{ItemKind, VisibilityFilter};
pub use layering::Layering;
pub use naming::Naming;
pub use require_return::RequireReturn;
pub use require_use::RequireUse;
pub use restrict_use::RestrictUse;
pub use scope::{Scope, ScopeRef};
pub use scope_dep::ScopeDep;
pub use vocab::{AttrPattern, GlobPattern, RegexPattern, ScopeName, UsePattern};

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

    /// Regex pattern is empty.
    #[error("regex pattern must not be empty")]
    EmptyRegexPattern,

    /// Regex pattern does not compile.
    #[error("invalid regex `{pattern}`: {reason}")]
    InvalidRegexPattern {
        /// The invalid pattern.
        pattern: String,
        /// Why it's invalid.
        reason: String,
    },

    /// Attribute pattern is empty.
    #[error("attribute pattern must not be empty")]
    EmptyAttrPattern,

    /// Attribute pattern is malformed.
    #[error("invalid attribute pattern `{pattern}`: {reason}")]
    InvalidAttrPattern {
        /// The invalid pattern.
        pattern: String,
        /// Why it's invalid.
        reason: String,
    },

    /// An item kind is not one of the supported values.
    #[error("unknown item kind `{value}`, expected: {allowed}")]
    InvalidItemKind {
        /// The invalid value.
        value: String,
        /// The accepted values, comma separated.
        allowed: String,
    },

    /// A visibility filter is neither `pub` nor `any`.
    #[error("unknown visibility `{value}`, expected: pub, any")]
    InvalidVisibility {
        /// The invalid value.
        value: String,
    },

    /// A layering has fewer than two layers.
    #[error("layering `order` needs at least 2 scopes, got {count}")]
    LayerOrderTooShort {
        /// How many layers were given.
        count: usize,
    },

    /// A layering repeats the same scope.
    #[error("layering `order` lists scope `{name}` more than once")]
    DuplicateLayer {
        /// The repeated scope name.
        name: ScopeName,
    },
}

/// Fixtures shared by the tests of more than one submodule.
#[cfg(test)]
pub(super) mod test_support {
    use super::{Layering, ScopeName};
    use crate::types::Severity;

    /// A three-layer `handler → domain → infra` layering named `clean-arch`.
    pub(crate) fn make_layering(adjacent_only: bool) -> Layering {
        Layering::new(
            Some("clean-arch".to_string()),
            vec![
                ScopeName::new("handler").unwrap(),
                ScopeName::new("domain").unwrap(),
                ScopeName::new("infra").unwrap(),
            ],
            "Layers may only depend downward.".to_string(),
            None,
            Severity::Error,
        )
        .unwrap()
        .with_adjacent_only(adjacent_only)
    }
}
