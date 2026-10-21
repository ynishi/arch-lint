//! Context types for rule execution.

use crate::crate_roots::CrateRoots;
use crate::declarative::graph::ScopeGraph;
use std::path::{Path, PathBuf};

/// Context provided to per-file rules.
///
/// Contains metadata about the file being analyzed that rules can use
/// to make context-aware decisions (e.g., skip checks in test files).
///
/// `#[non_exhaustive]`: fields stay `pub` for read access, but construct one
/// with [`FileContext::new`], not a struct literal, so a future field
/// addition is not a breaking change.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileContext<'a> {
    /// Absolute path to the file.
    pub path: &'a Path,
    /// File contents as a string.
    pub content: &'a str,
    /// Whether this file is detected as a test file.
    pub is_test: bool,
    /// Module path from the crate root of the package the file belongs to
    /// (e.g., `["crate", "module", "submodule"]`; `["crate"]` for the crate
    /// root file itself).
    ///
    /// `src/foo/bar.rs` is `["crate", "foo", "bar"]` and `src/foo/mod.rs` is
    /// `["crate", "foo"]`; `src/lib.rs` and `src/main.rs` are the crate root,
    /// while a nested `src/foo/lib.rs` is the module `foo::lib`.
    ///
    /// Empty means no crate-relative module path can be determined: the file
    /// is outside `src/`, or under `src/bin/`, each of which is its own crate
    /// root. See [`FileContext::crate_module`].
    pub module_path: Vec<String>,
    /// Path relative to the project root.
    pub relative_path: PathBuf,
    /// Directory of the package the file belongs to, relative to the project
    /// root; empty when it is the project root itself.
    crate_root: PathBuf,
}

impl<'a> FileContext<'a> {
    /// Creates a new file context.
    ///
    /// The file is taken to belong to the package at `root`, so
    /// [`FileContext::crate_root`] is empty and [`FileContext::module_path`]
    /// is computed from the path relative to `root`. The analyzer places the
    /// file in the package it actually belongs to when it runs the rules.
    #[must_use]
    pub fn new(path: &'a Path, content: &'a str, root: &Path) -> Self {
        let is_test = Self::detect_test_file(path);
        let relative_path = path
            .strip_prefix(root)
            .map_or_else(|_| path.to_path_buf(), Path::to_path_buf);
        let module_path = Self::compute_module_path(&relative_path, Path::new(""));

        Self {
            path,
            content,
            is_test,
            module_path,
            relative_path,
            crate_root: PathBuf::new(),
        }
    }

    /// Places the file in the package at the root `roots` finds for it and
    /// recomputes [`FileContext::module_path`] relative to that package.
    #[must_use]
    pub(crate) fn with_crate_roots(mut self, roots: &CrateRoots) -> Self {
        let crate_root = roots.crate_root_for(&self.relative_path);
        // `new` already computed the module path for the analyzer root.
        if !crate_root.as_os_str().is_empty() {
            self.crate_root = crate_root.to_path_buf();
            self.module_path = Self::compute_module_path(&self.relative_path, &self.crate_root);
        }
        self
    }

    /// Directory of the package the file belongs to, relative to the project
    /// root; empty when the package is the project root itself (or the file
    /// belongs to no package found under it).
    #[must_use]
    pub fn crate_root(&self) -> &Path {
        &self.crate_root
    }

    /// [`FileContext::module_path`] without the leading `crate`, e.g.
    /// `["foo", "bar"]` for `crate::foo::bar`; empty for the crate root file.
    ///
    /// `None` when the file has no crate-relative module path (see
    /// [`FileContext::module_path`]).
    #[must_use]
    pub fn crate_module(&self) -> Option<&[String]> {
        self.module_path.split_first().map(|(_, module)| module)
    }

    /// Detects if a file is a test file based on path conventions.
    fn detect_test_file(path: &Path) -> bool {
        // Check path components for test directories
        for component in path.components() {
            if let std::path::Component::Normal(s) = component {
                let s = s.to_string_lossy();
                if s == "tests" || s == "test" || s == "benches" {
                    return true;
                }
            }
        }

        // Check file name patterns
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.ends_with("_test.rs")
                || file_name.ends_with("_tests.rs")
                || file_name.starts_with("test_")
                || file_name == "tests.rs"
            {
                return true;
            }
        }

