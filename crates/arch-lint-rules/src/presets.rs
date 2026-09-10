//! Rule presets and the name → rule registry.
//!
//! This module is the single place that knows how to construct every
//! built-in rule from its kebab-case name (or `ALxxx` code) and from the
//! per-rule options in `[rules.<name>]` of `arch-lint.toml`.

use crate::{
    AsyncTraitSendCheck, HandlerComplexity, HandlerComplexityConfig, NoErrorSwallowing,
    NoPanicInLib, NoSilentResultDrop, NoSyncIo, NoUnwrapExpect, PreferFromOverInto,
    RequireDocComments, RequireThiserror, RequireTracing, RuntimeMode, TracingEnvInit,
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
        self.rules_with_config(&Config::default())
    }

    /// Returns the rules for this preset, applying per-rule options from `config`.
    ///
    /// Options under `[rules.<name>]` (e.g. `allow_in_tests`, `max_match_arms`)
    /// override the preset defaults. `enabled` / `severity` / `exclude_files`
    /// are applied by the analyzer, not here.
    #[must_use]
    pub fn rules_with_config(self, config: &Config) -> Vec<RuleBox> {
        let names: &[&str] = match self {
            Self::Recommended => RECOMMENDED,
            Self::Strict => ALL,
            Self::Minimal => MINIMAL,
        };
        names
            .iter()
            .filter_map(|name| build_rule(name, self, config.rules.get(*name)))
            .collect()
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
    build_rule(name, Preset::Recommended, None)
}

/// Builds a single rule by name or code, applying `[rules.<name>]` options.
///
/// Returns `None` for unknown names.
#[must_use]
pub fn rule_from_config(name_or_code: &str, config: &Config) -> Option<RuleBox> {
    let name = canonical_rule_name(name_or_code)?;
    build_rule(name, Preset::Recommended, config.rules.get(name))
}

/// Constructs a rule from its canonical name, preset-specific defaults, and
/// optional per-rule config. Config keys match the README `[rules.*]` docs.
fn build_rule(name: &str, preset: Preset, cfg: Option<&RuleConfig>) -> Option<RuleBox> {
    let strict = preset == Preset::Strict;
    let minimal = preset == Preset::Minimal;
    let get_bool = |key: &str, default: bool| cfg.map_or(default, |c| c.get_bool(key, default));
    let get_usize = |key: &str, default: usize| {
        cfg.and_then(|c| c.get_option::<usize>(key))
            .unwrap_or(default)
    };
    let get_strs = |key: &str| cfg.map(|c| c.get_str_array(key)).unwrap_or_default();

    let rule: RuleBox = match name {
        "no-unwrap-expect" => {
            let base = NoUnwrapExpect::new();
            let allow_in_tests = get_bool("allow_in_tests", !strict && base.allow_in_tests);
            let allow_expect = get_bool("allow_expect", minimal || (!strict && base.allow_expect));
            Box::new(
                base.allow_in_tests(allow_in_tests)
                    .allow_expect(allow_expect),
            )
        }
        "no-sync-io" => {
            let patterns = get_strs("allow_patterns");
            let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
            Box::new(NoSyncIo::new().allow_patterns(&refs))
        }
        "no-error-swallowing" => Box::new(NoErrorSwallowing::new()),
        "handler-complexity" => {
            let defaults = HandlerComplexityConfig::default();
            Box::new(
                HandlerComplexity::new()
                    .max_handler_lines(get_usize("max_handler_lines", defaults.max_handler_lines))
                    .max_match_arms(get_usize("max_match_arms", defaults.max_match_arms))
                    .max_enum_variants(get_usize("max_enum_variants", defaults.max_enum_variants)),
            )
        }
        "require-thiserror" => {
            let mut rule = RequireThiserror::new();
            for pattern in get_strs("patterns") {
                rule = rule.add_pattern(pattern);
            }
            Box::new(rule)
        }
        "require-tracing" => Box::new(RequireTracing::new()),
        "tracing-env-init" => Box::new(TracingEnvInit::new()),
        "async-trait-send-check" => {
            let mode = cfg
                .and_then(|c| c.get_option::<String>("runtime_mode"))
                .and_then(|m| match m.as_str() {
                    "single-thread" => Some(RuntimeMode::SingleThread),
                    "multi-thread" => Some(RuntimeMode::MultiThread),
                    _ => None,
                })
                .unwrap_or(AsyncTraitSendCheck::new().runtime_mode);
            Box::new(AsyncTraitSendCheck::new().runtime_mode(mode))
        }
        "prefer-from-over-into" => Box::new(PreferFromOverInto::new()),
        "no-panic-in-lib" => {
            let base = NoPanicInLib::new();
            let allow_in_tests = get_bool("allow_in_tests", !strict && base.allow_in_tests);
            Box::new(base.allow_in_tests(allow_in_tests))
        }
        "require-doc-comments" => {
            let base = RequireDocComments::new();
            let fn_docs = get_bool("require_fn_docs", base.require_fn_docs);
            let struct_docs = get_bool("require_struct_docs", base.require_struct_docs);
            let enum_docs = get_bool("require_enum_docs", base.require_enum_docs);
            Box::new(
                base.require_fn_docs(fn_docs)
                    .require_struct_docs(struct_docs)
                    .require_enum_docs(enum_docs),
            )
        }
        "no-silent-result-drop" => {
            let base = NoSilentResultDrop::new();
            let allow_in_tests = get_bool("allow_in_tests", !strict && base.allow_in_tests);
            let allow_ok = get_bool("allow_ok", !strict && base.allow_ok);
            let allow_let_underscore = get_bool("allow_let_underscore", base.allow_let_underscore);
            Box::new(
                base.allow_in_tests(allow_in_tests)
                    .allow_ok(allow_ok)
                    .allow_let_underscore(allow_let_underscore),
            )
        }
        _ => return None,
    };
    Some(rule)
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
        .filter_map(|name| build_rule(name, Preset::Recommended, None))
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

        let rules = Preset::Strict.rules_with_config(&config);
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
}
