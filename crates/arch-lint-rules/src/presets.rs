//! Rule presets and the name → rule registry.
//!
//! This module is the single place that knows how to construct every
//! built-in rule from its kebab-case name (or `ALxxx` code) and from the
//! per-rule options in `[rules.<name>]` of `arch-lint.toml`.

use crate::options::{
    parse_options, AsyncTraitSendCheckOptions, HandlerComplexityOptions, NoErrorSwallowingOptions,
    NoPanicInLibOptions, NoSilentResultDropOptions, NoSyncIoOptions, NoUnwrapExpectOptions,
    PreferFromOverIntoOptions, RequireDocCommentsOptions, RequireThiserrorOptions,
    RequireTracingOptions, RuleOptionsError, TracingEnvInitOptions,
};
use crate::{
    AsyncTraitSendCheck, HandlerComplexity, HandlerComplexityConfig, NoErrorSwallowing,
    NoPanicInLib, NoSilentResultDrop, NoSyncIo, NoUnwrapExpect, PreferFromOverInto,
    RequireDocComments, RequireThiserror, RequireTracing, TracingEnvInit,
};
use arch_lint_core::{Config, RuleBox, RuleConfig};

/// Preset configurations for arch-lint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Recommended rules with sensible defaults.
    Recommended,
    /// Strict rules for maximum safety.
    Strict,
    /// Minimal rules for gradual adoption.
    Minimal,
}

/// Error returned when parsing an unknown preset name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown preset `{0}`. Valid presets: recommended, strict, minimal")]
pub struct ParsePresetError(pub String);

impl std::str::FromStr for Preset {
    type Err = ParsePresetError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "recommended" => Ok(Self::Recommended),
            "strict" => Ok(Self::Strict),
            "minimal" => Ok(Self::Minimal),
            other => Err(ParsePresetError(other.to_string())),
        }
    }
}

impl Preset {
    /// Returns the rules for this preset with default options.
    #[must_use]
    pub fn rules(self) -> Vec<RuleBox> {
        self.rule_names()
            .iter()
            .filter_map(|name| build_rule(name, self, None).ok().flatten())
            .collect()
    }

    /// Returns the rules for this preset, applying per-rule options from `config`.
    ///
    /// Options under `[rules.<name>]` (e.g. `allow_in_tests`, `max_match_arms`)
    /// override the preset defaults. `enabled` / `severity` / `exclude_files`
    /// are applied by the analyzer, not here.
    ///
    /// # Errors
    ///
    /// Returns [`RuleOptionsError`] for the first `[rules.<name>]` table that
    /// carries an option the rule does not accept, or a value of the wrong
    /// type.
    pub fn rules_with_config(self, config: &Config) -> Result<Vec<RuleBox>, RuleOptionsError> {
        let names: &[&str] = self.rule_names();
        let mut rules = Vec::with_capacity(names.len());
        for name in names {
            if let Some(rule) = build_rule(name, self, config.rules.get(*name))? {
                rules.push(rule);
            }
        }
        Ok(rules)
    }

    /// Returns the kebab-case names of the rules in this preset.
    #[must_use]
    pub fn rule_names(self) -> &'static [&'static str] {
        match self {
            Self::Recommended => RECOMMENDED,
            Self::Strict => ALL,
            Self::Minimal => MINIMAL,
        }
    }
}

/// Rules in the `recommended` preset.
const RECOMMENDED: &[&str] = &[
    "no-unwrap-expect",
    "no-sync-io",
    "no-error-swallowing",
    "no-silent-result-drop",
    "require-thiserror",
    "require-tracing",
    "tracing-env-init",
];

/// Rules in the `minimal` preset.
const MINIMAL: &[&str] = &["no-unwrap-expect"];

