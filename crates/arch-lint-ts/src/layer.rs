//! Layer resolution: maps packages/imports to architecture layers.

use crate::config::ArchConfig;

/// Resolves fully-qualified package names to architecture layer names.
///
/// Resolution uses longest-prefix-match so that more specific package
/// prefixes take priority over broader ones.
pub struct LayerResolver {
    /// (package_prefix, layer_name) sorted by prefix length descending.
    map: Vec<(String, String)>,
}

impl LayerResolver {
    /// Build a resolver from config.
    #[must_use]
    pub fn new(config: &ArchConfig) -> Self {
        let mut map: Vec<(String, String)> = Vec::new();
        for layer in &config.layers {
            for pkg in &layer.packages {
                map.push((pkg.clone(), layer.name.clone()));
            }
        }
        // Longest prefix first for correct matching
        map.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Self { map }
    }

    /// Which layer does this package belong to?
    #[must_use]
    pub fn resolve(&self, qualified_name: &str) -> Option<&str> {
        for (prefix, layer_name) in &self.map {
            if qualified_name == prefix || qualified_name.starts_with(&format!("{prefix}.")) {
                return Some(layer_name);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ArchConfig, LayerDef};

    fn make_config() -> ArchConfig {
        ArchConfig {
            root: ".".into(),
            exclude: vec![],
            layers: vec![
                LayerDef {
                    name: "domain".into(),
                    packages: vec!["com.example.domain".into()],
                },
                LayerDef {
                    name: "app".into(),
                    packages: vec!["com.example.app".into()],
                },
                LayerDef {
                    name: "infra".into(),
                    packages: vec![
                        "com.example.infra".into(),
                        "com.example.infra.db".into(), // more specific
                    ],
                },
            ],
            dependencies: [
                ("domain".into(), vec![]),
                ("app".into(), vec!["domain".into()]),
                ("infra".into(), vec!["domain".into(), "app".into()]),
            ]
            .into_iter()
            .collect(),
            constraints: vec![],
        }
    }

    #[test]
    fn resolves_exact_match() {
        let r = LayerResolver::new(&make_config());
        assert_eq!(r.resolve("com.example.domain"), Some("domain"));
    }

    #[test]
    fn resolves_subpackage() {
        let r = LayerResolver::new(&make_config());
        assert_eq!(r.resolve("com.example.domain.model.User"), Some("domain"));
    }

    #[test]
    fn resolves_longest_prefix() {
        let r = LayerResolver::new(&make_config());
        // com.example.infra.db is more specific than com.example.infra
        assert_eq!(r.resolve("com.example.infra.db.Repo"), Some("infra"));
    }

    #[test]
    fn unknown_package_returns_none() {
        let r = LayerResolver::new(&make_config());
        assert_eq!(r.resolve("org.other.Foo"), None);
    }

    #[test]
    fn no_false_prefix_match() {
        let r = LayerResolver::new(&make_config());
        // "com.example.domains" should NOT match "com.example.domain"
        assert_eq!(r.resolve("com.example.domains.Foo"), None);
    }
}
