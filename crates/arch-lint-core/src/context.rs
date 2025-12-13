//! Context types for rule execution.

use std::path::{Path, PathBuf};

/// Context provided to per-file rules.
///
/// Contains metadata about the file being analyzed that rules can use
/// to make context-aware decisions (e.g., skip checks in test files).
#[derive(Debug, Clone)]
pub struct FileContext<'a> {
    /// Absolute path to the file.
    pub path: &'a Path,
    /// File contents as a string.
    pub content: &'a str,
    /// Whether this file is detected as a test file.
    pub is_test: bool,
    /// Module path from crate root (e.g., `["crate", "module", "submodule"]`).
    pub module_path: Vec<String>,
    /// Path relative to the project root.
    pub relative_path: PathBuf,
}

impl<'a> FileContext<'a> {
    /// Creates a new file context.
    #[must_use]
    pub fn new(path: &'a Path, content: &'a str, root: &Path) -> Self {
        let is_test = Self::detect_test_file(path);
        let relative_path = path
            .strip_prefix(root)
            .map_or_else(|_| path.to_path_buf(), Path::to_path_buf);
        let module_path = Self::compute_module_path(&relative_path);

        Self {
            path,
            content,
            is_test,
            module_path,
            relative_path,
        }
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

    /// Computes the module path from a relative file path.
    fn compute_module_path(relative_path: &Path) -> Vec<String> {
        let mut parts: Vec<String> = relative_path
            .with_extension("")
            .components()
            .filter_map(|c| {
                if let std::path::Component::Normal(s) = c {
                    s.to_str().map(String::from)
                } else {
                    None
                }
            })
            .collect();

        // Remove "mod" or "lib" from the path
        if let Some(last) = parts.last() {
            if last == "mod" || last == "lib" {
                parts.pop();
            }
        }

        // Prepend "crate" for the module path
        if !parts.is_empty() {
            parts.insert(0, "crate".to_string());
        }

        parts
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
/// Contains information about the project being analyzed.
#[derive(Debug, Clone)]
pub struct ProjectContext<'a> {
    /// Root directory of the project.
    pub root: &'a Path,
    /// List of all Rust source files in the project.
    pub source_files: Vec<PathBuf>,
    /// List of Cargo.toml files found.
    pub cargo_files: Vec<PathBuf>,
}

impl<'a> ProjectContext<'a> {
    /// Creates a new project context.
    #[must_use]
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            source_files: Vec::new(),
            cargo_files: Vec::new(),
        }
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

    #[test]
    fn test_module_path() {
        assert_eq!(
            FileContext::compute_module_path(Path::new("src/foo/bar.rs")),
            vec!["crate", "src", "foo", "bar"]
        );
        assert_eq!(
            FileContext::compute_module_path(Path::new("src/foo/mod.rs")),
            vec!["crate", "src", "foo"]
        );
    }

    #[test]
    fn test_offset_calculation() {
        let content = "line1\nline2\nline3";
        let ctx = FileContext {
            path: Path::new("test.rs"),
            content,
            is_test: false,
            module_path: vec![],
            relative_path: PathBuf::from("test.rs"),
        };

        assert_eq!(ctx.offset_for(1, 1), 0); // Start of line 1
        assert_eq!(ctx.offset_for(2, 1), 6); // Start of line 2
        assert_eq!(ctx.offset_for(2, 3), 8); // "ne" in line2
    }
}