/// Every built-in rule, in code order.
const ALL: &[&str] = &[
    "no-unwrap-expect",
    "no-sync-io",
    "no-error-swallowing",
    "handler-complexity",
    "require-thiserror",
    "require-tracing",
    "tracing-env-init",
    "async-trait-send-check",
    "prefer-from-over-into",
    "no-panic-in-lib",
    "require-doc-comments",
    "no-silent-result-drop",
];

/// Resolves a rule code (`AL001`) to its kebab-case name. Names pass through.
#[must_use]
pub fn canonical_rule_name(name_or_code: &str) -> Option<&'static str> {
    let name = match name_or_code {
        "AL001" | "no-unwrap-expect" => "no-unwrap-expect",
        "AL002" | "no-sync-io" => "no-sync-io",
        "AL003" | "no-error-swallowing" => "no-error-swallowing",
        "AL004" | "handler-complexity" => "handler-complexity",
        "AL005" | "require-thiserror" => "require-thiserror",
        "AL006" | "require-tracing" => "require-tracing",
        "AL007" | "tracing-env-init" => "tracing-env-init",
        "AL009" | "async-trait-send-check" => "async-trait-send-check",
        "AL010" | "prefer-from-over-into" => "prefer-from-over-into",
        "AL011" | "no-panic-in-lib" => "no-panic-in-lib",
        "AL012" | "require-doc-comments" => "require-doc-comments",
        "AL013" | "no-silent-result-drop" => "no-silent-result-drop",
        _ => return None,
    };
    Some(name)
}

/// Builds a single rule by name or code with default options.
///
/// Returns `None` for unknown names.
#[must_use]
pub fn rule_by_name(name_or_code: &str) -> Option<RuleBox> {
    let name = canonical_rule_name(name_or_code)?;
    build_rule(name, Preset::Recommended, None).ok().flatten()
}

/// Builds a single rule by name or code, applying `[rules.<name>]` options.
///
/// Returns `Ok(None)` for unknown names.
///
/// # Errors
///
/// Returns [`RuleOptionsError`] when the rule's `[rules.<name>]` table carries
/// an option the rule does not accept, or a value of the wrong type.
pub fn rule_from_config(
    name_or_code: &str,
    config: &Config,
) -> Result<Option<RuleBox>, RuleOptionsError> {
    let Some(name) = canonical_rule_name(name_or_code) else {
        return Ok(None);
    };
    build_rule(name, Preset::Recommended, config.rules.get(name))
}

/// Constructs a rule from its canonical name, preset-specific defaults, and
/// optional per-rule config.
///
/// The accepted option keys of each rule are defined by its options struct in
/// [`crate::options`]; an unrecognized key is an error rather than a silent
/// no-op.
fn build_rule(
    name: &str,
    preset: Preset,
    cfg: Option<&RuleConfig>,
) -> Result<Option<RuleBox>, RuleOptionsError> {
    let strict = preset == Preset::Strict;

    let rule: RuleBox = match name {
        "no-unwrap-expect" => Box::new(build_no_unwrap_expect(name, preset, cfg)?),
        "no-sync-io" => {
            let opts: NoSyncIoOptions = parse_options(name, cfg)?;
            let patterns = opts.allow_patterns.unwrap_or_default();
            let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
            Box::new(NoSyncIo::new().allow_patterns(&refs))
        }
        "no-error-swallowing" => {
            let _: NoErrorSwallowingOptions = parse_options(name, cfg)?;
            Box::new(NoErrorSwallowing::new())
        }
        "handler-complexity" => Box::new(build_handler_complexity(name, cfg)?),
        "require-thiserror" => {
            let opts: RequireThiserrorOptions = parse_options(name, cfg)?;
            let mut rule = RequireThiserror::new();
            for pattern in opts.patterns.unwrap_or_default() {
                rule = rule.add_pattern(pattern);
            }
            Box::new(rule)
        }
        "require-tracing" => {
            let _: RequireTracingOptions = parse_options(name, cfg)?;
            Box::new(RequireTracing::new())
        }
        "tracing-env-init" => {
            let _: TracingEnvInitOptions = parse_options(name, cfg)?;
            Box::new(TracingEnvInit::new())
        }
        "async-trait-send-check" => {
            let opts: AsyncTraitSendCheckOptions = parse_options(name, cfg)?;
            let base = AsyncTraitSendCheck::new();
            let mode = opts.runtime_mode.unwrap_or(base.runtime_mode);
            Box::new(base.runtime_mode(mode))
        }
        "prefer-from-over-into" => {
            let _: PreferFromOverIntoOptions = parse_options(name, cfg)?;
            Box::new(PreferFromOverInto::new())
        }
        "no-panic-in-lib" => {
            let opts: NoPanicInLibOptions = parse_options(name, cfg)?;
            let base = NoPanicInLib::new();
            let allow_in_tests = opts
                .allow_in_tests
                .unwrap_or(!strict && base.allow_in_tests);
            Box::new(base.allow_in_tests(allow_in_tests))
        }
        "require-doc-comments" => Box::new(build_require_doc_comments(name, cfg)?),
        "no-silent-result-drop" => Box::new(build_no_silent_result_drop(name, preset, cfg)?),
        _ => return Ok(None),
    };
    Ok(Some(rule))
}

