//! Suppression scope tracking for rule visitors.
//!
//! Rules that walk an AST with [`syn::visit::Visit`] share the same scaffolding:
//! on entering a module, function or impl block they widen the "suppressed"
//! state when the item carries `#[arch_lint::allow(...)]`, `#[cfg(test)]`,
//! `#[test]`, or a matching `#[allow(...)]` lint, and they restore the previous
//! state on the way out.
//!
//! [`ScopeTracker`] owns that state so each visitor delegates instead of
//! re-implementing it.
//!
//! # Example
//!
//! ```
//! use arch_lint_core::utils::scope::ScopeTracker;
//! use syn::visit::Visit;
//!
//! struct MyVisitor {
//!     scope: ScopeTracker,
//!     hits: usize,
//! }
//!
//! impl<'ast> Visit<'ast> for MyVisitor {
//!     fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
//!         let scope = self.scope.enter_item_fn(&node.attrs);
//!         if !self.scope.suppressed() {
//!             self.hits += 1;
//!         }
//!         syn::visit::visit_item_fn(self, node);
//!         self.scope.exit(scope);
//!     }
//! }
//!
//! let file: syn::File = syn::parse_str("#[test] fn a() {} fn b() {}")?;
//! let mut visitor = MyVisitor {
//!     scope: ScopeTracker::new("my-rule").with_allow_in_tests(true),
//!     hits: 0,
//! };
//! visitor.visit_file(&file);
//! assert_eq!(visitor.hits, 1);
//! # Ok::<(), syn::Error>(())
//! ```

use super::attributes::{check_arch_lint_allow, has_allow_attr, has_cfg_test, has_test_attr};
use syn::Attribute;

/// Snapshot of a [`ScopeTracker`] taken when entering an item.
///
/// Pass it back to [`ScopeTracker::exit`] after visiting the item's children to
/// restore the enclosing scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeState {
    in_test_context: bool,
    in_allowed_context: bool,
}

/// Tracks whether the visitor is currently inside a suppressed scope.
///
/// A scope is *allowed* when an enclosing item carries
/// `#[arch_lint::allow(<rule name>)]` (or one of the lint names registered with
/// [`ScopeTracker::with_fn_allow_lints`]), and it is a *test* scope when an
/// enclosing item carries `#[cfg(test)]` or `#[test]`.
#[derive(Debug, Clone)]
pub struct ScopeTracker {
    rule_name: &'static str,
    fn_allow_lints: &'static [&'static str],
    allow_in_tests: bool,
    state: ScopeState,
}

impl ScopeTracker {
    /// Creates a tracker for the rule with the given name.
    ///
    /// `rule_name` is matched against `#[arch_lint::allow(...)]` arguments and
    /// may be written in either kebab-case or `snake_case`.
    #[must_use]
    pub const fn new(rule_name: &'static str) -> Self {
        Self {
            rule_name,
            fn_allow_lints: &[],
            allow_in_tests: false,
            state: ScopeState {
                in_test_context: false,
                in_allowed_context: false,
            },
        }
    }

