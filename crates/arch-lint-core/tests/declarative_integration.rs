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

/// Acceptance 1 of the trailing-allow-comment fix, end-to-end through
/// `Analyzer::analyze()`: a `// arch-lint: allow(...)` comment placed after
/// the offending code on the same line suppresses `ALD003`, on its own
/// line. The identical `use` without the trailing comment (control) still
/// reports the violation.
#[test]
fn scope_dep_trailing_allow_comment_suppresses_the_violation() {
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

    let run = |domain_service_rs: &str| -> Vec<arch_lint_core::Violation> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let domain = root.join("src/domain");
        let infra = root.join("src/infra");
        std::fs::create_dir_all(&domain).expect("create domain dir");
        std::fs::create_dir_all(&infra).expect("create infra dir");
        std::fs::write(domain.join("service.rs"), domain_service_rs).expect("write domain file");
        std::fs::write(infra.join("db.rs"), "pub struct Pool;\n").expect("write infra file");

        let (config, rules) = load_config_and_rules(toml_content);
        let mut builder = Analyzer::builder().root(root).config(config);
        for rule in rules {
            builder = builder.rule_box(rule);
        }
        builder
            .build()
            .unwrap()
            .analyze()
            .unwrap()
            .violations
            .into_iter()
            .filter(|v| v.code == "ALD003")
            .collect()
    };

    let suppressed =
        run("use crate::infra::db::Pool; // arch-lint: allow(deny-scope-dep) reason=\"x\"\n");
    assert!(
        suppressed.is_empty(),
        "trailing allow comment should suppress ALD003: {suppressed:#?}"
    );

    // Control: the same line without the trailing comment still violates.
    let reported = run("use crate::infra::db::Pool;\n");
    assert_eq!(reported.len(), 1, "{reported:#?}");
}