/// `no-unwrap-expect`: both allowances relax outside `strict`, and `minimal`
/// additionally tolerates `.expect()`.
fn build_no_unwrap_expect(
    name: &str,
    preset: Preset,
    cfg: Option<&RuleConfig>,
) -> Result<NoUnwrapExpect, RuleOptionsError> {
    let strict = preset == Preset::Strict;
    let minimal = preset == Preset::Minimal;
    let opts: NoUnwrapExpectOptions = parse_options(name, cfg)?;
    let base = NoUnwrapExpect::new();
    let allow_in_tests = opts
        .allow_in_tests
        .unwrap_or(!strict && base.allow_in_tests);
    let allow_expect = opts
        .allow_expect
        .unwrap_or(minimal || (!strict && base.allow_expect));
    Ok(base
        .allow_in_tests(allow_in_tests)
        .allow_expect(allow_expect))
}

/// `handler-complexity`: every limit defaults to [`HandlerComplexityConfig`],
/// independent of the preset.
fn build_handler_complexity(
    name: &str,
    cfg: Option<&RuleConfig>,
) -> Result<HandlerComplexity, RuleOptionsError> {
    let opts: HandlerComplexityOptions = parse_options(name, cfg)?;
    let defaults = HandlerComplexityConfig::default();
    Ok(HandlerComplexity::new()
        .max_handler_lines(opts.max_handler_lines.unwrap_or(defaults.max_handler_lines))
        .max_match_arms(opts.max_match_arms.unwrap_or(defaults.max_match_arms))
        .max_enum_variants(opts.max_enum_variants.unwrap_or(defaults.max_enum_variants)))
}

/// `require-doc-comments`: every item kind is required by default, independent
/// of the preset.
fn build_require_doc_comments(
    name: &str,
    cfg: Option<&RuleConfig>,
) -> Result<RequireDocComments, RuleOptionsError> {
    let opts: RequireDocCommentsOptions = parse_options(name, cfg)?;
    let base = RequireDocComments::new();
    let fn_docs = opts.require_fn_docs.unwrap_or(base.require_fn_docs);
    let struct_docs = opts.require_struct_docs.unwrap_or(base.require_struct_docs);
    let enum_docs = opts.require_enum_docs.unwrap_or(base.require_enum_docs);
    Ok(base
        .require_fn_docs(fn_docs)
        .require_struct_docs(struct_docs)
        .require_enum_docs(enum_docs))
}