    /// Registers `#[allow(...)]` lint names that also suppress this rule when
    /// they appear on a function (e.g. `"clippy::unwrap_used"`).
    #[must_use]
    pub const fn with_fn_allow_lints(mut self, lints: &'static [&'static str]) -> Self {
        self.fn_allow_lints = lints;
        self
    }

    /// Sets whether a test scope counts as suppressed for [`Self::suppressed`].
    #[must_use]
    pub const fn with_allow_in_tests(mut self, allow: bool) -> Self {
        self.allow_in_tests = allow;
        self
    }

    /// Enters a module: `#[cfg(test)]` opens a test scope and
    /// `#[arch_lint::allow(...)]` opens an allowed scope.
    pub fn enter_item_mod(&mut self, attrs: &[Attribute]) -> ScopeState {
        let saved = self.state;
        if has_cfg_test(attrs) {
            self.state.in_test_context = true;
        }
        self.note_allow(attrs);
        saved
    }

    /// Enters a function: `#[test]` opens a test scope, while
    /// `#[arch_lint::allow(...)]` and the lints registered with
    /// [`Self::with_fn_allow_lints`] open an allowed scope.
    pub fn enter_item_fn(&mut self, attrs: &[Attribute]) -> ScopeState {
        let saved = self.state;
        if has_test_attr(attrs) {
            self.state.in_test_context = true;
        }
        if !self.fn_allow_lints.is_empty() && has_allow_attr(attrs, self.fn_allow_lints) {
            self.state.in_allowed_context = true;
        }
        self.note_allow(attrs);
        saved
    }

    /// Enters an impl block: `#[arch_lint::allow(...)]` opens an allowed scope.
    pub fn enter_item_impl(&mut self, attrs: &[Attribute]) -> ScopeState {
        let saved = self.state;
        self.note_allow(attrs);
        saved
    }

    /// Restores the scope captured by one of the `enter_*` methods.
    pub fn exit(&mut self, saved: ScopeState) {
        self.state = saved;
    }

    /// Returns true if `attrs` carry `#[arch_lint::allow(...)]` for this rule.
    ///
    /// Use this for items that are checked in place without opening a scope.
    #[must_use]
    pub fn allows(&self, attrs: &[Attribute]) -> bool {
        check_arch_lint_allow(attrs, self.rule_name).is_allowed()
    }

    /// Returns true if an enclosing item opened an allowed scope.
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        self.state.in_allowed_context
    }

    /// Returns true if an enclosing item opened a test scope.
    #[must_use]
    pub const fn in_test_context(&self) -> bool {
        self.state.in_test_context
    }

    /// Returns true if the current scope is allowed, or is a test scope and the
    /// rule was configured with [`Self::with_allow_in_tests`].
    #[must_use]
    pub const fn suppressed(&self) -> bool {
        self.state.in_allowed_context || (self.allow_in_tests && self.state.in_test_context)
    }

    fn note_allow(&mut self, attrs: &[Attribute]) {
        if self.allows(attrs) {
            self.state.in_allowed_context = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn tracker() -> ScopeTracker {
        ScopeTracker::new("no-unwrap-expect")
    }

    #[test]
    fn test_default_state_is_not_suppressed() {
        let tracker = tracker();
        assert!(!tracker.is_allowed());
        assert!(!tracker.in_test_context());
        assert!(!tracker.suppressed());
    }

    #[test]
    fn test_mod_cfg_test_opens_test_scope() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[cfg(test)])];
        let mut tracker = tracker().with_allow_in_tests(true);

        let saved = tracker.enter_item_mod(&attrs);
        assert!(tracker.in_test_context());
        assert!(tracker.suppressed());
        assert!(!tracker.is_allowed());

        tracker.exit(saved);
        assert!(!tracker.in_test_context());
        assert!(!tracker.suppressed());
    }

    #[test]
    fn test_test_scope_not_suppressed_without_allow_in_tests() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[test])];
        let mut tracker = tracker();

        let _saved = tracker.enter_item_fn(&attrs);
        assert!(tracker.in_test_context());
        assert!(!tracker.suppressed());
    }

    #[test]
    fn test_arch_lint_allow_opens_allowed_scope() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect)])];
        let mut tracker = tracker();

        let saved = tracker.enter_item_impl(&attrs);
        assert!(tracker.is_allowed());
        assert!(tracker.suppressed());

        tracker.exit(saved);
        assert!(!tracker.is_allowed());
    }

    #[test]
    fn test_fn_allow_lints() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[allow(clippy::unwrap_used)])];

        let mut without = tracker();
        let _ = without.enter_item_fn(&attrs);
        assert!(!without.is_allowed());

        let mut with = tracker().with_fn_allow_lints(&["clippy::unwrap_used"]);
        let _ = with.enter_item_fn(&attrs);
        assert!(with.is_allowed());
    }

    #[test]
    fn test_nested_scopes_restore_independently() {
        let allow_attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect)])];
        let plain_attrs: Vec<Attribute> = vec![];
        let mut tracker = tracker();

        let outer = tracker.enter_item_mod(&allow_attrs);
        let inner = tracker.enter_item_fn(&plain_attrs);
        assert!(tracker.is_allowed(), "inner scope inherits outer allowance");

        tracker.exit(inner);
        assert!(tracker.is_allowed());
        tracker.exit(outer);
        assert!(!tracker.is_allowed());
    }

    #[test]
    fn test_allows_without_entering() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[arch_lint::allow(no_unwrap_expect)])];
        let tracker = tracker();

        assert!(tracker.allows(&attrs));
        assert!(!tracker.is_allowed());
        assert!(!tracker.allows(&[]));
    }
}