/// A trailing allow comment is scoped to its own line: it must not also
/// suppress a violation on the next line — exactly one ALD003 is still
/// reported, on the second `use`.
#[test]
fn scope_dep_trailing_allow_comment_does_not_suppress_the_next_line() {
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

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let domain = root.join("src/domain");
    let infra = root.join("src/infra");
    std::fs::create_dir_all(&domain).expect("create domain dir");
    std::fs::create_dir_all(&infra).expect("create infra dir");
    std::fs::write(
        domain.join("service.rs"),
        "use crate::infra::db::Pool; // arch-lint: allow(deny-scope-dep) reason=\"x\"\nuse crate::infra::cache::Redis;\n",
    )
    .expect("write domain file");
    std::fs::write(infra.join("db.rs"), "pub struct Pool;\n").expect("write infra file");
    std::fs::write(infra.join("cache.rs"), "pub struct Redis;\n").expect("write infra file");

    let (config, rules) = load_config_and_rules(toml_content);
    let mut builder = Analyzer::builder().root(root).config(config);
    for rule in rules {
        builder = builder.rule_box(rule);
    }
    let violations: Vec<_> = builder
        .build()
        .unwrap()
        .analyze()
        .unwrap()
        .violations
        .into_iter()
        .filter(|v| v.code == "ALD003")
        .collect();

    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(
        violations[0].location.line, 2,
        "only the second `use` (Redis) should still violate: {violations:#?}"
    );
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

/// End-to-end coverage of `[[acyclic]]` (ALD009), issue #6's acceptance
/// items 1-5 (item 6, the CLI surface, is covered in `arch-lint-cli`).
mod acyclic {
    use arch_lint_core::declarative::{self, model::DeclarativeConfig};
    use arch_lint_core::{Analyzer, Severity};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Writes one file, creating its parent directories.
    fn write(root: &std::path::Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        std::fs::write(path, content).expect("write file");
    }

    /// Builds a tempdir project from `(path, content)` source files plus a
    /// `arch-lint.toml`-shaped `toml_content`, and returns its ALD009
    /// violations (any other declarative violation would be a bug in the
    /// fixture, so none are filtered out silently — callers assert on the
    /// full set when that matters). `toml_content` is also parsed as the
    /// base `Config`, so a `[rules.acyclic]` table (N1: `enabled` /
    /// `severity` overrides) in the same document takes effect, exactly as
    /// it would through `Session`.
    fn acyclic_violations(
        toml_content: &str,
        files: &[(&str, &str)],
    ) -> Vec<arch_lint_core::Violation> {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        for (path, content) in files {
            write(root, path, content);
        }

        let config: Arc<DeclarativeConfig> = Arc::new(
            declarative::load_config_from_toml(toml_content).expect("fixture config should load"),
        );
        let project_rules = declarative::create_project_rules_shared(&config);
        let base_config =
            arch_lint_core::Config::parse(toml_content).expect("fixture config should parse");

        let mut builder = Analyzer::builder()
            .root(root)
            .config(base_config)
            .declarative(Arc::clone(&config));
        for rule in project_rules {
            builder = builder.project_rule_box(rule);
        }
        let result = builder
            .build()
            .expect("analyzer should build")
            .analyze()
            .expect("analysis should succeed");

        result
            .violations
            .into_iter()
            .filter(|v| v.code == "ALD009")
            .collect()
    }

    const TWO_SCOPES: &str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[acyclic]]
"#;

    /// Acceptance 1: `domain -> infra` and `infra -> domain` is one ALD009
    /// violation naming the full cycle, at error severity.
    #[test]
    fn two_scope_cycle_is_one_error_naming_the_full_path() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[
                ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.severity, Severity::Error);
        assert!(
            v.message.contains("domain \u{2192} infra \u{2192} domain"),
            "{}",
            v.message
        );
        // N2: the primary location is the first edge's `use` leaf
        // identifier, and the second edge is attached as exactly one label.
        assert_eq!(
            v.location.file,
            std::path::PathBuf::from("src/domain/service.rs")
        );
        assert_eq!(v.location.line, 1);
        assert_eq!(v.location.column, 23, "column of `Pool`");
        assert_eq!(v.labels.len(), 1, "{:#?}", v.labels);
        assert_eq!(
            v.labels[0].location.file,
            std::path::PathBuf::from("src/infra/db.rs")
        );
        assert_eq!(v.labels[0].location.line, 1);
        assert_eq!(v.labels[0].location.column, 29, "column of `Service`");
    }

    /// N1: `[rules.acyclic] enabled = false` disables the rule entirely,
    /// even though the TOML still has a `[[acyclic]]` block and a real
    /// cycle on disk.
    #[test]
    fn rules_table_can_disable_acyclic() {
        let toml_content = format!("{TWO_SCOPES}\n[rules.acyclic]\nenabled = false\n");
        let violations = acyclic_violations(
            &toml_content,
            &[
                ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// N1: `[rules.acyclic] severity = "warning"` overrides the `[[acyclic]]`
    /// block's own (error) severity, the same generic mechanism every other
    /// project rule gets.
    #[test]
    fn rules_table_overrides_severity_to_warning() {
        let toml_content = format!("{TWO_SCOPES}\n[rules.acyclic]\nseverity = \"warning\"\n");
        let violations = acyclic_violations(
            &toml_content,
            &[
                ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    /// Acceptance 2: a 3-scope cycle lists all three scopes in path order.
    #[test]
    fn three_scope_cycle_lists_all_three_in_order() {
        let toml_content = r#"
[[scopes]]
name = "a"
paths = ["src/a/**"]

[[scopes]]
name = "b"
paths = ["src/b/**"]

[[scopes]]
name = "c"
paths = ["src/c/**"]

[[acyclic]]
"#;
        let violations = acyclic_violations(
            toml_content,
            &[
                ("src/a/x.rs", "use crate::b::y::Y;\n"),
                ("src/b/y.rs", "use crate::c::z::Z;\n"),
                ("src/c/z.rs", "use crate::a::x::X;\n"),
            ],
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(
            violations[0]
                .message
                .contains("a \u{2192} b \u{2192} c \u{2192} a"),
            "{}",
            violations[0].message
        );
        assert_eq!(violations[0].labels.len(), 2);
    }

    /// Acceptance 3: a one-directional dependency is not a cycle.
    #[test]
    fn one_directional_dependency_has_no_violation() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[("src/domain/service.rs", "use crate::infra::db::Pool;\n")],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// Acceptance 4: restricting `scopes` to `["domain", "app"]` ignores a
    /// cycle that passes through `infra`.
    #[test]
    fn scopes_filter_ignores_a_cycle_through_an_excluded_scope() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "app"
paths = ["src/app/**"]

[[acyclic]]
scopes = ["domain", "app"]
"#;
        let violations = acyclic_violations(
            toml_content,
            &[
                ("src/domain/service.rs", "use crate::infra::db::Pool;\n"),
                ("src/infra/db.rs", "use crate::app::handler::Handler;\n"),
                (
                    "src/app/handler.rs",
                    "use crate::domain::service::Service;\n",
                ),
            ],
        );
        assert!(
            violations.is_empty(),
            "the domain -> infra -> app -> domain cycle must be ignored: {violations:#?}"
        );
    }

    /// N2 positive control for the test above: the same `scopes` filter
    /// does report a cycle that stays entirely within the filtered set, so
    /// the empty result above is the filter at work, not the rule being
    /// inert.
    #[test]
    fn scopes_filter_still_reports_a_cycle_inside_the_filtered_scopes() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "app"
paths = ["src/app/**"]

[[acyclic]]
scopes = ["domain", "app"]
"#;
        let violations = acyclic_violations(
            toml_content,
            &[
                (
                    "src/domain/service.rs",
                    "use crate::app::handler::Handler;\n",
                ),
                (
                    "src/app/handler.rs",
                    "use crate::domain::service::Service;\n",
                ),
            ],
        );
        assert_eq!(
            violations.len(),
            1,
            "domain <-> app is entirely inside the filter and must be reported: {violations:#?}"
        );
    }

    /// Acceptance 5: an `allow(acyclic)` comment on one `use` of the cycle
    /// removes the violation.
    #[test]
    fn allow_comment_on_one_use_removes_the_violation() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[
                (
                    "src/domain/service.rs",
                    "// arch-lint: allow(acyclic) reason=\"bootstrap wiring\"\nuse crate::infra::db::Pool;\n",
                ),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );
        assert!(
            violations.is_empty(),
            "the allow directive must remove the edge and so the cycle: {violations:#?}"
        );
    }

    /// Acceptance 5 of the trailing-allow-comment fix: the same `allow`
    /// comment, placed after the `use` on the same line instead of above
    /// it, also removes the edge and so the cycle.
    #[test]
    fn trailing_allow_comment_on_one_use_removes_the_violation() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[
                (
                    "src/domain/service.rs",
                    "use crate::infra::db::Pool; // arch-lint: allow(acyclic) reason=\"bootstrap wiring\"\n",
                ),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );
        assert!(
            violations.is_empty(),
            "the trailing allow directive must remove the edge and so the cycle: {violations:#?}"
        );
    }

    /// R1 / F6: an `#[arch_lint::allow(acyclic)]` attribute directly on the
    /// `use` item, with no `reason`, still suppresses the cycle with no
    /// warning — attributes never require one.
    #[test]
    fn item_attribute_without_reason_suppresses_with_no_warning() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[
                (
                    "src/domain/service.rs",
                    "#[arch_lint::allow(acyclic)]\nuse crate::infra::db::Pool;\n",
                ),
                ("src/infra/db.rs", "use crate::domain::service::Service;\n"),
            ],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// R1 / F9: an enclosing `fn` attribute with no `reason`, plus a
    /// reasoned comment on the same site, still suppresses silently — the
    /// attribute wins, so there is no missing-reason warning either.
    #[test]
    fn enclosing_fn_attribute_without_reason_plus_reasoned_comment_suppresses_with_no_warning() {
        let violations = acyclic_violations(
            TWO_SCOPES,
            &[
                (
                    "src/domain/service.rs",
                    "#[arch_lint::allow(acyclic)]\npub fn f() {\n    // arch-lint: allow(acyclic) reason=\"also reasoned\"\n    crate::infra::db::connect();\n}\n",
                ),
                ("src/infra/db.rs", "use crate::domain::service::Service;\npub fn connect() {}\n"),
            ],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// S6, end to end: one edge (`domain -> infra`) has two sites — one
    /// carries a reason-less comment `allow`, the other is unsuppressed.
    /// The reason-less site still gets its own missing-reason warning
    /// (independent of the edge's fate), while the edge itself survives
    /// through its unsuppressed sibling site, so the cycle is still
    /// reported as an ALD009 error.
    #[test]
    fn reason_less_site_warns_while_the_edge_survives_via_its_sibling() {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "src/domain/service.rs",
            "// arch-lint: allow(acyclic)\nuse crate::infra::db::Pool;\n",
        );
        write(
            root,
            "src/domain/other.rs",
            "use crate::infra::cache::Redis;\n",
        );
        write(
            root,
            "src/infra/db.rs",
            "use crate::domain::service::Service;\n",
        );
        write(root, "src/infra/cache.rs", "pub struct Redis;\n");

        let config: Arc<DeclarativeConfig> = Arc::new(
            declarative::load_config_from_toml(TWO_SCOPES).expect("fixture config should load"),
        );
        let project_rules = declarative::create_project_rules_shared(&config);
        let base_config =
            arch_lint_core::Config::parse(TWO_SCOPES).expect("fixture config should parse");

        let mut builder = Analyzer::builder()
            .root(root)
            .config(base_config)
            .declarative(Arc::clone(&config));
        for rule in project_rules {
            builder = builder.project_rule_box(rule);
        }
        let result = builder
            .build()
            .expect("analyzer should build")
            .analyze()
            .expect("analysis should succeed");

        let ald009: Vec<_> = result
            .violations
            .iter()
            .filter(|v| v.code == "ALD009")
            .collect();
        assert_eq!(ald009.len(), 2, "{ald009:#?}");

        let warnings: Vec<_> = ald009
            .iter()
            .filter(|v| v.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1, "{ald009:#?}");
        assert_eq!(
            warnings[0].location.file,
            std::path::PathBuf::from("src/domain/service.rs"),
            "the warning must point at the reason-less site, not its unsuppressed sibling"
        );

        let errors: Vec<_> = ald009
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "the edge survives via the unsuppressed sibling site, so the cycle is still an error: {ald009:#?}"
        );

        assert!(
            result.has_violations_at(Severity::Error),
            "exit-code semantics: this must still fail the build"
        );
    }

    /// Unknown scope names in `scopes` are a load-time error, same as
    /// `[[deny-scope-dep]]`.
    #[test]
    fn unknown_scope_in_scopes_list_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[acyclic]]
scopes = ["domain", "nope"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "unknown scope name must be rejected");
    }
}

