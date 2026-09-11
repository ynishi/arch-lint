//! Suppression support shared by the declarative rules.
//!
//! Declarative rules honor the same two directives as the built-in rules:
//!
//! - the block-level attribute `#[arch_lint::allow(<name>, reason = "...")]` on
//!   an enclosing `mod` / `fn` / `impl` (or `trait`) block, and on the offending
//!   item itself, tracked with [`ScopeTracker`];
//! - the inline comment `// arch-lint: allow(<name>) reason="..."` on the
//!   violating line or the line above it, parsed by
//!   [`check_allow_with_reason`].
//!
//! One declarative [`Rule`](crate::rule::Rule) instance carries every TOML rule
//! of its kind, so suppression is tracked *per TOML rule*: [`Suppressions`]
//! holds one [`ScopeTracker`] per applicable TOML rule, and an allow naming one
//! rule never suppresses its neighbours.
//!
//! `<name>` may be the rule kind name (`restrict-use`), the user-given rule
//! name from the TOML (`no-sqlx-in-domain`) or the rule code (`ALD001`), each
//! in either kebab-case or `snake_case`. `allow(all)` matches every rule, as it
//! does for the built-in rules.
//!
//! Test code is *not* exempt: the trackers are built without
//! [`ScopeTracker::with_allow_in_tests`], so `#[cfg(test)]` / `#[test]` scopes
//! stay in scope for declarative rules, which express architecture constraints
//! rather than code-quality preferences.

use crate::context::FileContext;
use crate::types::{Location, Severity, Suggestion, Violation};
use crate::utils::allowance::check_allow_with_reason;
use crate::utils::scope::{ScopeState, ScopeTracker};

/// Position in the scope stack, returned by the `enter_*` methods and passed
/// back to [`Suppressions::exit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScopeMark(usize);

/// What to do with one candidate violation.
pub(crate) enum Decision {
    /// Nothing suppresses it: report it.
    Report,
    /// An allow directive covers it.
    Suppressed,
    /// A comment directive covers it, but the rule requires a reason and the
    /// directive has none: report this warning instead of the violation, the
    /// same way the built-in rules do.
    MissingReason(Box<Violation>),
}

/// Suppression state for one TOML rule.
struct RuleSuppression {
    tracker: ScopeTracker,
    /// Names accepted in a comment directive, in match order.
    comment_names: Vec<String>,
    /// The TOML rule name, used in the missing-reason message.
    display_name: String,
    /// Whether an allow directive must carry a reason (severity `error`, the
    /// same formula as [`Rule::requires_allow_reason`](crate::rule::Rule::requires_allow_reason)).
    reason_required: bool,
}

/// Per-file suppression state for every applicable TOML rule of one kind.
///
/// The rules are addressed by index, in the order they were passed to
/// [`Suppressions::new`] — the same order as the `applicable` vector each
/// declarative rule builds.
pub(crate) struct Suppressions {
    rules: Vec<RuleSuppression>,
    /// Saved scope states, `rules.len()` per entered scope.
    saved: Vec<ScopeState>,
}

impl Suppressions {
    /// Builds the suppression state for the applicable TOML rules of one kind.
    ///
    /// `kind_name` is the rule kind name (`Rule::name()`), `code` the rule code
    /// (`Rule::code()`), and each entry of `rules` is one TOML rule's name and
    /// effective severity.
    pub(crate) fn new<I>(kind_name: &'static str, code: &'static str, rules: I) -> Self
    where
        I: IntoIterator<Item = (String, Severity)>,
    {
        let rules: Vec<RuleSuppression> = rules
            .into_iter()
            .map(|(name, severity)| RuleSuppression {
                tracker: ScopeTracker::new(kind_name)
                    .with_aliases([name.clone(), code.to_string()]),
                comment_names: comment_names(kind_name, code, &name),
                display_name: name,
                reason_required: severity == Severity::Error,
            })
            .collect();

        Self {
            rules,
            saved: Vec::new(),
        }
    }

    /// Enters a `mod` block.
    pub(crate) fn enter_item_mod(&mut self, attrs: &[syn::Attribute]) -> ScopeMark {
        self.enter(attrs, ScopeTracker::enter_item_mod)
    }

