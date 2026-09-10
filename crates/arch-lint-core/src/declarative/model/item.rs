//! Item selectors: which Rust items a rule targets, by kind and visibility.

use super::ModelError;
use std::fmt;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
