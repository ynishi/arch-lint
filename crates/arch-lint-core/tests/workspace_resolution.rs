//! Integration test: `crate::` / `self::` / `super::` resolve against the
//! package the referencing file belongs to (the nearest `Cargo.toml` with a
//! `[package]` table), while scope globs stay relative to the analyzer root.
//!
//! Each fixture is built under a tempdir; none is committed.

use arch_lint_core::declarative::{self, model::DeclarativeConfig, model::ScopeName};
use arch_lint_core::{
    Analyzer, EdgeKind, ProjectContext, ProjectRule, ProjectRuleBox, ScopeGraph, Violation,
};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

const VIRTUAL_ROOT: &str = "[workspace]\nmembers = [\"crates/*\"]\n";

fn package(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n")
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
    std::fs::write(path, content).expect("write file");
}

fn scope(name: &str, glob: &str) -> String {
    format!("[[scopes]]\nname = \"{name}\"\npaths = [\"{glob}\"]\n\n")
}

fn deny(from: &str, to: &str) -> String {
    format!(
        "[[deny-scope-dep]]\nfrom = \"{from}\"\nto = [\"{to}\"]\n\
         message = \"{from} must not use {to}.\"\nseverity = \"error\"\n\n"
    )
}

fn name(s: &str) -> ScopeName {
    ScopeName::new(s).expect("valid scope name")
}

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

/// Runs the analyzer on `root` with the declarative rules of `toml` and
/// returns the ALD003 / ALD004 violations and the scope graph.
///
/// Asserts on the way that the graph `analyze()` hands to project rules equals
/// `Analyzer::scope_graph()`, so every fixture checks that the two agree.
fn run(root: &Path, toml: &str) -> (Vec<Violation>, ScopeGraph) {
    let config: Arc<DeclarativeConfig> =
        Arc::new(declarative::load_config_from_toml(toml).expect("config should load"));
    let captured: Arc<Mutex<Option<ScopeGraph>>> = Arc::new(Mutex::new(None));
    let capture: ProjectRuleBox = Box::new(CaptureGraphRule {
        captured: Arc::clone(&captured),
    });

    let mut builder = Analyzer::builder()
        .root(root)
        .declarative(Arc::clone(&config))
        .project_rule_box(capture);
    for rule in declarative::create_rules_shared(&config) {
        builder = builder.rule_box(rule);
    }
    let analyzer = builder.build().expect("analyzer should build");
    let result = analyzer.analyze().expect("analysis should succeed");

    let analyzed = captured
        .lock()
        .expect("lock")
        .clone()
        .expect("project rule should have run");
    assert_eq!(
        analyzed,
        analyzer.scope_graph().expect("scope graph should build"),
        "analyze() and scope_graph() must agree"
    );

    let violations = result
        .violations
        .into_iter()
        .filter(|v| v.code == "ALD003" || v.code == "ALD004")
        .collect();
    (violations, analyzed)
}

fn count_code(violations: &[Violation], code: &str) -> usize {
    violations.iter().filter(|v| v.code == code).count()
}

/// The violation reported in `file`, which must be the only one there.
fn only_in<'a>(violations: &'a [Violation], file: &str) -> &'a Violation {
    let mut hits = violations
        .iter()
        .filter(|v| v.location.file == Path::new(file));
    let hit = hits
        .next()
        .unwrap_or_else(|| panic!("{file} is reported: {violations:#?}"));
    assert!(
        hits.next().is_none(),
        "{file} is reported once: {violations:#?}"
    );
    hit
}