    /// Enters a `fn` body.
    pub(crate) fn enter_item_fn(&mut self, attrs: &[syn::Attribute]) -> ScopeMark {
        self.enter(attrs, ScopeTracker::enter_item_fn)
    }

    /// Enters an `impl` or `trait` block: only `#[arch_lint::allow(...)]`
    /// widens the scope, exactly as [`ScopeTracker::enter_item_impl`] does.
    pub(crate) fn enter_block(&mut self, attrs: &[syn::Attribute]) -> ScopeMark {
        self.enter(attrs, ScopeTracker::enter_item_impl)
    }

    fn enter(
        &mut self,
        attrs: &[syn::Attribute],
        enter: fn(&mut ScopeTracker, &[syn::Attribute]) -> ScopeState,
    ) -> ScopeMark {
        let mark = ScopeMark(self.saved.len());
        for rule in &mut self.rules {
            self.saved.push(enter(&mut rule.tracker, attrs));
        }
        mark
    }

    /// Restores the scope captured by one of the `enter_*` methods.
    pub(crate) fn exit(&mut self, mark: ScopeMark) {
        for (offset, rule) in self.rules.iter_mut().enumerate() {
            if let Some(state) = self.saved.get(mark.0 + offset) {
                rule.tracker.exit(*state);
            }
        }
        self.saved.truncate(mark.0);
    }

    /// Decides what to do with a candidate violation of the TOML rule at
    /// `index`, reported at `span`.
    ///
    /// `item_attrs` are the attributes of the item the violation is reported on
    /// (the `use` statement, the struct, the function, ...), when there is one.
    ///
    /// The three mechanisms are consulted in the same order as in the built-in
    /// rules: enclosing scope first, then the item's own attribute, then the
    /// inline comment — and only the comment path checks for a missing reason,
    /// again as in the built-in rules.
    pub(crate) fn decide(
        &self,
        index: usize,
        ctx: &FileContext,
        code: &'static str,
        span: proc_macro2::Span,
        item_attrs: Option<&[syn::Attribute]>,
    ) -> Decision {
        let Some(rule) = self.rules.get(index) else {
            return Decision::Report;
        };

        if rule.tracker.suppressed() {
            return Decision::Suppressed;
        }

        if let Some(attrs) = item_attrs {
            if rule.tracker.allows(attrs) {
                return Decision::Suppressed;
            }
        }

        let start = span.start();
        for name in &rule.comment_names {
            let check = check_allow_with_reason(ctx.content, start.line, name);
            if !check.is_allowed() {
                continue;
            }
            if rule.reason_required && check.reason().is_none() {
                let location =
                    Location::new(ctx.relative_path.clone(), start.line, start.column + 1);
                let violation = Violation::new(
                    code,
                    &rule.display_name,
                    Severity::Warning,
                    location,
                    format!(
                        "Allow directive for '{}' is missing required reason",
                        rule.display_name
                    ),
                )
                .with_suggestion(Suggestion::new(
                    "Add reason=\"...\" to explain why this exception is necessary",
                ));
                return Decision::MissingReason(Box::new(violation));
            }
            return Decision::Suppressed;
        }

        Decision::Report
    }
}

