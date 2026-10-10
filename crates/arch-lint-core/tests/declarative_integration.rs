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

fn patterns_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declarative_patterns")
}

fn suppression_fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declarative_suppression")
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

    assert_eq!(scope_dep.rule, "deny-scope-dep:domain");
    assert_eq!(scope_dep.severity, Severity::Error);
    assert!(scope_dep.message.contains("domain"));
    assert!(scope_dep.message.contains("infra"));
    assert!(scope_dep
        .location
        .file
        .to_string_lossy()
        .contains("domain/service.rs"));
}

/// Acceptance 1, end-to-end through `Analyzer::analyze()` (not just the
/// rule's own `check()`): `use super::super::infra::db::Pool;` in a fresh
/// `src/domain/service.rs` is reported as `ALD003`, at the line it is
/// written on. Built under a tempdir (not the shared fixture, which has no
/// `self::`/`super::` usage and must stay that way so the `crate::`-only
/// behaviour-unchanged diff keeps comparing like for like).
#[test]
fn scope_dep_double_super_violation_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let domain = root.join("src/domain");
    let infra = root.join("src/infra");
    std::fs::create_dir_all(&domain).expect("create domain dir");
    std::fs::create_dir_all(&infra).expect("create infra dir");
    std::fs::write(
        domain.join("service.rs"),
        "// a leading comment, so the `use` is not on line 1\nuse super::super::infra::db::Pool;\n",
    )
    .expect("write domain file");
    std::fs::write(infra.join("db.rs"), "pub struct Pool;\n").expect("write infra file");

    let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
severity = "error"
"#;
    let (config, rules) = load_config_and_rules(toml_content);

    let mut builder = Analyzer::builder().root(root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let scope_dep_violations: Vec<_> = result
        .violations
        .iter()
        .filter(|v| v.code == "ALD003")
        .collect();
    assert_eq!(scope_dep_violations.len(), 1, "{:#?}", result.violations);

    let violation = scope_dep_violations[0];
    assert_eq!(
        violation.location.file,
        PathBuf::from("src/domain/service.rs")
    );
    assert_eq!(
        violation.location.line, 2,
        "the `use` is on line 2 (line 1 is a leading comment)"
    );
    assert!(violation.message.contains("super::super::infra::db::Pool"));
}

// ── Predefined patterns: ALD004-ALD008 plus except / exclude ──

#[test]
fn detects_every_predefined_pattern() {
    let root = patterns_fixture_root();
    let toml_content =
        std::fs::read_to_string(root.join("arch-lint.toml")).expect("fixture TOML should exist");
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let summary: Vec<String> = result
        .violations
        .iter()
        .map(|v| {
            format!(
                "{} {} @ {}:{}",
                v.code,
                v.rule,
                v.location.file.display(),
                v.location.line
            )
        })
        .collect();

    for code in ["ALD004", "ALD005", "ALD006", "ALD007", "ALD008"] {
        assert_eq!(
            result.violations.iter().filter(|v| v.code == code).count(),
            1,
            "expected exactly one {code} violation, got: {summary:#?}"
        );
    }

    // `except` suppresses the only sqlx import in the domain scope
    assert!(
        !result.violations.iter().any(|v| v.code == "ALD001"),
        "restrict-use `except` should have suppressed the sqlx::types import: {summary:#?}"
    );

    // `exclude` takes src/domain/tests/** out of the domain scope entirely
    assert!(
        !result.violations.iter().any(|v| v
            .location
            .file
            .to_string_lossy()
            .contains("domain/tests")),
        "scope `exclude` should have skipped the tests directory: {summary:#?}"
    );

    assert_eq!(result.violations.len(), 5, "{summary:#?}");
}

#[test]
fn layering_violation_points_at_the_upward_import() {
    let root = patterns_fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let layering = result
        .violations
        .iter()
        .find(|v| v.code == "ALD004")
        .expect("layering violation should exist");

    assert_eq!(layering.rule, "clean-arch");
    assert_eq!(layering.severity, Severity::Error);
    assert!(layering
        .location
        .file
        .to_string_lossy()
        .contains("infra/db.rs"));
    assert!(layering.message.contains("scope `infra`"));
    assert!(layering.message.contains("scope `handler`"));
}

// ── Suppression: attribute and comment directives on declarative rules ──