        false
    }

    /// Computes the module path of the file at `relative_path` within the
    /// package at `crate_root` (both relative to the project root).
    ///
    /// Empty for a file outside the package's `src/` or under `src/bin/`:
    /// each file under `src/bin/`, like one under `tests/` or `benches/`, is
    /// its own crate root (there can be several `src/bin/*.rs` binaries in
    /// one package), so there is no single module path for it.
    ///
    /// A trailing `mod` segment is always dropped (`src/a/mod.rs` is
    /// `crate::a`: the file names a submodule of its parent directory, not
    /// itself). `lib` or `main` is dropped only when it is the *sole*
    /// remaining segment (`src/lib.rs` / `src/main.rs` are the crate root): a
    /// nested `src/foo/lib.rs` or `src/cli/main.rs` is a real submodule that
    /// happens to be named `lib` / `main`, and keeps its segment.
    fn compute_module_path(relative_path: &Path, crate_root: &Path) -> Vec<String> {
        let in_crate = relative_path
            .strip_prefix(crate_root)
            .unwrap_or(relative_path);
        let mut segments: Vec<String> = in_crate
            .with_extension("")
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str().map(String::from),
                _ => None,
            })
            .collect();

        if segments.first().map(String::as_str) != Some("src") {
            return Vec::new();
        }
        segments.remove(0);

        if segments.first().map(String::as_str) == Some("bin") {
            return Vec::new();
        }

        let drop_last = segments.last().map(String::as_str) == Some("mod")
            || (segments.len() == 1 && matches!(segments[0].as_str(), "lib" | "main"));
        if drop_last {
            segments.pop();
        }

        segments.insert(0, "crate".to_string());
        segments
    }

    /// Calculates byte offset for a given line and column.
    ///
    /// # Arguments
    ///
    /// * `line` - 1-indexed line number
    /// * `column` - 1-indexed column number
    ///
    /// # Returns
    ///
    /// Byte offset from the start of the file, or 0 if out of bounds.
    #[must_use]
    pub fn offset_for(&self, line: usize, column: usize) -> usize {
        if line == 0 {
            return 0;
        }

        let mut offset = 0;
        for (i, line_content) in self.content.lines().enumerate() {
            if i + 1 == line {
                return offset + column.saturating_sub(1);
            }
            offset += line_content.len() + 1; // +1 for newline
        }

        offset
    }
}

/// Context provided to project-wide rules.
///
/// Contains information about the project being analyzed: the files the
/// analyzer discovered and the [`ScopeGraph`] it collected while running the
/// per-file rules. The graph is empty when the analyzer was built without a
/// declarative config (see
/// [`AnalyzerBuilder::declarative`](crate::AnalyzerBuilder::declarative)) or
/// the config declares no `[[scopes]]`.
///
/// `#[non_exhaustive]`: fields stay `pub` for read access, but construct one
/// with [`ProjectContext::new`] and the `with_*` builders, not a struct
/// literal, so a future field addition is not a breaking change.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProjectContext<'a> {
    /// Root directory of the project.
    pub root: &'a Path,
    /// List of all Rust source files in the project.
    pub source_files: Vec<PathBuf>,
    /// List of Cargo.toml files found.
    pub cargo_files: Vec<PathBuf>,
    /// Dependency graph between the declared scopes.
    pub scope_graph: ScopeGraph,
}