/// End-to-end coverage of `[[independent]]` (ALD010), issue #7's acceptance
/// items 1-4 (item 4's CLI surface is covered in `arch-lint-cli`).
mod independent {
    use arch_lint_core::declarative::{self, model::DeclarativeConfig};
    use arch_lint_core::{Analyzer, Severity};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Writes one file, creating its parent directories.
    fn write(root: &std::path::Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        std::fs::write(path, content).expect("write file");
    }

    /// Builds a tempdir project from `(path, content)` source files plus a
    /// `arch-lint.toml`-shaped `toml_content`, and returns its ALD010
    /// violations (any other declarative violation would be a bug in the
    /// fixture, so none are filtered out silently). `toml_content` is also
    /// parsed as the base `Config`, so a `[rules.independent]` table
    /// (`enabled` / `severity` overrides) in the same document takes
    /// effect, exactly as it would through `Session`.
    fn independent_violations(
        toml_content: &str,
        files: &[(&str, &str)],
    ) -> Vec<arch_lint_core::Violation> {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        for (path, content) in files {
            write(root, path, content);
        }

        let config: Arc<DeclarativeConfig> = Arc::new(
            declarative::load_config_from_toml(toml_content).expect("fixture config should load"),
        );
        let project_rules = declarative::create_project_rules_shared(&config);
        let base_config =
            arch_lint_core::Config::parse(toml_content).expect("fixture config should parse");

        let mut builder = Analyzer::builder()
            .root(root)
            .config(base_config)
            .declarative(Arc::clone(&config));
        for rule in project_rules {
            builder = builder.project_rule_box(rule);
        }
        let result = builder
            .build()
            .expect("analyzer should build")
            .analyze()
            .expect("analysis should succeed");

        result
            .violations
            .into_iter()
            .filter(|v| v.code == "ALD010")
            .collect()
    }