/// Names accepted in a comment directive for one TOML rule.
///
/// Comment directives are matched literally by
/// [`check_allow_with_reason`], so both spellings of every name are listed
/// here; this reproduces the `-`/`_` normalization the attribute matcher
/// applies.
fn comment_names(kind_name: &str, code: &str, user_name: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(6);
    for base in [kind_name, user_name, code] {
        for candidate in [
            base.to_string(),
            base.replace('-', "_"),
            base.replace('_', "-"),
        ] {
            if !names.contains(&candidate) {
                names.push(candidate);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::super::test_support::make_ctx;
    use super::*;
    use syn::spanned::Spanned;

    fn suppressions(severity: Severity) -> Suppressions {
        Suppressions::new(
            "restrict-use",
            "ALD001",
            [("no-sqlx-in-domain".to_string(), severity)],
        )
    }

    fn span_at(content: &str) -> (syn::File, proc_macro2::Span) {
        let file: syn::File = syn::parse_file(content).expect("test code should parse");
        let span = match file.items.first() {
            Some(syn::Item::Use(u)) => u.tree.span(),
            _ => panic!("expected a use item"),
        };
        (file, span)
    }

    #[test]
    fn comment_names_cover_kind_user_and_code() {
        let names = comment_names("restrict-use", "ALD001", "no_sqlx_in_domain");
        assert!(names.contains(&"restrict-use".to_string()));
        assert!(names.contains(&"restrict_use".to_string()));
        assert!(names.contains(&"no_sqlx_in_domain".to_string()));
        assert!(names.contains(&"no-sqlx-in-domain".to_string()));
        assert!(names.contains(&"ALD001".to_string()));
    }

    #[test]
    fn reports_when_nothing_suppresses() {
        let content = "use sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let sup = suppressions(Severity::Error);

        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Report
        ));
    }

    #[test]
    fn out_of_range_index_reports() {
        let content = "use sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let sup = suppressions(Severity::Error);

        assert!(matches!(
            sup.decide(7, &ctx, "ALD001", span, None),
            Decision::Report
        ));
    }

    #[test]
    fn comment_with_reason_suppresses() {
        let content = "// arch-lint: allow(no-sqlx-in-domain) reason=\"legacy\"\nuse sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let sup = suppressions(Severity::Error);

        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Suppressed
        ));
    }

    #[test]
    fn comment_without_reason_on_error_rule_warns() {
        let content = "// arch-lint: allow(no-sqlx-in-domain)\nuse sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let sup = suppressions(Severity::Error);

        match sup.decide(0, &ctx, "ALD001", span, None) {
            Decision::MissingReason(v) => {
                assert_eq!(v.code, "ALD001");
                assert_eq!(v.rule, "no-sqlx-in-domain");
                assert_eq!(v.severity, Severity::Warning);
                assert!(v.message.contains("missing required reason"));
            }
            _ => panic!("expected a missing-reason warning"),
        }
    }

    #[test]
    fn comment_without_reason_on_warning_rule_suppresses() {
        let content = "// arch-lint: allow(no-sqlx-in-domain)\nuse sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let sup = suppressions(Severity::Warning);

        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Suppressed
        ));
    }

    #[test]
    fn scope_enter_and_exit_restore_state() {
        let content = "use sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let mut sup = suppressions(Severity::Error);

        let attrs: Vec<syn::Attribute> =
            vec![syn::parse_quote!(#[arch_lint::allow(no_sqlx_in_domain)])];

        let mark = sup.enter_item_mod(&attrs);
        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Suppressed
        ));
        sup.exit(mark);
        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Report
        ));
    }

    #[test]
    fn test_scopes_do_not_suppress() {
        let content = "use sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let mut sup = suppressions(Severity::Error);

        let attrs: Vec<syn::Attribute> = vec![syn::parse_quote!(#[cfg(test)])];
        let _mark = sup.enter_item_mod(&attrs);

        assert!(
            matches!(sup.decide(0, &ctx, "ALD001", span, None), Decision::Report),
            "declarative rules keep firing inside #[cfg(test)]"
        );
    }

    #[test]
    fn one_rule_name_does_not_suppress_a_sibling() {
        let content = "use sqlx::Pool;";
        let (_file, span) = span_at(content);
        let ctx = make_ctx("src/domain/service.rs", content);
        let mut sup = Suppressions::new(
            "restrict-use",
            "ALD001",
            [
                ("no-sqlx-in-domain".to_string(), Severity::Error),
                ("no-reqwest-in-domain".to_string(), Severity::Error),
            ],
        );

        let attrs: Vec<syn::Attribute> =
            vec![syn::parse_quote!(#[arch_lint::allow(no_sqlx_in_domain, reason = "legacy")])];
        let _mark = sup.enter_item_mod(&attrs);

        assert!(matches!(
            sup.decide(0, &ctx, "ALD001", span, None),
            Decision::Suppressed
        ));
        assert!(matches!(
            sup.decide(1, &ctx, "ALD001", span, None),
            Decision::Report
        ));
    }
}