/// A workspace analyzed from its virtual root: the references inside a member
/// resolve to the member's `src/`, so both the `use` and the inline path are
/// reported by ALD003 and ALD004.
#[test]
fn member_of_a_virtual_workspace_resolves_against_its_own_src() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    write(root, "crates/a-core/Cargo.toml", &package("a-core"));
    write(
        root,
        "crates/a-core/src/domain/mod.rs",
        "use crate::app::helper;\npub fn f() { crate::app::helper(); }\n",
    );
    write(root, "crates/a-core/src/app/mod.rs", "pub fn helper() {}\n");
    write(root, "crates/b-core/Cargo.toml", &package("b-core"));
    write(root, "crates/b-core/src/lib.rs", "pub fn b() {}\n");

    let toml = format!(
        "{}{}{}{}[[layering]]\nname = \"layers\"\norder = [\"app\", \"domain\", \"infra\"]\n\
         message = \"Layers only depend downward.\"\nseverity = \"error\"\n",
        scope("domain", "crates/a-core/src/domain/**"),
        scope("app", "crates/a-core/src/app/**"),
        scope("infra", "crates/a-core/src/infra/**"),
        deny("domain", "app"),
    );
    let (violations, graph) = run(root, &toml);

    assert_eq!(count_code(&violations, "ALD003"), 2, "{violations:#?}");
    assert_eq!(count_code(&violations, "ALD004"), 2, "{violations:#?}");
    assert!(
        violations
            .iter()
            .all(|v| v.location.file == Path::new("crates/a-core/src/domain/mod.rs")),
        "{violations:#?}"
    );

    let kinds: Vec<EdgeKind> = graph
        .sites(&name("domain"), &name("app"))
        .iter()
        .map(|s| s.kind)
        .collect();
    assert_eq!(kinds, [EdgeKind::Use, EdgeKind::Inline]);
}

/// A root manifest with both `[package]` and `[workspace]` is a package of its
/// own: its `src/` resolves against the root, a member's against the member.
#[test]
fn root_package_and_member_each_resolve_against_their_own_src() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        &format!(
            "{}\n[workspace]\nmembers = [\"crates/m\"]\n",
            package("root")
        ),
    );
    write(root, "crates/m/Cargo.toml", &package("m"));
    let code = "use crate::infra::db::Pool;\n";
    write(root, "src/domain/a.rs", code);
    write(root, "crates/m/src/domain/a.rs", code);

    let toml = format!(
        "{}{}{}{}{}{}",
        scope("root-domain", "src/domain/**"),
        scope("root-infra", "src/infra/**"),
        scope("m-domain", "crates/m/src/domain/**"),
        scope("m-infra", "crates/m/src/infra/**"),
        deny("root-domain", "root-infra"),
        deny("m-domain", "m-infra"),
    );
    let (violations, _) = run(root, &toml);

    assert_eq!(violations.len(), 2, "{violations:#?}");
    let root_hit = only_in(&violations, "src/domain/a.rs");
    assert!(
        root_hit.message.contains("scope `root-infra`"),
        "{}",
        root_hit.message
    );
    let member_hit = only_in(&violations, "crates/m/src/domain/a.rs");
    assert!(
        member_hit.message.contains("scope `m-infra`"),
        "{}",
        member_hit.message
    );
}

/// The longest package directory wins, and the match is by path component:
/// `crates/x-core` is not inside `crates/x`.
#[test]
fn nearest_package_wins_and_matching_is_by_component() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    for dir in ["crates/x", "crates/x-core", "crates/x/inner"] {
        write(root, &format!("{dir}/Cargo.toml"), &package("p"));
        write(
            root,
            &format!("{dir}/src/domain/a.rs"),
            "use crate::infra::db::Pool;\n",
        );
    }

    let toml = format!(
        "{}{}{}{}{}{}{}{}{}",
        scope("x-domain", "crates/x/src/domain/**"),
        scope("x-infra", "crates/x/src/infra/**"),
        scope("x-core-domain", "crates/x-core/src/domain/**"),
        scope("x-core-infra", "crates/x-core/src/infra/**"),
        scope("inner-domain", "crates/x/inner/src/domain/**"),
        scope("inner-infra", "crates/x/inner/src/infra/**"),
        deny("x-domain", "x-infra"),
        deny("x-core-domain", "x-core-infra"),
        deny("inner-domain", "inner-infra"),
    );
    let (violations, _) = run(root, &toml);

    assert_eq!(violations.len(), 3, "{violations:#?}");
    for (file, target) in [
        ("crates/x/src/domain/a.rs", "x-infra"),
        ("crates/x-core/src/domain/a.rs", "x-core-infra"),
        ("crates/x/inner/src/domain/a.rs", "inner-infra"),
    ] {
        let hit = only_in(&violations, file);
        assert!(
            hit.message.contains(&format!("scope `{target}`")),
            "{file}: {}",
            hit.message
        );
    }
}