#[test]
fn suppression_directives_silence_declarative_rules() {
    let root = suppression_fixture_root();
    let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
    let (config, rules) = load_config_and_rules(&toml_content);

    let mut builder = Analyzer::builder().root(&root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let result = builder.build().unwrap().analyze().unwrap();

    let summary: Vec<String> = result
        .violations
        .iter()
        .map(|v| {
            format!(
                "{} {} @ {}:{} — {}",
                v.code,
                v.rule,
                v.location.file.display(),
                v.location.line,
                v.message
            )
        })
        .collect();

    // Everything in the fixture is suppressed except:
    //   1. the control trait in src/domain/control.rs (nothing suppresses it)
    //   2. the reason-less directive on `list_users`, which is reported as a
    //      "missing required reason" warning instead of the ALD008 error
    assert_eq!(result.violations.len(), 2, "{summary:#?}");

    let control = result
        .violations
        .iter()
        .find(|v| v.code == "ALD007")
        .unwrap_or_else(|| panic!("control naming violation should still fire: {summary:#?}"));
    assert_eq!(control.rule, "domain-ports");
    assert!(control
        .location
        .file
        .to_string_lossy()
        .contains("domain/control.rs"));

    let missing_reason = result
        .violations
        .iter()
        .find(|v| v.code == "ALD008")
        .unwrap_or_else(|| panic!("missing-reason warning should be reported: {summary:#?}"));
    assert_eq!(missing_reason.rule, "handlers-return-app-result");
    assert_eq!(missing_reason.severity, Severity::Warning);
    assert!(missing_reason.message.contains("missing required reason"));
    assert!(missing_reason
        .location
        .file
        .to_string_lossy()
        .contains("handler/api.rs"));

    // The suppressed kinds produce nothing at all.
    for code in ["ALD001", "ALD004", "ALD005", "ALD006"] {
        assert!(
            !result.violations.iter().any(|v| v.code == code),
            "{code} should be suppressed: {summary:#?}"
        );
    }
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

// ── ScopeGraph (issue #4) ──
//
// Fixture: `tests/fixtures/declarative/` declares scopes `domain` and
// `infra`; `src/domain/service.rs:5` has `use crate::infra::db::Connection;`
// and `src/app/handler.rs` belongs to no declared scope.

mod scope_graph {
    use super::fixture_root;
    use arch_lint_core::declarative::graph::EdgeKind;
    use arch_lint_core::declarative::model::ScopeName;
    use arch_lint_core::declarative::{self, model::DeclarativeConfig};
    use arch_lint_core::{
        Analyzer, ProjectContext, ProjectRule, ProjectRuleBox, ScopeGraph, Violation,
    };
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn name(s: &str) -> ScopeName {
        ScopeName::new(s).expect("valid scope name")
    }

    fn fixture_config() -> Arc<DeclarativeConfig> {
        let toml_content = std::fs::read_to_string(fixture_root().join("arch-lint.toml"))
            .expect("fixture TOML should exist");
        Arc::new(
            declarative::load_config_from_toml(&toml_content).expect("fixture config should parse"),
        )
    }

    /// Nodes are exactly the declared scopes (`domain`, `infra`); the file
    /// outside every scope (`src/app/handler.rs`) contributes no node, and
    /// the single cross-scope `use` is recorded as one `domain → infra` edge
    /// pointing at the `use` line (acceptance 1, 2, 3).
    #[test]
    fn scope_graph_has_exact_nodes_and_one_use_edge() {
        let root = fixture_root();
        let config = fixture_config();

        let analyzer = Analyzer::builder()
            .root(&root)
            .declarative(Arc::clone(&config))
            .build()
            .expect("analyzer should build");

        let graph = analyzer.scope_graph().expect("scope graph should build");

        let nodes: Vec<&str> = graph.nodes().map(ScopeName::as_str).collect();
        assert_eq!(
            nodes,
            ["domain", "infra"],
            "app/handler.rs is in no declared scope and must not add an `app` node: {nodes:?}"
        );

        assert_eq!(graph.edge_count(), 1, "exactly one domain -> infra edge");
        assert!(!graph.has_edge(&name("infra"), &name("domain")));

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].kind, EdgeKind::Use);
        assert_eq!(sites[0].path, "crate::infra::db::Connection");
        assert_eq!(sites[0].location.line, 5);
        assert_eq!(
            sites[0].location.file,
            PathBuf::from("src/domain/service.rs")
        );
    }

    /// `Analyzer::scope_graph()` records an inline path reference (not only a
    /// `use`) as an `Inline` site. Built fresh under a tempdir, since the
    /// shared fixture's cross-scope reference is a `use` (covered above).
    #[test]
    fn scope_graph_records_an_inline_path_reference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let domain = root.join("src/domain");
        let infra = root.join("src/infra");
        std::fs::create_dir_all(&domain).expect("create domain dir");
        std::fs::create_dir_all(&infra).expect("create infra dir");
        std::fs::write(
            domain.join("a.rs"),
            "pub fn f() { crate::infra::db::connect(); }\n",
        )
        .expect("write domain file");
        std::fs::write(infra.join("db.rs"), "pub fn connect() {}\n").expect("write infra file");

        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]