impl<'a> ProjectContext<'a> {
    /// Creates a new project context.
    #[must_use]
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            source_files: Vec::new(),
            cargo_files: Vec::new(),
            scope_graph: ScopeGraph::new(),
        }
    }

    /// Sets the scope dependency graph.
    #[must_use]
    pub fn with_scope_graph(mut self, graph: ScopeGraph) -> Self {
        self.scope_graph = graph;
        self
    }

    /// Returns the scope dependency graph collected for this project.
    #[must_use]
    pub fn scope_graph(&self) -> &ScopeGraph {
        &self.scope_graph
    }

    /// Sets the list of source files.
    #[must_use]
    pub fn with_source_files(mut self, files: Vec<PathBuf>) -> Self {
        self.source_files = files;
        self
    }

    /// Sets the list of Cargo.toml files.
    #[must_use]
    pub fn with_cargo_files(mut self, files: Vec<PathBuf>) -> Self {
        self.cargo_files = files;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_test_file() {
        assert!(FileContext::detect_test_file(Path::new("src/tests/foo.rs")));
        assert!(FileContext::detect_test_file(Path::new(
            "tests/integration.rs"
        )));
        assert!(FileContext::detect_test_file(Path::new("src/foo_test.rs")));
        assert!(FileContext::detect_test_file(Path::new("src/test_foo.rs")));
        assert!(!FileContext::detect_test_file(Path::new("src/foo.rs")));
        assert!(!FileContext::detect_test_file(Path::new("src/lib.rs")));
    }

    fn module_path(relative_path: &str, crate_root: &str) -> Vec<String> {
        FileContext::compute_module_path(Path::new(relative_path), Path::new(crate_root))
    }

    #[test]
    fn test_module_path() {
        assert_eq!(
            module_path("src/foo/bar.rs", ""),
            vec!["crate", "foo", "bar"]
        );
        assert_eq!(module_path("src/foo/mod.rs", ""), vec!["crate", "foo"]);
        assert_eq!(
            module_path("src/domain/mod.rs", ""),
            vec!["crate", "domain"]
        );
    }

    #[test]
    fn test_module_path_crate_root_files() {
        assert_eq!(module_path("src/lib.rs", ""), vec!["crate"]);
        assert_eq!(module_path("src/main.rs", ""), vec!["crate"]);
    }

    /// `lib` / `main` are only the crate root when they are the *only*
    /// segment; a nested file genuinely named `lib.rs` / `main.rs` is a
    /// submodule and keeps its segment.
    #[test]
    fn test_module_path_nested_lib_and_main_keep_their_segment() {
        assert_eq!(
            module_path("src/foo/lib.rs", ""),
            vec!["crate", "foo", "lib"]
        );
        assert_eq!(
            module_path("src/cli/main.rs", ""),
            vec!["crate", "cli", "main"]
        );
    }

    #[test]
    fn test_module_path_outside_src_is_empty() {
        assert!(module_path("test.rs", "").is_empty());
        assert!(module_path("tests/x.rs", "").is_empty());
        assert!(module_path("benches/b.rs", "").is_empty());
        assert!(module_path("build.rs", "").is_empty());
    }

    #[test]
    fn test_module_path_src_bin_is_empty() {
        assert!(module_path("src/bin/foo.rs", "").is_empty());
        assert!(module_path("src/bin/foo/main.rs", "").is_empty());
    }

    #[test]
    fn test_module_path_is_relative_to_the_crate_root() {
        assert_eq!(
            module_path("crates/a/src/foo/bar.rs", "crates/a"),
            vec!["crate", "foo", "bar"]
        );
        assert_eq!(
            module_path("crates/a/src/foo/mod.rs", "crates/a"),
            vec!["crate", "foo"]
        );
        assert_eq!(
            module_path("crates/a/src/lib.rs", "crates/a"),
            vec!["crate"]
        );
        assert!(module_path("crates/a/tests/it.rs", "crates/a").is_empty());
        assert!(module_path("crates/a/src/bin/x.rs", "crates/a").is_empty());
        // Without the crate root the same path is outside `src/`.
        assert!(module_path("crates/a/src/foo/bar.rs", "").is_empty());
    }

    #[test]
    fn test_with_crate_roots_recomputes_the_module_path() {
        let roots = CrateRoots::new([PathBuf::from("crates/a")]);
        let path = Path::new("/ws/crates/a/src/domain/mod.rs");
        let ctx = FileContext::new(path, "", Path::new("/ws"));
        assert!(ctx.module_path.is_empty());
        assert_eq!(ctx.crate_root(), Path::new(""));

        let ctx = ctx.with_crate_roots(&roots);
        assert_eq!(ctx.crate_root(), Path::new("crates/a"));
        assert_eq!(ctx.module_path, vec!["crate", "domain"]);
        assert_eq!(
            ctx.crate_module().map(<[String]>::to_vec),
            Some(vec!["domain".to_string()])
        );
    }

    #[test]
    fn test_crate_module() {
        let ctx = crate::test_utils::file_ctx("src/lib.rs", "", false);
        assert_eq!(ctx.crate_module(), Some(&[][..]));
        let ctx = crate::test_utils::file_ctx("test.rs", "", false);
        assert_eq!(ctx.crate_module(), None);
    }

    #[test]
    fn test_offset_calculation() {
        let content = "line1\nline2\nline3";
        let ctx = crate::test_utils::file_ctx("test.rs", content, false);

        assert_eq!(ctx.offset_for(1, 1), 0); // Start of line 1
        assert_eq!(ctx.offset_for(2, 1), 6); // Start of line 2
        assert_eq!(ctx.offset_for(2, 3), 8); // "ne" in line2
    }
}