/// `self::` and `super::` (inline `mod` blocks included) resolve against the
/// member, not the analyzer root.
#[test]
fn self_and_super_resolve_against_the_member() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    write(root, "crates/a-core/Cargo.toml", &package("a-core"));
    write(
        root,
        "crates/a-core/src/domain/service.rs",
        "use super::super::app::helper;\n\
         use self::inner::f;\n\
         mod wiring {\n    use super::super::super::app::other;\n    use self::deep::g;\n}\n",
    );

    let toml = format!(
        "{}{}{}",
        scope("domain", "crates/a-core/src/domain/**"),
        scope("app", "crates/a-core/src/app/**"),
        deny("domain", "app"),
    );
    let (violations, _) = run(root, &toml);

    // The two `super::` paths climb to the member's crate root and land in
    // `app`; the two `self::` paths stay in `domain`.
    assert_eq!(violations.len(), 2, "{violations:#?}");
    assert!(
        violations.iter().all(|v| v.message.contains("super::")),
        "{violations:#?}"
    );
}

/// A virtual manifest is not a crate root: files under it resolve against the
/// analyzer root, exactly as they do with no manifest at all.
#[test]
fn virtual_manifest_is_not_a_crate_root() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    write(root, "tools/Cargo.toml", "[workspace]\n");
    let code = "use crate::infra::db::Pool;\n";
    write(root, "src/domain/a.rs", code);
    write(root, "tools/src/domain/a.rs", code);

    let toml = format!(
        "{}{}{}{}{}{}",
        scope("domain", "src/domain/**"),
        scope("infra", "src/infra/**"),
        scope("tools-domain", "tools/src/domain/**"),
        scope("tools-infra", "tools/src/infra/**"),
        deny("domain", "infra"),
        deny("tools-domain", "tools-infra"),
    );
    let (violations, _) = run(root, &toml);

    // Under the root, `crate::infra` is `src/infra`. Under `tools/`, which is
    // not a package either, it is still `src/infra`, not `tools/src/infra`, so
    // the `tools-domain` rule has nothing to report.
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].location.file, Path::new("src/domain/a.rs"));
}

/// A manifest that does not parse is skipped, and its files resolve against
/// the enclosing package (here `crates/outer`, not the manifest's own
/// directory and not the analyzer root).
#[test]
fn unparsable_manifest_falls_back_to_the_enclosing_package() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    write(root, "crates/outer/Cargo.toml", &package("outer"));
    write(root, "crates/outer/broken/Cargo.toml", "[package\nname = ");
    write(
        root,
        "crates/outer/broken/src/domain/a.rs",
        "use crate::infra::db::Pool;\n",
    );

    let toml = format!(
        "{}{}{}",
        scope("broken-domain", "crates/outer/broken/src/domain/**"),
        scope("outer-infra", "crates/outer/src/infra/**"),
        deny("broken-domain", "outer-infra"),
    );
    let (violations, _) = run(root, &toml);

    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(
        violations[0].location.file,
        Path::new("crates/outer/broken/src/domain/a.rs")
    );
}

/// Both entry points build the same graph on a workspace, and it is not the
/// trivially-equal empty one.
#[test]
fn analyze_and_scope_graph_agree_on_a_workspace() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", VIRTUAL_ROOT);
    for member in ["a", "b"] {
        write(
            root,
            &format!("crates/{member}/Cargo.toml"),
            &package(member),
        );
        write(
            root,
            &format!("crates/{member}/src/domain/mod.rs"),
            "use crate::infra::db::Pool;\nuse super::infra::db::Conn;\n",
        );
    }

    let toml = format!(
        "{}{}{}{}",
        scope("a-domain", "crates/a/src/domain/**"),
        scope("a-infra", "crates/a/src/infra/**"),
        scope("b-domain", "crates/b/src/domain/**"),
        scope("b-infra", "crates/b/src/infra/**"),
    );
    // `run` asserts the two graphs are equal.
    let (_, graph) = run(root, &toml);

    assert_eq!(graph.sites(&name("a-domain"), &name("a-infra")).len(), 2);
    assert_eq!(graph.sites(&name("b-domain"), &name("b-infra")).len(), 2);
    assert!(!graph.has_edge(&name("a-domain"), &name("b-infra")));
}
