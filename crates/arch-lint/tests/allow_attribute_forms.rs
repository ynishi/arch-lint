//! Acceptance 3 (kebab-attribute-allow): proves, through real compilation
//! rather than `syn` re-parsing, that `#[arch_lint::allow(...)]` — the
//! `arch_lint_macros::allow` proc-macro attribute re-exported as
//! `arch_lint::allow` — accepts every spelling `arch-lint`'s analyzer is
//! expected to recognize: `snake_case`, kebab-case, a rule code, and the
//! `allow-dep:<scope>` display name (`Punct(':')` included).
//!
//! `arch_lint_macros::allow` is an identity transform — it returns the
//! annotated item unchanged and never inspects its own arguments (see
//! `crates/arch-lint-macros/src/lib.rs`) — so nothing here was ever at risk
//! of a macro-side rejection. What *was* at risk, before this fix, is
//! `arch-lint`'s own analyzer silently failing to recognize the kebab-case
//! and `allow-dep:<scope>` spellings once the macro had let them through
//! (see `declarative_integration.rs` and `attributes.rs` for that half).
//! This file exists so a future change to the macro (e.g. validating its
//! arguments) cannot narrow accepted spellings without failing a test: if
//! any attribute below stopped compiling, `cargo test -p arch-lint` would
//! fail right here, with the compiler's own diagnostic as the evidence.

#[arch_lint::allow(no_unwrap_expect, reason = "snake_case form")]
fn snake_case_form() {}

#[arch_lint::allow(no-unwrap-expect, reason = "kebab-case form")]
fn kebab_case_form() {}

#[arch_lint::allow(AL001, reason = "rule code form")]
fn rule_code_form() {}

#[arch_lint::allow(allow-dep:domain, reason = "allow-dep display name form")]
fn allow_dep_display_name_form() {}

/// Review follow-up: a digit-leading segment (`core-2`, as in a scope
/// literally named `core-2` — scope names may contain `[a-z0-9-]`) in an
/// `allow-dep:<scope>` display name compiles the same way: `core-2`
/// tokenizes as `Ident("core")`, `Punct('-')`, `Literal("2")`, which the
/// macro accepts just as readily as any other token sequence (see the
/// module doc above).
#[arch_lint::allow(allow-dep:core-2, reason = "digit-leading scope segment form")]
fn allow_dep_digit_leading_scope_form() {}

#[test]
fn attribute_forms_compile() {
    // Reaching this point means every attribute above was accepted by the
    // real `arch_lint_macros::allow` proc-macro at compile time, not just
    // by `syn` re-parsing the source text as `arch-lint`'s analyzer does.
    snake_case_form();
    kebab_case_form();
    rule_code_form();
    allow_dep_display_name_form();
    allow_dep_digit_leading_scope_form();
}