    const BILLING_SHIPPING: &str = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[scopes]]
name = "shipping"
paths = ["src/shipping/**"]

[[independent]]
scopes = ["billing", "shipping"]
"#;

    /// Acceptance 1: `billing -> shipping` is exactly one ALD010 violation
    /// at the `use` line, naming both scopes with the arrow, error
    /// severity.
    #[test]
    fn billing_to_shipping_is_one_violation_at_the_use_line() {
        let violations = independent_violations(
            BILLING_SHIPPING,
            &[(
                "src/billing/service.rs",
                "use crate::shipping::order::Order;\n",
            )],
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.severity, Severity::Error);
        assert!(
            v.message.contains("billing \u{2192} shipping"),
            "{}",
            v.message
        );
        assert_eq!(
            v.location.file,
            std::path::PathBuf::from("src/billing/service.rs")
        );
        assert_eq!(v.location.line, 1);
        assert_eq!(v.location.column, 29, "column of `Order`");
        assert!(v.labels.is_empty());
    }

    /// N1: `[rules.independent] enabled = false` disables the rule
    /// entirely, even though the TOML still has an `[[independent]]` block
    /// and a real offending reference on disk.
    #[test]
    fn rules_table_can_disable_independent() {
        let toml_content = format!("{BILLING_SHIPPING}\n[rules.independent]\nenabled = false\n");
        let violations = independent_violations(
            &toml_content,
            &[(
                "src/billing/service.rs",
                "use crate::shipping::order::Order;\n",
            )],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// N1: `[rules.independent] severity = "warning"` overrides the
    /// `[[independent]]` block's own (error) severity.
    #[test]
    fn rules_table_overrides_severity_to_warning() {
        let toml_content =
            format!("{BILLING_SHIPPING}\n[rules.independent]\nseverity = \"warning\"\n");
        let violations = independent_violations(
            &toml_content,
            &[(
                "src/billing/service.rs",
                "use crate::shipping::order::Order;\n",
            )],
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    /// Acceptance 2: `billing -> shared`, with `shared` not in `scopes`, is
    /// not reported.
    #[test]
    fn reference_to_a_scope_outside_the_universe_has_no_violation() {
        let toml_content = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[scopes]]
name = "shipping"
paths = ["src/shipping/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[independent]]
scopes = ["billing", "shipping"]
"#;
        let violations = independent_violations(
            toml_content,
            &[
                ("src/billing/service.rs", "use crate::shared::util::Util;\n"),
                ("src/shared/util.rs", "pub struct Util;\n"),
            ],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// Acceptance 3: adding `catalog` to `scopes` and a
    /// `catalog -> billing` reference reports a second violation, with no
    /// other config change.
    #[test]
    fn adding_a_third_scope_reports_a_second_violation() {
        // A real before/after over the same project: the scope
        // declarations and source files never change, only the
        // `[[independent]].scopes` list does.
        let scopes_decl = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[scopes]]
name = "shipping"
paths = ["src/shipping/**"]

[[scopes]]
name = "catalog"
paths = ["src/catalog/**"]
"#;
        let files: &[(&str, &str)] = &[
            (
                "src/billing/service.rs",
                "use crate::shipping::order::Order;\n",
            ),
            (
                "src/catalog/item.rs",
                "use crate::billing::service::Service;\n",
            ),
        ];

        let before_toml =
            format!("{scopes_decl}\n[[independent]]\nscopes = [\"billing\", \"shipping\"]\n");
        let before_violations = independent_violations(&before_toml, files);
        assert_eq!(
            before_violations.len(),
            1,
            "catalog -> billing must be ignored before catalog joins scopes: {before_violations:#?}"
        );

        let after_toml = format!(
            "{scopes_decl}\n[[independent]]\nscopes = [\"billing\", \"shipping\", \"catalog\"]\n"
        );
        let after_violations = independent_violations(&after_toml, files);
        assert_eq!(
            after_violations.len(),
            2,
            "catalog -> billing must be reported once catalog joins scopes: {after_violations:#?}"
        );
    }

    /// Both directions between the same two scopes report two separate
    /// violations, one per edge.
    #[test]
    fn both_directions_report_two_violations() {
        let violations = independent_violations(
            BILLING_SHIPPING,
            &[
                (
                    "src/billing/service.rs",
                    "use crate::shipping::order::Order;\n",
                ),
                (
                    "src/shipping/order.rs",
                    "use crate::billing::service::Service;\n",
                ),
            ],
        );
        assert_eq!(violations.len(), 2, "{violations:#?}");
    }

    /// Acceptance 4 fixture for the load-time validation: an unknown scope
    /// name in `scopes` is a load error, same as `[[acyclic]]`.
    #[test]
    fn unknown_scope_in_scopes_list_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[independent]]
scopes = ["billing", "nope"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "unknown scope name must be rejected");
    }

    /// `scopes` needs at least two entries — a load-time error.
    #[test]
    fn a_single_scope_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[independent]]
scopes = ["billing"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "fewer than 2 scopes must be rejected");
    }

    /// A repeated scope name collapses to one graph node, so the rule would
    /// silently check nothing against it — a load-time error instead.
    #[test]
    fn an_immediate_duplicate_scope_in_scopes_list_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "billing"
paths = ["src/billing/**"]

[[independent]]
scopes = ["billing", "billing"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "a duplicate scope name must be rejected");
    }

    /// A non-adjacent duplicate (`a`, `b`, `a`) is rejected the same way.
    #[test]
    fn a_non_adjacent_duplicate_scope_in_scopes_list_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "a"
