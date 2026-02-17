//! Integration test: declarative rules end-to-end via Analyzer.
//!
//! Uses fixture files under `tests/fixtures/declarative/` to verify
//! that the full TOML → DTO → domain model → Rule → Analyzer pipeline
//! correctly detects architecture violations.

use arch_lint_core::declarative;
use arch_lint_core::{Analyzer, Config, Severity};
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declarative")
}

fn load_config_and_rules(toml_content: &str) -> (Config, Vec<arch_lint_core::RuleBox>) {
    let config = if toml_content.is_empty() {
        Config::default()
    } else {
        Config::parse(toml_content).expect("fixture config should parse")
    };
    let rules = declarative::load_rules_from_toml(toml_content).expect("fixture rules should load");
    (config, rules)
}

// ── Happy-path: detects expected violations ──

#[test]
fn detects_all_three_violation_types() {
    let root = fixture_root();
    let toml_content =
        std::fs::read_to_string(root.join("arch-lint.toml")).expect("fixture TOML should exist");

    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let analyzer = builder.build().expect("analyzer should build");
    let result = analyzer.analyze().expect("analysis should succeed");

    // Expect exactly 3 violations:
    //   1. restrict-use: sqlx in domain/service.rs
    //   2. deny-scope-dep: domain -> infra in domain/service.rs
    //   3. require-use: log in app/handler.rs
    assert_eq!(
        result.violations.len(),
        3,
        "expected 3 violations, got {}: {:#?}",
        result.violations.len(),
        result
            .violations
            .iter()
            .map(|v| format!("{} @ {}", v.rule, v.location.file.display()))
            .collect::<Vec<_>>()
    );

    // Verify violation codes
    let codes: Vec<&str> = result.violations.iter().map(|v| v.code.as_str()).collect();
    assert!(codes.contains(&"ALD001"), "missing restrict-use violation");
    assert!(codes.contains(&"ALD002"), "missing require-use violation");
    assert!(
        codes.contains(&"ALD003"),
        "missing deny-scope-dep violation"
    );
}

#[test]
fn restrict_use_violation_details() {
    let root = fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let restrict = result
        .violations
        .iter()
        .find(|v| v.code == "ALD001")
        .expect("should have restrict-use violation");

    assert_eq!(restrict.rule, "no-sqlx-in-domain");
    assert_eq!(restrict.severity, Severity::Error);
    assert!(restrict.message.contains("sqlx::Pool"));
    assert!(restrict
        .location
        .file
        .to_string_lossy()
        .contains("domain/service.rs"));
}

#[test]
fn require_use_violation_details() {
    let root = fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let require = result
        .violations
        .iter()
        .find(|v| v.code == "ALD002")
        .expect("should have require-use violation");

    assert_eq!(require.rule, "prefer-tracing");
    assert_eq!(require.severity, Severity::Warning);
    assert!(require.message.contains("tracing"));
    assert!(require.message.contains("log"));
    assert!(require
        .location
        .file
        .to_string_lossy()
        .contains("app/handler.rs"));
}

#[test]
fn scope_dep_violation_details() {
    let root = fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let scope_dep = result
        .violations
        .iter()
        .find(|v| v.code == "ALD003")
        .expect("should have deny-scope-dep violation");

    assert_eq!(scope_dep.rule, "deny-scope-dep");
    assert_eq!(scope_dep.severity, Severity::Error);
    assert!(scope_dep.message.contains("domain"));
    assert!(scope_dep.message.contains("infra"));
    assert!(scope_dep
        .location
        .file
        .to_string_lossy()
        .contains("domain/service.rs"));
}

// ── Edge case: empty config produces no violations ──

#[test]
fn empty_config_no_violations() {
    let root = fixture_root();
    let (config, rules) = load_config_and_rules("");

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    assert!(
        result.violations.is_empty(),
        "empty config should produce no violations"
    );
}

// ── Severity filtering ──

#[test]
fn has_violations_at_respects_severity() {
    let root = fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    // There are Error-level violations (restrict-use, scope-dep)
    assert!(result.has_violations_at(Severity::Error));
    // There are also Warning-level violations (require-use)
    assert!(result.has_violations_at(Severity::Warning));
}