/// `no-silent-result-drop`: only the test allowance relaxes outside `strict`.
fn build_no_silent_result_drop(
    name: &str,
    preset: Preset,
    cfg: Option<&RuleConfig>,
) -> Result<NoSilentResultDrop, RuleOptionsError> {
    let strict = preset == Preset::Strict;
    let opts: NoSilentResultDropOptions = parse_options(name, cfg)?;
    let base = NoSilentResultDrop::new();
    let allow_in_tests = opts
        .allow_in_tests
        .unwrap_or(!strict && base.allow_in_tests);
    let allow_ok = opts.allow_ok.unwrap_or(!strict && base.allow_ok);
    let allow_let_underscore = opts
        .allow_let_underscore
        .unwrap_or(base.allow_let_underscore);
    Ok(base
        .allow_in_tests(allow_in_tests)
        .allow_ok(allow_ok)
        .allow_let_underscore(allow_let_underscore))
}

/// Returns the recommended set of rules.
///
/// Includes:
/// - `no-unwrap-expect` (AL001) - Forbids `.unwrap()/.expect()`
/// - `no-sync-io` (AL002) - Forbids blocking I/O
/// - `no-error-swallowing` (AL003) - Forbids silent error handling
/// - `no-silent-result-drop` (AL013) - Forbids silently discarding Result errors
/// - `require-thiserror` (AL005) - Requires thiserror for error types
/// - `require-tracing` (AL006) - Requires tracing instead of log
/// - `tracing-env-init` (AL007) - Prevents hardcoded log levels
#[must_use]
pub fn recommended_rules() -> Vec<RuleBox> {
    Preset::Recommended.rules()
}

/// Returns the strict set of rules.
///
/// Includes every built-in rule (AL001-AL013), with stricter settings:
/// - `no-unwrap-expect` - no exceptions in tests, `.expect()` forbidden
/// - `no-silent-result-drop` - no exceptions in tests, `.ok()` forbidden
/// - `no-panic-in-lib` - no exceptions in tests
#[must_use]
pub fn strict_rules() -> Vec<RuleBox> {
    Preset::Strict.rules()
}

/// Returns the minimal set of rules.
///
/// For gradual adoption, only includes:
/// - `no-unwrap-expect` (allowing `.expect()`)
#[must_use]
pub fn minimal_rules() -> Vec<RuleBox> {
    Preset::Minimal.rules()
}

