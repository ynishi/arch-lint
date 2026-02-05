//! Language-agnostic extraction types and trait.
//!
//! `LanguageExtractor` is the extension point for adding new languages.
//! Implement it to teach arch-lint-ts how to extract imports, declarations,
//! and package info from a new language via Tree-sitter.

use std::path::PathBuf;

/// Package/module declaration extracted from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// Line number (1-indexed).
    pub line: usize,
    /// Fully qualified package path (e.g., `com.example.domain.model`).
    pub path: String,
}

/// A single import statement extracted from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    /// Line number (1-indexed).
    pub line: usize,
    /// Column (0-indexed byte offset within line).
    pub column: usize,
    /// Fully qualified import path (e.g., `com.example.infra.db.UserRepository`).
    pub path: String,
}

/// Kind of declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    /// `class Foo`
    Class,
    /// `data class Foo`
    DataClass,
    /// `sealed class Foo`
    SealedClass,
    /// `enum class Foo`
    EnumClass,
    /// `interface Foo`
    Interface,
    /// `object Foo`
    Object,
    /// `fun foo()`
    Function,
}

/// A declaration (class, interface, object, function) extracted from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclInfo {
    /// Line number (1-indexed).
    pub line: usize,
    /// Identifier name.
    pub name: String,
    /// Kind of declaration.
    pub kind: DeclKind,
    /// Package this declaration belongs to.
    pub package: String,
}

/// Result of analyzing a single source file with Tree-sitter.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    /// Path relative to project root.
    pub file_path: PathBuf,
    /// Package declaration, if present.
    pub package: Option<PackageInfo>,
    /// All import statements found.
    pub imports: Vec<ImportInfo>,
    /// All top-level declarations found.
    pub declarations: Vec<DeclInfo>,
}

/// Trait for language-specific Tree-sitter extraction.
///
/// Implement this to add support for a new language.
/// The extractor receives raw source text and returns a [`FileAnalysis`]
/// containing the language-agnostic intermediate representation.
pub trait LanguageExtractor: Send + Sync {
    /// Language identifier (e.g., `"kotlin"`, `"go"`).
    fn language_id(&self) -> &'static str;

    /// File extensions this extractor handles (e.g., `&[".kt", ".kts"]`).
    fn extensions(&self) -> &'static [&'static str];

    /// Extract imports, declarations, and package info from source code.
    fn analyze(&self, source: &str) -> FileAnalysis;
}
