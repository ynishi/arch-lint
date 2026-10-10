//! SDK-level integration tests for [`arch_lint::Session`].
//!
//! These drive the public API the way an embedder would: point a session at a
//! directory, hand it configuration, run it and inspect the result.

use arch_lint::report::{render, Format};
use arch_lint::session::ConfigSource;
use arch_lint::{Session, Severity};
use std::fs;
use tempfile::TempDir;

/// A project with one domain file that imports `sqlx`.
fn project_with_sqlx_import() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let domain = tmp.path().join("src/domain");
    fs::create_dir_all(&domain).expect("create dirs");
    fs::write(
        domain.join("model.rs"),
        "use sqlx::PgPool;\n\n/// A user.\npub struct User {\n    /// Pool.\n    pub pool: PgPool,\n}\n",
    )
    .expect("write source");
    tmp
}

const DECLARATIVE_CONFIG: &str = r#"
preset = "minimal"

[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::**"]
message = "Domain must be DB-agnostic."
severity = "error"
"#;

#[test]
fn from_config_str_runs_a_declarative_rule() {
    let tmp = project_with_sqlx_import();

    let session = Session::from_config_str(tmp.path(), DECLARATIVE_CONFIG).expect("session builds");
    assert_eq!(session.config_source(), &ConfigSource::Default);
    assert_eq!(session.fail_on(), Severity::Error);

    let fail_on = session.fail_on();
    let result = session.run().expect("analysis runs");

    assert_eq!(result.files_checked, 1);
    assert_eq!(
        result.violations.len(),
        1,
        "expected exactly the declarative violation, got {:?}",
        result.violations
    );

    let violation = &result.violations[0];
    assert_eq!(violation.code, "ALD001");
    assert_eq!(violation.severity, Severity::Error);
    assert!(
        violation.message.contains("Domain must be DB-agnostic."),
        "{}",
        violation.message
    );
    assert!(result.has_violations_at(fail_on));

    let rendered = render(&result, Format::default()).expect("render");
    assert!(rendered.contains("ALD001"), "{rendered}");
    assert!(!rendered.contains('\x1b'), "library default must not color");
}

#[test]
fn discover_prefers_the_project_file_over_the_global_one() {
    let global = TempDir::new().expect("tempdir");
    fs::write(
        global.path().join("config.toml"),
        "preset = \"strict\"\nfail_on = \"info\"\n",
    )
    .expect("write global config");

    let project = project_with_sqlx_import();
    fs::write(project.path().join("arch-lint.toml"), DECLARATIVE_CONFIG)
        .expect("write project config");

    // `ConfigSource::resolve_in` takes the global directory as an argument, so
    // this test does not touch $ARCH_LINT_CONFIG_DIR and cannot race with
    // tests running in parallel in the same process.
    let source = ConfigSource::resolve_in(project.path(), None, Some(global.path().to_path_buf()));
    assert_eq!(
        source,
        ConfigSource::Project(project.path().join("arch-lint.toml"))
    );

    let session = Session::from_source(project.path(), source).expect("session builds");
    assert_eq!(session.preset(), arch_lint::rules::Preset::Minimal);
    assert_eq!(session.fail_on(), Severity::Error);

    // The global file alone yields the other settings, proving the two configs
    // really differ and the project one won.
    let global_session = Session::from_source(
        project.path(),
        ConfigSource::Global(global.path().join("config.toml")),
    )
    .expect("session builds");
    assert_eq!(global_session.preset(), arch_lint::rules::Preset::Strict);
    assert_eq!(global_session.fail_on(), Severity::Info);
}

#[test]
fn discover_falls_back_to_defaults_without_any_config() {
    let project = project_with_sqlx_import();

    let source = ConfigSource::resolve_in(project.path(), None, None);
    assert_eq!(source, ConfigSource::Default);

    let session = Session::from_source(project.path(), source).expect("session builds");
    assert_eq!(session.preset(), arch_lint::rules::Preset::Recommended);
    assert_eq!(session.fail_on(), Severity::Error);
}

#[test]
fn rule_filter_narrows_the_rule_set() {
    let tmp = project_with_sqlx_import();

    match Session::from_config_str(tmp.path(), "")
        .expect("session builds")
        .with_rule_filter(&["AL001", "bogus"])
    {
        Ok(_) => panic!("unknown rule must be rejected"),
        Err(e) => assert!(e.to_string().contains("bogus"), "{e}"),
    }

    let session = Session::from_config_str(tmp.path(), "")
        .expect("session builds")
        .with_rule_filter(&["require-doc-comments"])
        .expect("known rule accepted");
    let result = session.run().expect("analysis runs");
    assert!(result
        .violations
        .iter()
        .all(|v| v.rule == "require-doc-comments"));
}

#[test]
fn exclude_pattern_skips_files() {
    let tmp = project_with_sqlx_import();

    let session = Session::from_config_str(tmp.path(), DECLARATIVE_CONFIG)
        .expect("session builds")
        .with_exclude("**/domain/**");
    let result = session.run().expect("analysis runs");

    assert_eq!(result.files_checked, 0);
    assert!(result.violations.is_empty());
}

// ── ScopeGraph (issue #4) ──

/// A project with a `domain` file that imports from `infra`.
fn project_with_cross_scope_import() -> TempDir {
    let tmp = TempDir::new().expect("tempdir");
    let domain = tmp.path().join("src/domain");
    let infra = tmp.path().join("src/infra");
    fs::create_dir_all(&domain).expect("create dirs");
    fs::create_dir_all(&infra).expect("create dirs");
    fs::write(
        domain.join("service.rs"),
        "use crate::infra::db::Connection;\n\npub fn run() {}\n",
    )
    .expect("write source");
    fs::write(infra.join("db.rs"), "pub struct Connection;\n").expect("write source");
    tmp
}

const SCOPE_GRAPH_CONFIG: &str = r#"
preset = "minimal"

[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]
"#;

#[test]
fn scope_graph_reports_the_cross_scope_edge() {
    let tmp = project_with_cross_scope_import();

    let graph = Session::from_config_str(tmp.path(), SCOPE_GRAPH_CONFIG)
        .expect("session builds")
        .scope_graph()
        .expect("scope graph builds");

    let nodes: Vec<&str> = graph.nodes().map(|n| n.as_str()).collect();
    assert_eq!(nodes, ["domain", "infra"]);
    assert_eq!(graph.edge_count(), 1, "exactly one domain -> infra edge");
}

#[test]
fn scope_graph_is_empty_without_scopes() {
    let tmp = project_with_cross_scope_import();

    let graph = Session::from_config_str(tmp.path(), "")
        .expect("session builds")
        .scope_graph()
        .expect("scope graph builds");

    assert!(
        graph.is_empty(),
        "a config with no [[scopes]] yields an empty graph"
    );
}