"#;
        let config =
            Arc::new(declarative::load_config_from_toml(toml_content).expect("config parses"));

        let analyzer = Analyzer::builder()
            .root(root)
            .declarative(config)
            .build()
            .expect("analyzer should build");

        let graph = analyzer.scope_graph().expect("scope graph should build");

        let sites = graph.sites(&name("domain"), &name("infra"));
        assert_eq!(sites.len(), 1, "exactly one inline site: {sites:#?}");
        assert_eq!(sites[0].kind, EdgeKind::Inline);
        assert_eq!(sites[0].path, "crate::infra::db::connect");
    }

    /// A `ProjectRule` reachable only through `ctx.scope_graph()` sees the
    /// same graph `Analyzer::scope_graph()` reports, and adding it changes
    /// nothing about the per-file violation count (acceptance 4: behaviour
    /// unchanged).
    struct CaptureGraphRule {
        captured: Arc<Mutex<Option<ScopeGraph>>>,
    }

    impl ProjectRule for CaptureGraphRule {
        fn name(&self) -> &'static str {
            "capture-scope-graph"
        }

        fn code(&self) -> &'static str {
            "TEST-CAPTURE"
        }

        fn check_project(&self, ctx: &ProjectContext) -> Vec<Violation> {
            *self.captured.lock().expect("lock") = Some(ctx.scope_graph().clone());
            vec![]
        }
    }

    #[test]
    fn analyze_hands_project_rules_the_same_graph_and_keeps_violation_count() {
        let root = fixture_root();
        let config = fixture_config();
        let toml_content = std::fs::read_to_string(root.join("arch-lint.toml")).unwrap();
        let rules =
            declarative::load_rules_from_toml(&toml_content).expect("fixture rules should load");

        let expected_graph = Analyzer::builder()
            .root(&root)
            .declarative(Arc::clone(&config))
            .build()
            .expect("analyzer should build")
            .scope_graph()
            .expect("scope graph should build");

        let captured: Arc<Mutex<Option<ScopeGraph>>> = Arc::new(Mutex::new(None));
        let capture_rule: ProjectRuleBox = Box::new(CaptureGraphRule {
            captured: Arc::clone(&captured),
        });

        let mut builder = Analyzer::builder()
            .root(&root)
            .declarative(Arc::clone(&config))
            .project_rule_box(capture_rule);
        for rule in rules {
            builder = builder.rule_box(rule);
        }
        let result = builder
            .build()
            .expect("analyzer should build")
            .analyze()
            .expect("analysis should succeed");

        let seen = captured
            .lock()
            .expect("lock")
            .clone()
            .expect("project rule should have run");
        assert_eq!(
            seen, expected_graph,
            "ctx.scope_graph() must match Analyzer::scope_graph()"
        );

        // Unchanged from `detects_all_three_violation_types`: adding the
        // graph-capturing project rule reports no violations of its own and
        // does not change the per-file rule count.
        assert_eq!(
            result.violations.len(),
            3,
            "behaviour must be unchanged: {:#?}",
            result
                .violations
                .iter()
                .map(|v| format!("{} @ {}", v.rule, v.location.file.display()))
                .collect::<Vec<_>>()
        );
        let codes: Vec<&str> = result.violations.iter().map(|v| v.code.as_str()).collect();
        assert!(codes.contains(&"ALD001"));
        assert!(codes.contains(&"ALD002"));
        assert!(codes.contains(&"ALD003"));
    }
}
