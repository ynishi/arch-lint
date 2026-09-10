//! The `[[layering]]` rule entity (ALD004).

use super::{ModelError, ScopeDep, ScopeName};
use crate::types::Severity;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declarative::model::test_support::make_layering;

    fn layer_names(dep: &ScopeDep) -> Vec<&str> {
        dep.denied_targets().iter().map(ScopeName::as_str).collect()
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
}
