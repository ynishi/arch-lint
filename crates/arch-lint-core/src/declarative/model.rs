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

/// A validated regular expression, used by the naming and return-type rules.
///
/// The regex is compiled once at construction and reused for all match calls.
#[derive(Debug, Clone)]
pub struct RegexPattern {
    raw: String,
    compiled: regex::Regex,
}

impl RegexPattern {
    /// Creates a new regex pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or is not a valid regex.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        if pattern.is_empty() {
            return Err(ModelError::EmptyRegexPattern);
        }
        let compiled = regex::Regex::new(pattern).map_err(|e| ModelError::InvalidRegexPattern {
            pattern: pattern.to_string(),
            reason: e.to_string(),
        })?;
        Ok(Self {
            raw: pattern.to_string(),
            compiled,
        })
    }

    /// Tests whether the regex matches anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.compiled.is_match(text)
    }

    /// Returns the pattern as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

/// The kind of Rust item targeted by the attribute / naming / return rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// `struct Foo { .. }`
    Struct,
    /// `enum Foo { .. }`
    Enum,
    /// `fn foo() { .. }` (free functions, impl methods, trait methods)
    Fn,
    /// `trait Foo { .. }`
    Trait,
    /// `impl Foo { .. }` / `impl Bar for Foo { .. }`
    Impl,
    /// `mod foo { .. }`
    Mod,
    /// `type Foo = ..;`
    Type,
    /// `const FOO: T = ..;`
    Const,
    /// `static FOO: T = ..;`
    Static,
    /// `union Foo { .. }`
    Union,
}

impl ItemKind {
    /// Every item kind, in the order used as the default for `on`.
    pub const ALL: [Self; 10] = [
        Self::Struct,
        Self::Enum,
        Self::Fn,
        Self::Trait,
        Self::Impl,
        Self::Mod,
        Self::Type,
        Self::Const,
        Self::Static,
        Self::Union,
    ];

    /// Item kinds that carry an identifier, usable as a `[[naming]]` target.
    pub const NAMED: [Self; 8] = [
        Self::Struct,
        Self::Enum,
        Self::Trait,
        Self::Fn,
        Self::Mod,
        Self::Type,
        Self::Const,
        Self::Static,
    ];

    /// Parses an item kind from its TOML spelling.
    ///
    /// # Errors
    ///
    /// Returns error if the value is not one of [`ItemKind::ALL`].
    pub fn parse(value: &str) -> Result<Self, ModelError> {
        Self::parse_within(value, &Self::ALL)
    }

    /// Parses an item kind restricted to kinds that carry an identifier.
    ///
    /// # Errors
    ///
    /// Returns error if the value is not one of [`ItemKind::NAMED`].
    pub fn parse_named(value: &str) -> Result<Self, ModelError> {
        Self::parse_within(value, &Self::NAMED)
    }

    fn parse_within(value: &str, allowed: &[Self]) -> Result<Self, ModelError> {
        allowed
            .iter()
            .copied()
            .find(|k| k.as_str() == value)
            .ok_or_else(|| ModelError::InvalidItemKind {
                value: value.to_string(),
                allowed: allowed
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }

    /// Returns the TOML spelling of this item kind.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Fn => "fn",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Mod => "mod",
            Self::Type => "type",
            Self::Const => "const",
            Self::Static => "static",
            Self::Union => "union",
        }
    }
}

impl fmt::Display for ItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Which items a rule applies to, by visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityFilter {
    /// Only `pub` items.
    Pub,
    /// Every item, regardless of visibility.
    Any,
}

impl VisibilityFilter {
    /// Parses a visibility filter from its TOML spelling (`pub` / `any`).
    ///
    /// # Errors
    ///
    /// Returns error if the value is neither `pub` nor `any`.
    pub fn parse(value: &str) -> Result<Self, ModelError> {
        match value {
            "pub" => Ok(Self::Pub),
            "any" => Ok(Self::Any),
            _ => Err(ModelError::InvalidVisibility {
                value: value.to_string(),
            }),
        }
    }

    /// Tests whether an item with the given visibility is selected.
    #[must_use]
    pub fn matches(self, is_pub: bool) -> bool {
        match self {
            Self::Pub => is_pub,
            Self::Any => true,
        }
    }
}

/// A named file scope — a set of files matched by glob patterns.
#[derive(Debug, Clone)]
pub struct Scope {
    name: ScopeName,
    patterns: Vec<GlobPattern>,
    excludes: Vec<GlobPattern>,
}

