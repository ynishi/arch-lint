//! Kotlin language extractor using Tree-sitter.

use std::path::PathBuf;
use tree_sitter::{Language, Node, Parser};

use crate::extractor::{
    DeclInfo, DeclKind, FileAnalysis, ImportInfo, LanguageExtractor, PackageInfo,
};

/// Extracts imports, classes, and package declarations from Kotlin source.
pub struct KotlinExtractor {
    language: Language,
}

impl KotlinExtractor {
    /// Creates a new Kotlin extractor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: tree_sitter_kotlin_ng::LANGUAGE.into(),
        }
    }

    fn text<'a>(node: &Node<'_>, src: &'a [u8]) -> &'a str {
        std::str::from_utf8(&src[node.start_byte()..node.end_byte()]).unwrap_or("")
    }

    /// Join identifier children of a `qualified_identifier` node with dots.
    fn qualified_id(node: &Node<'_>, src: &[u8]) -> String {
        let mut parts = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                parts.push(Self::text(&child, src).to_owned());
            }
        }
        parts.join(".")
    }

    fn extract_package(node: &Node<'_>, src: &[u8]) -> Option<PackageInfo> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "qualified_identifier" {
                return Some(PackageInfo {
                    line: node.start_position().row + 1,
                    path: Self::qualified_id(&child, src),
                });
            }
        }
        None
    }

    fn extract_import(node: &Node<'_>, src: &[u8]) -> Option<ImportInfo> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "qualified_identifier" {
                return Some(ImportInfo {
                    line: node.start_position().row + 1,
                    column: node.start_position().column,
                    path: Self::qualified_id(&child, src),
                });
            }
        }
        None
    }

    fn classify_declaration(node: &Node<'_>, src: &[u8]) -> DeclKind {
        if node.kind() == "object_declaration" {
            return DeclKind::Object;
        }

        let mut has_interface = false;
        let mut modifiers: Vec<String> = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "interface" {
                has_interface = true;
            } else if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for mod_child in child.children(&mut mod_cursor) {
                    if mod_child.kind() == "class_modifier" {
                        if let Some(inner) = mod_child.child(0) {
                            modifiers.push(Self::text(&inner, src).to_owned());
                        }
                    }
                }
            }
        }

        if has_interface {
            DeclKind::Interface
        } else if modifiers.iter().any(|m| m == "data") {
            DeclKind::DataClass
        } else if modifiers.iter().any(|m| m == "sealed") {
            DeclKind::SealedClass
        } else if modifiers.iter().any(|m| m == "enum") {
            DeclKind::EnumClass
        } else {
            DeclKind::Class
        }
    }

    fn extract_declaration(
        node: &Node<'_>,
        src: &[u8],
        package: &Option<PackageInfo>,
    ) -> Option<DeclInfo> {
        let kind = Self::classify_declaration(node, src);

        let mut name = None;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                name = Some(Self::text(&child, src).to_owned());
                break;
            }
        }

        let name = name?;
        let pkg = package.as_ref().map_or(String::new(), |p| p.path.clone());

        Some(DeclInfo {
            line: node.start_position().row + 1,
            name,
            kind,
            package: pkg,
        })
    }
}

impl Default for KotlinExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageExtractor for KotlinExtractor {
    fn language_id(&self) -> &'static str {
        "kotlin"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &[".kt", ".kts"]
    }

    fn analyze(&self, source: &str) -> FileAnalysis {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .expect("failed to set kotlin language");

        let src = source.as_bytes();
        let tree = parser.parse(src, None).expect("failed to parse");
        let root = tree.root_node();

        let mut result = FileAnalysis {
            file_path: PathBuf::new(),
            package: None,
            imports: Vec::new(),
            declarations: Vec::new(),
        };

        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            match node.kind() {
                "package_header" => {
                    result.package = Self::extract_package(&node, src);
                }
                "import" => {
                    if let Some(imp) = Self::extract_import(&node, src) {
                        result.imports.push(imp);
                    }
                }
                "class_declaration" | "object_declaration" => {
                    if let Some(decl) = Self::extract_declaration(&node, src, &result.package) {
                        result.declarations.push(decl);
                    }
                }
                _ => {}
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(src: &str) -> FileAnalysis {
        KotlinExtractor::new().analyze(src)
    }

    #[test]
    fn extracts_package() {
        let a = analyze("package com.example.domain.model\n");
        assert_eq!(
            a.package.as_ref().map(|p| p.path.as_str()),
            Some("com.example.domain.model")
        );
    }

    #[test]
    fn extracts_imports() {
        let a = analyze(
            "package com.example.app\nimport com.example.domain.User\nimport com.example.infra.Repo\n",
        );
        assert_eq!(a.imports.len(), 2);
        assert_eq!(a.imports[0].path, "com.example.domain.User");
        assert_eq!(a.imports[1].path, "com.example.infra.Repo");
    }

    #[test]
    fn extracts_class() {
        let a = analyze("package com.example.domain\nclass User(val id: Long)\n");
        assert_eq!(a.declarations.len(), 1);
        assert_eq!(a.declarations[0].name, "User");
        assert_eq!(a.declarations[0].kind, DeclKind::Class);
    }

    #[test]
    fn extracts_data_class() {
        let a = analyze("package com.example.domain\ndata class UserDto(val id: Long)\n");
        assert_eq!(a.declarations[0].kind, DeclKind::DataClass);
    }

    #[test]
    fn extracts_interface() {
        let a = analyze("package com.example.domain\ninterface UserRepository { }\n");
        assert_eq!(a.declarations[0].kind, DeclKind::Interface);
        assert_eq!(a.declarations[0].name, "UserRepository");
    }

    #[test]
    fn extracts_object() {
        let a = analyze("package com.example.domain\nobject Factory { }\n");
        assert_eq!(a.declarations[0].kind, DeclKind::Object);
    }

    #[test]
    fn empty_source() {
        let a = analyze("");
        assert!(a.package.is_none());
        assert!(a.imports.is_empty());
        assert!(a.declarations.is_empty());
    }

    #[test]
    fn package_propagated_to_declarations() {
        let a = analyze("package com.example.infra.db\nclass RepoImpl { }\n");
        assert_eq!(a.declarations[0].package, "com.example.infra.db");
    }
}