paths = ["src/a/**"]

[[scopes]]
name = "b"
paths = ["src/b/**"]

[[independent]]
scopes = ["a", "b", "a"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "a duplicate scope name must be rejected");
    }

    /// An `allow(independent) reason="..."` comment on one `use` of the
    /// offending edge removes the violation.
    #[test]
    fn allow_comment_with_reason_removes_the_violation() {
        let violations = independent_violations(
            BILLING_SHIPPING,
            &[(
                "src/billing/service.rs",
                "// arch-lint: allow(independent) reason=\"bootstrap wiring\"\nuse crate::shipping::order::Order;\n",
            )],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// An `#[arch_lint::allow(independent)]` attribute on the `use` item,
    /// with no `reason`, still suppresses the violation with no warning —
    /// attributes never require one.
    #[test]
    fn item_attribute_without_reason_suppresses_with_no_warning() {
        let violations = independent_violations(
            BILLING_SHIPPING,
            &[(
                "src/billing/service.rs",
                "#[arch_lint::allow(independent)]\nuse crate::shipping::order::Order;\n",
            )],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// A comment `allow` with no reason on an error-severity rule reports
    /// the missing-reason warning in place of the violation.
    #[test]
    fn comment_without_reason_on_error_rule_warns() {
        let violations = independent_violations(
            BILLING_SHIPPING,
            &[(
                "src/billing/service.rs",
                "// arch-lint: allow(independent)\nuse crate::shipping::order::Order;\n",
            )],
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(
            violations[0].message.contains("missing required reason"),
            "{}",
            violations[0].message
        );
    }
}

/// End-to-end coverage of `[[allow-dep]]` (ALD011), issue #8's acceptance
/// items 1-4 (item 4's CLI surface is covered in `arch-lint-cli`).
mod allow_dep {
    use arch_lint_core::declarative::{self, model::DeclarativeConfig};
    use arch_lint_core::{Analyzer, Severity};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Writes one file, creating its parent directories.
    fn write(root: &std::path::Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        std::fs::write(path, content).expect("write file");
    }

    /// Builds a tempdir project from `(path, content)` source files plus a
    /// `arch-lint.toml`-shaped `toml_content`, and returns its ALD011
    /// violations (any other declarative violation would be a bug in the
    /// fixture, so none are filtered out silently). `toml_content` is also
    /// parsed as the base `Config`, so a `[rules.allow-dep]` table
    /// (`enabled` / `severity` overrides) in the same document takes
    /// effect, exactly as it would through `Session`.
    fn allow_dep_violations(
        toml_content: &str,
        files: &[(&str, &str)],
    ) -> Vec<arch_lint_core::Violation> {
        let tmp = TempDir::new().expect("tempdir");
        let root = tmp.path();
        for (path, content) in files {
            write(root, path, content);
        }

        let config: Arc<DeclarativeConfig> = Arc::new(
            declarative::load_config_from_toml(toml_content).expect("fixture config should load"),
        );
        let project_rules = declarative::create_project_rules_shared(&config);
        let base_config =
            arch_lint_core::Config::parse(toml_content).expect("fixture config should parse");

        let mut builder = Analyzer::builder()
            .root(root)
            .config(base_config)
            .declarative(Arc::clone(&config));
        for rule in project_rules {
            builder = builder.project_rule_box(rule);
        }
        let result = builder
            .build()
            .expect("analyzer should build")
            .analyze()
            .expect("analysis should succeed");

        result
            .violations
            .into_iter()
            .filter(|v| v.code == "ALD011")
            .collect()
    }

    const DOMAIN_SHARED_INFRA: &str = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[allow-dep]]
from = "domain"
to = ["shared"]
"#;

    /// Acceptance 1: `domain -> shared` (allowed) and `domain -> infra`
    /// (not allowed) with `to = ["shared"]` reports exactly one ALD011
    /// violation, for `infra`, at the `use` line, with the message naming
    /// both scopes and the arrow; the CLI's exit code is covered in
    /// `arch-lint-cli`.
    #[test]
    fn domain_to_infra_is_the_only_violation() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[
                ("src/domain/a.rs", "use crate::shared::util::Util;\n"),
                ("src/domain/b.rs", "use crate::infra::db::Pool;\n"),
            ],
        );

        assert_eq!(violations.len(), 1, "{violations:#?}");
        let v = &violations[0];
        assert_eq!(v.severity, Severity::Error);
        assert!(v.message.contains("domain \u{2192} infra"), "{}", v.message);
        assert_eq!(v.location.file, std::path::PathBuf::from("src/domain/b.rs"));
        assert_eq!(v.location.line, 1);
        assert!(v.labels.is_empty());
    }

    /// Acceptance 2: a reference to an external crate (resolves to no
    /// declared scope) never reaches the graph, so it is never a violation.
    #[test]
    fn reference_to_an_external_crate_has_no_violation() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[("src/domain/a.rs", "use tokio::fs::File;\n")],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// Acceptance 3: `to = []` forbids every declared-scope dependency from
    /// `domain` — two declared targets referenced report two violations.
    #[test]
    fn empty_to_forbids_every_declared_scope_dependency() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[allow-dep]]
from = "domain"
to = []
"#;
        let violations = allow_dep_violations(
            toml_content,
            &[
                ("src/domain/a.rs", "use crate::shared::util::Util;\n"),
                ("src/domain/b.rs", "use crate::infra::db::Pool;\n"),
            ],
        );
        assert_eq!(violations.len(), 2, "{violations:#?}");
    }

    /// A reference from a scope other than `from` is ignored, even when it
    /// targets a scope outside `to`.
    #[test]
    fn reference_from_a_different_scope_is_ignored() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[("src/infra/db.rs", "use crate::domain::service::Service;\n")],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// `to` containing `from` is a load-time error.
    #[test]
    fn to_containing_from_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[allow-dep]]
