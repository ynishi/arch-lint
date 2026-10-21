//! Package roots of the analyzed project.
//!
//! A file's `crate::`, `self::` and `super::` paths are relative to the
//! package it belongs to, not to the analyzer root: in a Cargo workspace
//! analyzed from its root, `crate::app` inside `crates/foo/src/domain/mod.rs`
//! names `crates/foo/src/app`. [`CrateRoots`] holds the directories of the
//! packages found under the analyzer root and answers which one a file
//! belongs to.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The root-relative directories of the packages under the analyzer root.
///
/// A directory is a package when its `Cargo.toml` has a `[package]` table
/// (see [`manifest_declares_package`]); a virtual manifest (`[workspace]`
/// only) is not one. [`PathBuf::new`] (the empty path) stands for the analyzer
/// root itself.
#[derive(Debug, Clone, Default)]
pub(crate) struct CrateRoots {
    dirs: HashSet<PathBuf>,
}

impl CrateRoots {
    /// Creates the set from root-relative package directories.
    pub(crate) fn new(dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            dirs: dirs.into_iter().collect(),
        }
    }

    /// Returns the root-relative directory of the package `relative_path`
    /// belongs to: the longest package directory that is an ancestor of it.
    ///
    /// Matching is by path component, so a package at `crates/x` does not
    /// contain `crates/x-core/src/a.rs`. Returns the empty path (the analyzer
    /// root) when no package contains the file.
    ///
    /// Walks the ancestors of `relative_path` from the nearest one, so the
    /// cost is the path's depth, not the number of packages.
    pub(crate) fn crate_root_for(&self, relative_path: &Path) -> &Path {
        relative_path
            .ancestors()
            .skip(1)
            .find_map(|ancestor| self.dirs.get(ancestor))
            .map_or(Path::new(""), PathBuf::as_path)
    }
}

/// Returns whether the `Cargo.toml` text in `content` declares a package, i.e.
/// has a `[package]` table.
///
/// A `[workspace]`-only (virtual) manifest returns `false`; a manifest with
/// both tables returns `true`.
///
/// # Errors
///
/// Returns the parse error when `content` is not valid TOML.
pub(crate) fn manifest_declares_package(content: &str) -> Result<bool, toml::de::Error> {
    let manifest: toml::Table = content.parse()?;
    Ok(manifest.get("package").is_some_and(toml::Value::is_table))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(dirs: &[&str]) -> CrateRoots {
        CrateRoots::new(dirs.iter().map(PathBuf::from))
    }

    #[test]
    fn longest_matching_package_wins() {
        let roots = roots(&["", "crates/x", "crates/x/inner"]);
        assert_eq!(
            roots.crate_root_for(Path::new("crates/x/inner/src/a.rs")),
            Path::new("crates/x/inner")
        );
        assert_eq!(
            roots.crate_root_for(Path::new("crates/x/src/a.rs")),
            Path::new("crates/x")
        );
        assert_eq!(roots.crate_root_for(Path::new("src/a.rs")), Path::new(""));
    }

    #[test]
    fn match_is_by_component_not_by_prefix() {
        let roots = roots(&["crates/x"]);
        assert_eq!(
            roots.crate_root_for(Path::new("crates/x-core/src/a.rs")),
            Path::new("")
        );
    }

    #[test]
    fn no_match_is_the_analyzer_root() {
        assert_eq!(
            roots(&["crates/a"]).crate_root_for(Path::new("src/a.rs")),
            Path::new("")
        );
        assert_eq!(
            CrateRoots::default().crate_root_for(Path::new("src/a.rs")),
            Path::new("")
        );
    }

    #[test]
    fn manifest_with_package_declares_one() {
        assert!(manifest_declares_package("[package]\nname = \"a\"\n").unwrap());
    }

    #[test]
    fn manifest_with_package_and_workspace_declares_one() {
        let content = "[package]\nname = \"a\"\n\n[workspace]\nmembers = [\"crates/*\"]\n";
        assert!(manifest_declares_package(content).unwrap());
    }

    #[test]
    fn virtual_manifest_declares_none() {
        assert!(!manifest_declares_package("[workspace]\nmembers = [\"crates/*\"]\n").unwrap());
    }

    #[test]
    fn invalid_manifest_is_an_error() {
        assert!(manifest_declares_package("[package\nname =").is_err());
    }
}