impl Scope {
    /// Creates a new scope with no exclusions.
    ///
    /// Use [`Self::with_excludes`] to subtract files from the scope.
    #[must_use]
    pub fn new(name: ScopeName, patterns: Vec<GlobPattern>) -> Self {
        Self {
            name,
            patterns,
            excludes: Vec::new(),
        }
    }

    /// Sets the exclusion patterns — files matching any of these are not in
    /// the scope even when they match one of the `paths` patterns.
    #[must_use]
    pub fn with_excludes(mut self, excludes: Vec<GlobPattern>) -> Self {
        self.excludes = excludes;
        self
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

    /// Returns the exclusion glob patterns.
    #[must_use]
    pub fn excludes(&self) -> &[GlobPattern] {
        &self.excludes
    }

    /// Tests whether a relative file path belongs to this scope.
    ///
    /// A path belongs to the scope when it matches at least one `paths`
    /// pattern and no `exclude` pattern.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.patterns.iter().any(|p| p.matches(path))
            && !self.excludes.iter().any(|p| p.matches(path))
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
    /// [`DeclarativeConfig::scope_ref_contains`] instead.
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

/// An ordered stack of layers: each layer may only depend on layers below it.
#[derive(Debug, Clone)]
pub struct Layering {
    name: Option<String>,
    order: Vec<ScopeName>,
    adjacent_only: bool,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
    check_inline: bool,
}

impl Layering {
    /// Creates a new layering rule.
    ///
    /// `order` lists scopes top-down: the first entry may depend on all the
    /// others, the last entry may depend on none of them.
    ///
    /// # Errors
    ///
    /// Returns error if `order` has fewer than two entries or repeats a scope.
    pub fn new(
        name: Option<String>,
        order: Vec<ScopeName>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Result<Self, ModelError> {
        if order.len() < 2 {
            return Err(ModelError::LayerOrderTooShort { count: order.len() });
        }
        for (i, layer) in order.iter().enumerate() {
            if order[..i].contains(layer) {
                return Err(ModelError::DuplicateLayer {
                    name: layer.clone(),
                });
            }
        }
        Ok(Self {
            name,
            order,
            adjacent_only: false,
            message,
            doc_ref,
            severity,
            check_inline: true,
        })
    }

    /// Sets whether only the immediately-next layer may be depended upon
    /// (skipping a layer becomes a violation).
    #[must_use]
    pub fn with_adjacent_only(mut self, adjacent_only: bool) -> Self {
        self.adjacent_only = adjacent_only;
        self
    }

    /// Sets whether inline qualified paths are also checked.
    #[must_use]
    pub fn with_check_inline(mut self, check_inline: bool) -> Self {
        self.check_inline = check_inline;
        self
    }

    /// Returns the rule name, or the fallback `"layering"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| "layering".to_string())
    }

    /// Returns the layer order, top-down.
    #[must_use]
    pub fn order(&self) -> &[ScopeName] {
        &self.order
    }

    /// Returns whether only adjacent (immediately below) dependencies are allowed.
    #[must_use]
    pub fn adjacent_only(&self) -> bool {
        self.adjacent_only
    }

    /// Returns whether inline qualified paths are checked.
    #[must_use]
    pub fn check_inline(&self) -> bool {
        self.check_inline
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

    /// Expands this layering into the equivalent [`ScopeDep`] constraints.
    ///
    /// For layer `i`, every layer above it (`0..i`) is denied. With
    /// `adjacent_only`, the layers below the next one (`i + 2..`) are denied
    /// as well, so skipping a layer is reported too.
    #[must_use]
    pub fn expand(&self) -> Vec<ScopeDep> {
        let name = self.display_name();
        self.order
            .iter()
            .enumerate()
            .filter_map(|(i, from)| {
                let mut denied: Vec<ScopeName> = self.order[..i].to_vec();
                if self.adjacent_only {
                    let skip_start = (i + 2).min(self.order.len());
                    denied.extend(self.order[skip_start..].iter().cloned());
                }
                if denied.is_empty() {
                    return None;
                }
                Some(
                    ScopeDep::new(
                        Some(name.clone()),
                        from.clone(),
                        denied,
                        self.message.clone(),
                        self.doc_ref.clone(),
                        self.severity,
                    )
                    .with_check_inline(self.check_inline),
                )
            })
            .collect()
    }
}

/// A validated attribute pattern such as `tracing::instrument`,
/// `serde::Serialize` or `derive(sqlx::FromRow)`.
#[derive(Debug, Clone)]
pub struct AttrPattern {
    raw: String,
    derive_inner: Option<String>,
}

impl AttrPattern {
    /// Creates a new attribute pattern.
    ///
    /// # Errors
    ///
    /// Returns error if the pattern is empty or is a `derive()` with no
    /// path inside.
    pub fn new(pattern: &str) -> Result<Self, ModelError> {
        let raw = pattern.trim();
        if raw.is_empty() {
            return Err(ModelError::EmptyAttrPattern);
        }
        let derive_inner = raw
            .strip_prefix("derive(")
            .and_then(|rest| rest.strip_suffix(')'))
            .map(str::trim);
        if let Some(inner) = derive_inner {
            if inner.is_empty() {
                return Err(ModelError::InvalidAttrPattern {
                    pattern: raw.to_string(),
                    reason: "`derive(..)` needs a path inside the parentheses".to_string(),
                });
            }
        }
        Ok(Self {
            raw: raw.to_string(),
            derive_inner: derive_inner.map(ToString::to_string),
        })
    }

    /// Returns the pattern as written in the TOML.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Tests whether this pattern matches an attribute.
    ///
    /// `attr_path` is the attribute's own path rendered with `::`
    /// (`tracing::instrument`, `derive`, ...); `derive_paths` holds the paths
    /// inside a `#[derive(..)]` list (empty for any other attribute).
    ///
    /// A `derive(P)` pattern matches only derive lists. A bare pattern matches
    /// either the attribute path itself or an entry of a derive list. A derive
    /// entry matches when it equals the pattern in full, or when its last
    /// segment equals the pattern — so `FromRow` matches both
    /// `#[derive(FromRow)]` and `#[derive(sqlx::FromRow)]`, while
    /// `sqlx::FromRow` matches only the latter.
    #[must_use]
    pub fn matches(&self, attr_path: &str, derive_paths: &[String]) -> bool {
        let needle = self.derive_inner.as_deref().unwrap_or(&self.raw);
        if self.derive_inner.is_none() && attr_path == needle {
            return true;
        }
        attr_path == "derive"
            && derive_paths
                .iter()
                .any(|d| d == needle || last_path_segment(d) == needle)
    }
}

fn last_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

/// An attribute-denial rule: items in a scope must not carry given attributes.
#[derive(Debug, Clone)]
pub struct DenyAttr {
    name: String,
    scope: ScopeRef,
    attrs: Vec<AttrPattern>,
    on: Vec<ItemKind>,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl DenyAttr {
    /// Creates a new deny-attr rule.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        attrs: Vec<AttrPattern>,
        on: Vec<ItemKind>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            attrs,
            on,
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

    /// Returns the denied attribute patterns.
    #[must_use]
    pub fn attrs(&self) -> &[AttrPattern] {
        &self.attrs
    }

    /// Returns the item kinds this rule applies to.
    #[must_use]
    pub fn on(&self) -> &[ItemKind] {
        &self.on
    }

    /// Returns whether this rule applies to the given item kind.
    #[must_use]
    pub fn applies_to(&self, kind: ItemKind) -> bool {
        self.on.contains(&kind)
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

/// An attribute-requirement rule: items in a scope must carry one of the
/// given attributes.
#[derive(Debug, Clone)]
pub struct RequireAttr {
    name: String,
    scope: ScopeRef,
    attrs: Vec<AttrPattern>,
    on: Vec<ItemKind>,
    visibility: VisibilityFilter,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RequireAttr {
    /// Creates a new require-attr rule checking `pub` items.
    ///
    /// Use [`Self::with_visibility`] to widen it to every item.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        attrs: Vec<AttrPattern>,
        on: Vec<ItemKind>,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            attrs,
            on,
            visibility: VisibilityFilter::Pub,
            message,
            doc_ref,
            severity,
        }
    }

    /// Sets which items are checked by visibility.
    #[must_use]
    pub fn with_visibility(mut self, visibility: VisibilityFilter) -> Self {
        self.visibility = visibility;
        self
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

    /// Returns the required attribute patterns (at least one must be present).
    #[must_use]
    pub fn attrs(&self) -> &[AttrPattern] {
        &self.attrs
    }

    /// Returns the item kinds this rule applies to.
    #[must_use]
    pub fn on(&self) -> &[ItemKind] {
        &self.on
    }

    /// Returns whether this rule applies to the given item kind.
    #[must_use]
    pub fn applies_to(&self, kind: ItemKind) -> bool {
        self.on.contains(&kind)
    }

    /// Returns the visibility filter.
    #[must_use]
    pub fn visibility(&self) -> VisibilityFilter {
        self.visibility
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

/// A naming rule: identifiers of one item kind must match a regex.
#[derive(Debug, Clone)]
pub struct Naming {
    name: String,
    scope: ScopeRef,
    item: ItemKind,
    pattern: RegexPattern,
    visibility: VisibilityFilter,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl Naming {
    /// Creates a new naming rule checking `pub` items.
    ///
    /// Use [`Self::with_visibility`] to widen it to every item.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        item: ItemKind,
        pattern: RegexPattern,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            item,
            pattern,
            visibility: VisibilityFilter::Pub,
            message,
            doc_ref,
            severity,
        }
    }

    /// Sets which items are checked by visibility.
    #[must_use]
    pub fn with_visibility(mut self, visibility: VisibilityFilter) -> Self {
        self.visibility = visibility;
        self
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

    /// Returns the targeted item kind.
    #[must_use]
    pub fn item(&self) -> ItemKind {
        self.item
    }

    /// Returns the identifier pattern.
    #[must_use]
    pub fn pattern(&self) -> &RegexPattern {
        &self.pattern
    }

    /// Returns the visibility filter.
    #[must_use]
    pub fn visibility(&self) -> VisibilityFilter {
        self.visibility
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

/// A return-type rule: functions in a scope must return a matching type.
#[derive(Debug, Clone)]
pub struct RequireReturn {
    name: String,
    scope: ScopeRef,
    visibility: VisibilityFilter,
    asyncness: Option<bool>,
    pattern: RegexPattern,
    message: String,
    doc_ref: Option<String>,
    severity: Severity,
}

impl RequireReturn {
    /// Creates a new require-return rule checking every `pub` function.
    ///
    /// Use [`Self::with_visibility`] and [`Self::with_asyncness`] to narrow it.
    #[must_use]
    pub fn new(
        name: String,
        scope: ScopeRef,
        pattern: RegexPattern,
        message: String,
        doc_ref: Option<String>,
        severity: Severity,
    ) -> Self {
        Self {
            name,
            scope,
            visibility: VisibilityFilter::Pub,
            asyncness: None,
            pattern,
            message,
            doc_ref,
            severity,
        }
    }

    /// Sets which functions are checked by visibility.
    #[must_use]
    pub fn with_visibility(mut self, visibility: VisibilityFilter) -> Self {
        self.visibility = visibility;
        self
    }

    /// Restricts the rule to `async` (`Some(true)`) or non-`async`
    /// (`Some(false)`) functions; `None` checks every function.
    #[must_use]
    pub fn with_asyncness(mut self, asyncness: Option<bool>) -> Self {
        self.asyncness = asyncness;
        self
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

    /// Returns the visibility filter.
    #[must_use]
    pub fn visibility(&self) -> VisibilityFilter {
        self.visibility
    }

    /// Returns the asyncness filter.
    #[must_use]
    pub fn asyncness(&self) -> Option<bool> {
        self.asyncness
    }

    /// Returns the return-type pattern.
    #[must_use]
    pub fn pattern(&self) -> &RegexPattern {
        &self.pattern
    }

    /// Returns whether a function with the given asyncness is checked.
    #[must_use]
    pub fn matches_asyncness(&self, is_async: bool) -> bool {
        self.asyncness.is_none_or(|want| want == is_async)
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
    layerings: Vec<Layering>,
    deny_attrs: Vec<DenyAttr>,
    require_attrs: Vec<RequireAttr>,
    namings: Vec<Naming>,
    require_returns: Vec<RequireReturn>,
}

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
            .map(|s| (s.name.clone(), s))
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
            check_ref(&rule.scope, format!("restrict-use '{}'", rule.name));
        }
        for rule in &self.require_uses {
            check_ref(&rule.scope, format!("require-use '{}'", rule.name));
        }
        for rule in &self.deny_attrs {
            check_ref(&rule.scope, format!("deny-attr '{}'", rule.name));
        }
        for rule in &self.require_attrs {
            check_ref(&rule.scope, format!("require-attr '{}'", rule.name));
        }
        for rule in &self.namings {
            check_ref(&rule.scope, format!("naming '{}'", rule.name));
        }
        for rule in &self.require_returns {
            check_ref(&rule.scope, format!("require-return '{}'", rule.name));
        }

        // Validate scope-dep refs
        for dep in &self.scope_deps {
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

        // Validate layer names
        for layering in &self.layerings {
            for layer in &layering.order {
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
            .map(|r| r.name())
            .collect();
        assert_eq!(applicable.len(), 2);
        assert!(applicable.contains(&"no-sqlx-in-shared"));
        assert!(applicable.contains(&"no-diesel-in-domain"));
    }

    // -- Scope exclude --

    #[test]
    fn scope_exclude_removes_matching_file() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/**").unwrap()],
        )
        .with_excludes(vec![GlobPattern::new("src/**/tests/**").unwrap()]);

        assert!(scope.contains(Path::new("src/domain/entity.rs")));
        assert!(!scope.contains(Path::new("src/domain/tests/entity_test.rs")));
        assert_eq!(scope.excludes().len(), 1);
    }

    #[test]
    fn scope_without_excludes_matches_everything_in_paths() {
        let scope = Scope::new(
            ScopeName::new("domain").unwrap(),
            vec![GlobPattern::new("src/**").unwrap()],
        );
        assert!(scope.contains(Path::new("src/domain/tests/entity_test.rs")));
    }

    // -- RestrictUse except --

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

    // -- RegexPattern --

    #[test]
    fn regex_pattern_valid_and_matching() {
        let pat = RegexPattern::new("^[A-Z][A-Za-z0-9]*(Repository|Port)$").unwrap();
        assert!(pat.is_match("UserRepository"));
        assert!(!pat.is_match("UserService"));
        assert_eq!(pat.as_str(), "^[A-Z][A-Za-z0-9]*(Repository|Port)$");
    }

    #[test]
    fn regex_pattern_rejects_empty_and_invalid() {
        assert!(matches!(
            RegexPattern::new(""),
            Err(ModelError::EmptyRegexPattern)
        ));
        assert!(matches!(
            RegexPattern::new("([a-z"),
            Err(ModelError::InvalidRegexPattern { .. })
        ));
    }

    // -- ItemKind / VisibilityFilter --

    #[test]
    fn item_kind_parse_roundtrip() {
        for kind in ItemKind::ALL {
            assert_eq!(ItemKind::parse(kind.as_str()).unwrap(), kind);
        }
    }

    #[test]
    fn item_kind_rejects_unknown() {
        assert!(matches!(
            ItemKind::parse("macro"),
            Err(ModelError::InvalidItemKind { .. })
        ));
    }

    #[test]
    fn item_kind_named_rejects_impl_and_union() {
        assert!(ItemKind::parse_named("trait").is_ok());
        assert!(matches!(
            ItemKind::parse_named("impl"),
            Err(ModelError::InvalidItemKind { .. })
        ));
        assert!(matches!(
            ItemKind::parse_named("union"),
            Err(ModelError::InvalidItemKind { .. })
        ));
    }

    #[test]
    fn visibility_filter_parse_and_match() {
        assert!(VisibilityFilter::parse("pub").unwrap().matches(true));
        assert!(!VisibilityFilter::parse("pub").unwrap().matches(false));
        assert!(VisibilityFilter::parse("any").unwrap().matches(false));
        assert!(matches!(
            VisibilityFilter::parse("private"),
            Err(ModelError::InvalidVisibility { .. })
        ));
    }

    // -- AttrPattern --

    #[test]
    fn attr_pattern_matches_plain_path() {
        let pat = AttrPattern::new("tracing::instrument").unwrap();
        assert!(pat.matches("tracing::instrument", &[]));
        assert!(!pat.matches("instrument", &[]));
    }

    #[test]
    fn attr_pattern_matches_derive_entry_by_full_path() {
        let pat = AttrPattern::new("sqlx::FromRow").unwrap();
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
        // last-segment-only spelling in the code does not match a qualified pattern
        assert!(!pat.matches("derive", &["FromRow".to_string()]));
    }

    #[test]
    fn attr_pattern_bare_name_matches_both_derive_spellings() {
        let pat = AttrPattern::new("FromRow").unwrap();
        assert!(pat.matches("derive", &["FromRow".to_string()]));
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
    }

    #[test]
    fn attr_pattern_derive_wrapper_only_matches_derives() {
        let pat = AttrPattern::new("derive(sqlx::FromRow)").unwrap();
        assert!(pat.matches("derive", &["sqlx::FromRow".to_string()]));
        assert!(!pat.matches("sqlx::FromRow", &[]));
    }

    #[test]
    fn attr_pattern_rejects_empty_and_empty_derive() {
        assert!(matches!(
            AttrPattern::new("  "),
            Err(ModelError::EmptyAttrPattern)
        ));
        assert!(matches!(
            AttrPattern::new("derive()"),
            Err(ModelError::InvalidAttrPattern { .. })
        ));
    }

    // -- Layering --

    fn layer_names(dep: &ScopeDep) -> Vec<&str> {
        dep.denied_targets().iter().map(ScopeName::as_str).collect()
    }

    fn make_layering(adjacent_only: bool) -> Layering {
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

    #[test]
    fn layering_expands_to_upward_denials() {
        let deps = make_layering(false).expand();
        // handler (top) denies nothing, so it is not expanded
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].from_scope().as_str(), "domain");
        assert_eq!(layer_names(&deps[0]), vec!["handler"]);
        assert_eq!(deps[1].from_scope().as_str(), "infra");
        assert_eq!(layer_names(&deps[1]), vec!["handler", "domain"]);
        assert_eq!(deps[0].display_name(), "clean-arch");
    }

    #[test]
    fn layering_adjacent_only_also_denies_skipping() {
        let deps = make_layering(true).expand();
        assert_eq!(deps.len(), 3);
        // handler may only reach domain — infra is a skipped layer
        assert_eq!(deps[0].from_scope().as_str(), "handler");
        assert_eq!(layer_names(&deps[0]), vec!["infra"]);
        assert_eq!(deps[1].from_scope().as_str(), "domain");
        assert_eq!(layer_names(&deps[1]), vec!["handler"]);
        assert_eq!(layer_names(&deps[2]), vec!["handler", "domain"]);
    }

    #[test]
    fn layering_default_name_is_layering() {
        let layering = Layering::new(
            None,
            vec![ScopeName::new("a").unwrap(), ScopeName::new("b").unwrap()],
            "msg".to_string(),
            None,
            Severity::Error,
        )
        .unwrap();
        assert_eq!(layering.display_name(), "layering");
        assert!(layering.check_inline());
        assert!(!layering.adjacent_only());
    }

    #[test]
    fn layering_rejects_short_order() {
        assert!(matches!(
            Layering::new(
                None,
                vec![ScopeName::new("a").unwrap()],
                "msg".to_string(),
                None,
                Severity::Error,
            ),
            Err(ModelError::LayerOrderTooShort { count: 1 })
        ));
    }

    #[test]
    fn layering_rejects_duplicate_layer() {
        assert!(matches!(
            Layering::new(
                None,
                vec![
                    ScopeName::new("a").unwrap(),
                    ScopeName::new("b").unwrap(),
                    ScopeName::new("a").unwrap(),
                ],
                "msg".to_string(),
                None,
                Severity::Error,
            ),
            Err(ModelError::DuplicateLayer { .. })
        ));
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
    fn require_return_asyncness_filter() {
        let rule = |asyncness| {
            RequireReturn::new(
                "r".to_string(),
                ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
                RegexPattern::new("^Result<").unwrap(),
                "msg".to_string(),
                None,
                Severity::Error,
            )
            .with_asyncness(asyncness)
        };
        assert!(rule(None).matches_asyncness(true));
        assert!(rule(None).matches_asyncness(false));
        assert!(rule(Some(true)).matches_asyncness(true));
        assert!(!rule(Some(true)).matches_asyncness(false));
    }

    #[test]
    fn attr_rules_applies_to_item_kind() {
        let deny = DenyAttr::new(
            "no-orm".to_string(),
            ScopeRef::Inline(vec![GlobPattern::new("src/**").unwrap()]),
            vec![AttrPattern::new("sqlx::FromRow").unwrap()],
            vec![ItemKind::Struct, ItemKind::Enum],
            "msg".to_string(),
            None,
            Severity::Error,
        );
        assert!(deny.applies_to(ItemKind::Struct));
        assert!(!deny.applies_to(ItemKind::Fn));
        assert_eq!(deny.attrs().len(), 1);
        assert_eq!(deny.on().len(), 2);
    }
}