/// Returns all available rules with default options.
#[must_use]
pub fn all_rules() -> Vec<RuleBox> {
    ALL.iter()
        .filter_map(|name| build_rule(name, Preset::Recommended, None).ok().flatten())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rules: &[RuleBox]) -> Vec<&str> {
        rules.iter().map(|r| r.name()).collect()
    }

    #[test]
    fn test_preset_rules() {
        assert!(!Preset::Recommended.rules().is_empty());
        assert!(!Preset::Strict.rules().is_empty());
        assert!(!Preset::Minimal.rules().is_empty());
    }

    #[test]
    fn strict_and_all_contain_every_rule() {
        assert_eq!(strict_rules().len(), ALL.len());
        assert_eq!(all_rules().len(), ALL.len());
        let all = all_rules();
        let n = names(&all);
        for expected in [
            "async-trait-send-check",
            "prefer-from-over-into",
            "no-panic-in-lib",
            "require-doc-comments",
            "no-silent-result-drop",
        ] {
            assert!(n.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn all_rules_have_unique_codes() {
        let mut codes: Vec<&str> = all_rules().iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), ALL.len());
    }

    #[test]
    fn preset_parse() {
        assert_eq!("strict".parse::<Preset>(), Ok(Preset::Strict));
        assert_eq!("minimal".parse::<Preset>(), Ok(Preset::Minimal));
        assert!("nope".parse::<Preset>().is_err());
    }

    #[test]
    fn rule_by_name_accepts_codes_and_names() {
        assert_eq!(
            rule_by_name("AL004").map(|r| r.name()),
            Some("handler-complexity")
        );
        assert_eq!(rule_by_name("AL012").map(|r| r.code()), Some("AL012"));
        assert!(rule_by_name("AL008").is_none());
        assert!(rule_by_name("unknown").is_none());
    }

    #[test]
    fn config_options_are_applied() {
        let config = Config::parse(
            r#"
[rules.handler-complexity]
max_handler_lines = 7
max_match_arms = 3

[rules.no-sync-io]
allow_patterns = ["my_io::"]
"#,
        )
        .expect("parse");

        let rules = Preset::Strict
            .rules_with_config(&config)
            .expect("options parse");
        let hc = rules
            .iter()
            .find(|r| r.name() == "handler-complexity")
            .expect("handler-complexity present");
        // Exercise the configured limit: 8-line handler must trip max_handler_lines = 7.
        let code = r"
fn handle_request() {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    let e = 5;
    let f = 6;
    let g = 7;
    let _ = (a, b, c, d, e, f, g);
}
";
        let ctx = arch_lint_core::FileContext::new(
            std::path::Path::new("src/handler.rs"),
            code,
            std::path::Path::new("."),
        );
        let ast = syn::parse_file(code).expect("parse");
        assert!(!hc.check(&ctx, &ast).is_empty());

        let default_rules = Preset::Strict.rules();
        let hc_default = default_rules
            .iter()
            .find(|r| r.name() == "handler-complexity")
            .expect("present");
        assert!(hc_default.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn strict_preset_disables_test_allowances() {
        let code = r"
#[cfg(test)]
mod tests {
    fn f() { let x: Option<u8> = None; x.unwrap(); }
}
";
        let ctx = arch_lint_core::FileContext::new(
            std::path::Path::new("src/lib.rs"),
            code,
            std::path::Path::new("."),
        );
        let ast = syn::parse_file(code).expect("parse");
        let strict = Preset::Strict.rules();
        let rule = strict.iter().find(|r| r.code() == "AL001").expect("AL001");
        assert!(!rule.check(&ctx, &ast).is_empty());

        let recommended = Preset::Recommended.rules();
        let rule = recommended
            .iter()
            .find(|r| r.code() == "AL001")
            .expect("AL001");
        assert!(rule.check(&ctx, &ast).is_empty());
    }

    #[test]
    fn unknown_rule_option_is_rejected_with_rule_and_key() {
        let config =
            Config::parse("[rules.no-unwrap-expect]\nallow_in_test = true\n").expect("parse");

        let err = Preset::Recommended
            .rules_with_config(&config)
            .err()
            .expect("typo key must be rejected");
        let text = err.to_string();
        assert_eq!(err.rule, "no-unwrap-expect");
        assert!(text.contains("no-unwrap-expect"), "{text}");
        assert!(text.contains("unknown field `allow_in_test`"), "{text}");

        let err = rule_from_config("AL001", &config)
            .err()
            .expect("typo key must be rejected");
        assert_eq!(err.rule, "no-unwrap-expect");
    }

    #[test]
    fn invalid_runtime_mode_is_rejected() {
        let config = Config::parse("[rules.async-trait-send-check]\nruntime_mode = \"green\"\n")
            .expect("parse");
        let err = Preset::Strict
            .rules_with_config(&config)
            .err()
            .expect("unknown runtime_mode must be rejected");
        assert_eq!(err.rule, "async-trait-send-check");
    }

    #[test]
    fn option_tables_of_rules_outside_the_preset_are_ignored() {
        // `handler-complexity` is not in `recommended`, so its table is never
        // parsed by that preset.
        let config = Config::parse("[rules.handler-complexity]\nnot_a_key = 1\n").expect("parse");
        assert!(Preset::Recommended.rules_with_config(&config).is_ok());
        assert!(Preset::Strict.rules_with_config(&config).is_err());
    }

    /// Number of violations `code` reports for `src`, seen as the file `path`.
    fn violations(rules: &[RuleBox], code: &str, path: &str, src: &str) -> usize {
        let rule = rules
            .iter()
            .find(|r| r.code() == code)
            .unwrap_or_else(|| panic!("{code} is not in this preset"));
        let ctx = arch_lint_core::FileContext::new(
            std::path::Path::new(path),
            src,
            std::path::Path::new("."),
        );
        let ast = syn::parse_file(src).expect("parse");
        rule.check(&ctx, &ast).len()
    }

    const UNWRAP_IN_TESTS: &str = r"
#[cfg(test)]
mod tests {
    fn helper() { let x: Option<u8> = None; x.unwrap(); }
}
";
    const EXPECT_IN_PROD: &str = r#"
fn foo() { let x: Option<u8> = None; x.expect("should exist"); }
"#;

    #[test]
    fn default_no_unwrap_expect_options_per_preset() {
        // allow_in_tests: true under recommended/minimal, false under strict.
        assert_eq!(
            violations(
                &Preset::Recommended.rules(),
                "AL001",
                "src/lib.rs",
                UNWRAP_IN_TESTS
            ),
            0
        );
        assert_eq!(
            violations(
                &Preset::Minimal.rules(),
                "AL001",
                "src/lib.rs",
                UNWRAP_IN_TESTS
            ),
            0
        );
        assert_eq!(
            violations(
                &Preset::Strict.rules(),
                "AL001",
                "src/lib.rs",
                UNWRAP_IN_TESTS
            ),
            1
        );

        // allow_expect: true under minimal, false under recommended/strict.
        assert_eq!(
            violations(
                &Preset::Minimal.rules(),
                "AL001",
                "src/lib.rs",
                EXPECT_IN_PROD
            ),
            0
        );
        assert_eq!(
            violations(
                &Preset::Recommended.rules(),
                "AL001",
                "src/lib.rs",
                EXPECT_IN_PROD
            ),
            1
        );
        assert_eq!(
            violations(
                &Preset::Strict.rules(),
                "AL001",
                "src/lib.rs",
                EXPECT_IN_PROD
            ),
            1
        );
    }

    #[test]
    fn default_no_silent_result_drop_options_per_preset() {
        // allow_in_tests: true under recommended, false under strict.
        let in_tests = r"
#[cfg(test)]
mod tests {
    fn helper() { let r: Result<u8, ()> = Ok(1); r.unwrap_or_default(); }
}
";
        assert_eq!(
            violations(
                &Preset::Recommended.rules(),
                "AL013",
                "src/lib.rs",
                in_tests
            ),
            0
        );
        assert_eq!(
            violations(&Preset::Strict.rules(), "AL013", "src/lib.rs", in_tests),
            1
        );

        // allow_ok: false under every preset.
        let ok_call = r"
fn foo() { let r: Result<u8, ()> = Ok(1); let _v = r.ok(); }
";
        assert_eq!(
            violations(&Preset::Recommended.rules(), "AL013", "src/lib.rs", ok_call),
            1
        );
        assert_eq!(
            violations(&Preset::Strict.rules(), "AL013", "src/lib.rs", ok_call),
            1
        );
    }

    #[test]
    fn default_no_panic_in_lib_allows_tests_outside_strict() {
        let panic_in_tests = r#"
#[cfg(test)]
mod tests {
    fn helper() { panic!("boom"); }
}
"#;
        // `no-panic-in-lib` only ships in `strict`, where allow_in_tests is false.
        assert_eq!(
            violations(
                &Preset::Strict.rules(),
                "AL011",
                "src/lib.rs",
                panic_in_tests
            ),
            1
        );
        // Built outside a preset it keeps the base default (allow_in_tests = true).
        let standalone = vec![rule_by_name("AL011").expect("AL011")];
        assert_eq!(
            violations(&standalone, "AL011", "src/lib.rs", panic_in_tests),
            0
        );
    }

    #[test]
    fn default_handler_complexity_limits() {
        use std::fmt::Write as _;
        let strict = Preset::Strict.rules();

        // max_match_arms = 20: 20 arms pass, 21 trip.
        let arms = |n: usize| {
            let mut body = String::new();
            for i in 0..n {
                let _ = writeln!(body, "        V{i} => {{}},");
            }
            format!("fn handle_action(action: Action) {{\n    match action {{\n{body}    }}\n}}\n")
        };
        assert_eq!(violations(&strict, "AL004", "src/handler.rs", &arms(20)), 0);
        assert_eq!(violations(&strict, "AL004", "src/handler.rs", &arms(21)), 1);

        // max_enum_variants = 30: 30 variants pass, 31 trip.
        let variants = |n: usize| {
            let mut body = String::new();
            for i in 0..n {
                let _ = writeln!(body, "    V{i},");
            }
            format!("enum UserAction {{\n{body}}}\n")
        };
        assert_eq!(
            violations(&strict, "AL004", "src/handler.rs", &variants(30)),
            0
        );
        assert_eq!(
            violations(&strict, "AL004", "src/handler.rs", &variants(31)),
            1
        );

        // max_handler_lines = 150: a 140-statement handler passes, 160 trips.
        let handler = |n: usize| {
            let mut body = String::new();
            for i in 0..n {
                let _ = writeln!(body, "    let v{i} = {i};");
            }
            format!("fn handle_request() {{\n{body}}}\n")
        };
        assert_eq!(
            violations(&strict, "AL004", "src/handler.rs", &handler(140)),
            0
        );
        assert_eq!(
            violations(&strict, "AL004", "src/handler.rs", &handler(160)),
            1
        );
    }

    #[test]
    fn default_require_doc_comments_requires_all_three_kinds() {
        let strict = Preset::Strict.rules();
        for src in [
            "pub fn process_data() {}\n",
            "pub struct Config { name: String }\n",
            "pub enum Mode { A }\n",
        ] {
            assert_eq!(
                violations(&strict, "AL012", "src/lib.rs", src),
                1,
                "expected a violation for {src}"
            );
        }
    }

    #[test]
    fn default_no_sync_io_keeps_only_the_built_in_allow_patterns() {
        let recommended = Preset::Recommended.rules();
        assert_eq!(
            violations(
                &recommended,
                "AL002",
                "src/lib.rs",
                "fn foo() { let _ = std::fs::read_to_string(\"f.txt\"); }\n"
            ),
            1
        );
        assert_eq!(
            violations(
                &recommended,
                "AL002",
                "src/lib.rs",
                "async fn foo() { let _ = tokio::fs::read_to_string(\"f.txt\").await; }\n"
            ),
            0
        );
    }

    #[test]
    fn default_require_thiserror_matches_only_the_error_suffix() {
        let recommended = Preset::Recommended.rules();
        assert_eq!(
            violations(
                &recommended,
                "AL005",
                "src/lib.rs",
                "#[derive(Debug)]\npub enum MyError { Io }\n"
            ),
            1
        );
        assert_eq!(
            violations(
                &recommended,
                "AL005",
                "src/lib.rs",
                "#[derive(Debug)]\npub enum MyFailure { Io }\n"
            ),
            0
        );
    }

    #[test]
    fn default_async_trait_send_check_runtime_mode_is_single_thread() {
        // Under `single-thread` an `#[async_trait]` without `?Send` is flagged.
        let src = r"
use async_trait::async_trait;

#[async_trait]
trait MyTrait {
    async fn foo(&self);
}
";
        assert_eq!(
            violations(&Preset::Strict.rules(), "AL009", "src/lib.rs", src),
            1
        );

        let config =
            Config::parse("[rules.async-trait-send-check]\nruntime_mode = \"multi-thread\"\n")
                .expect("parse");
        let configured = Preset::Strict
            .rules_with_config(&config)
            .expect("options parse");
        assert_eq!(violations(&configured, "AL009", "src/lib.rs", src), 0);
    }
}