from = "domain"
to = ["shared", "domain"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "`to` containing `from` must be rejected");
    }

    /// A duplicate in `to` is a load-time error.
    #[test]
    fn duplicate_in_to_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[allow-dep]]
from = "domain"
to = ["shared", "shared"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "a duplicate `to` entry must be rejected");
    }

    /// An unknown `from` scope is a load-time error.
    #[test]
    fn unknown_from_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[allow-dep]]
from = "nope"
to = ["shared"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "unknown `from` scope must be rejected");
    }

    /// An unknown `to` scope is a load-time error.
    #[test]
    fn unknown_to_is_a_load_error() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[allow-dep]]
from = "domain"
to = ["nope"]
"#;
        let result = declarative::load_config_from_toml(toml_content);
        assert!(result.is_err(), "unknown `to` scope must be rejected");
    }

    /// Two `[[allow-dep]]` blocks for the same `from` are both enforced,
    /// each against its own `to`.
    #[test]
    fn two_blocks_for_the_same_from_are_both_enforced() {
        let toml_content = r#"
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "shared"
paths = ["src/shared/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[allow-dep]]
name = "domain-allows-shared-only"
from = "domain"
to = ["shared"]

[[allow-dep]]
name = "domain-allows-infra-only"
from = "domain"
to = ["infra"]
"#;
        let violations = allow_dep_violations(
            toml_content,
            &[
                ("src/domain/a.rs", "use crate::shared::util::Util;\n"),
                ("src/domain/b.rs", "use crate::infra::db::Pool;\n"),
            ],
        );
        // `domain -> infra` violates the first block (only `shared`
        // allowed), and `domain -> shared` violates the second (only
        // `infra` allowed) — two violations, neither block widening the
        // other's allow-list.
        assert_eq!(violations.len(), 2, "{violations:#?}");
    }

    /// N1-style: `[rules.allow-dep] enabled = false` disables the rule
    /// entirely, even though the TOML still has an `[[allow-dep]]` block and
    /// a real offending reference on disk.
    #[test]
    fn rules_table_can_disable_allow_dep() {
        let toml_content = format!("{DOMAIN_SHARED_INFRA}\n[rules.allow-dep]\nenabled = false\n");
        let violations = allow_dep_violations(
            &toml_content,
            &[("src/domain/a.rs", "use crate::infra::db::Pool;\n")],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// N1-style: `[rules.allow-dep] severity = "warning"` overrides the
    /// `[[allow-dep]]` block's own (error) severity.
    #[test]
    fn rules_table_overrides_severity_to_warning() {
        let toml_content =
            format!("{DOMAIN_SHARED_INFRA}\n[rules.allow-dep]\nseverity = \"warning\"\n");
        let violations = allow_dep_violations(
            &toml_content,
            &[("src/domain/a.rs", "use crate::infra::db::Pool;\n")],
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
    }

    /// An `allow(allow-dep) reason="..."` comment on the offending `use`
    /// removes the violation.
    #[test]
    fn allow_comment_with_reason_removes_the_violation() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[(
                "src/domain/a.rs",
                "// arch-lint: allow(allow-dep) reason=\"bootstrap wiring\"\nuse crate::infra::db::Pool;\n",
            )],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// An `#[arch_lint::allow(allow_dep)]` attribute on the `use` item, with
    /// no `reason`, still suppresses the violation with no warning —
    /// attributes never require one.
    #[test]
    fn item_attribute_without_reason_suppresses_with_no_warning() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[(
                "src/domain/a.rs",
                "#[arch_lint::allow(allow_dep)]\nuse crate::infra::db::Pool;\n",
            )],
        );
        assert!(violations.is_empty(), "{violations:#?}");
    }

    /// A comment `allow` with no reason on an error-severity rule reports
    /// the missing-reason warning in place of the violation.
    #[test]
    fn comment_without_reason_on_error_rule_warns() {
        let violations = allow_dep_violations(
            DOMAIN_SHARED_INFRA,
            &[(
                "src/domain/a.rs",
                "// arch-lint: allow(allow-dep)\nuse crate::infra::db::Pool;\n",
            )],
        );
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert_eq!(violations[0].severity, Severity::Warning);
        assert!(
            violations[0].message.contains("missing required reason"),
            "{}",
            violations[0].message
        );
    }
}
