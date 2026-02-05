//! Architecture rule engine.
//!
//! Evaluates layer dependency rules and pattern constraints
//! against a [`FileAnalysis`], producing [`Violation`]s from arch-lint-core.

use arch_lint_core::{Location, Severity, Violation};

use crate::config::ArchConfig;
use crate::extractor::FileAnalysis;
use crate::layer::LayerResolver;

/// Evaluates architecture rules against extracted file analysis.
pub struct ArchRuleEngine {
    config: ArchConfig,
    resolver: LayerResolver,
}

impl ArchRuleEngine {
    /// Create a new engine from config.
    #[must_use]
    pub fn new(config: ArchConfig) -> Self {
        let resolver = LayerResolver::new(&config);
        Self { config, resolver }
    }

    /// Check a single file analysis for architecture violations.
    #[must_use]
    pub fn check(&self, analysis: &FileAnalysis) -> Vec<Violation> {
        let mut violations = Vec::new();
        violations.extend(self.check_layer_deps(analysis));
        violations.extend(self.check_constraints(analysis));
        violations.extend(self.check_naming_rules(analysis));
        violations
    }

    fn check_layer_deps(&self, analysis: &FileAnalysis) -> Vec<Violation> {
        let package = match &analysis.package {
            Some(p) => &p.path,
            None => return Vec::new(),
        };

        let from_layer = match self.resolver.resolve(package) {
            Some(l) => l,
            None => return Vec::new(),
        };

        let allowed = self
            .config
            .dependencies
            .get(from_layer)
            .cloned()
            .unwrap_or_default();

        let mut violations = Vec::new();

        for imp in &analysis.imports {
            let to_layer = match self.resolver.resolve(&imp.path) {
                Some(l) => l,
                None => continue,
            };

            if to_layer == from_layer {
                continue;
            }

            if !allowed.iter().any(|a| a == to_layer) {
                violations.push(Violation::new(
                    "LAYER001",
                    "layer-dependency",
                    Severity::Error,
                    Location::new(analysis.file_path.clone(), imp.line, imp.column + 1),
                    format!("{from_layer} -> {to_layer} dependency not allowed"),
                ));
            }
        }

        violations
    }

    fn check_naming_rules(&self, analysis: &FileAnalysis) -> Vec<Violation> {
        let from_layer = match analysis
            .package
            .as_ref()
            .and_then(|p| self.resolver.resolve(&p.path))
        {
            Some(l) => l.to_owned(),
            None => return Vec::new(),
        };

        let decl_names: Vec<&str> = analysis
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();

        let mut violations = Vec::new();

        for constraint in &self.config.constraints {
            if constraint.kind != "naming-rule" {
                continue;
            }
            if !constraint.in_layers.iter().any(|l| l == &from_layer) {
                continue;
            }
            if constraint.import_matches.is_empty() {
                continue;
            }

            for imp in &analysis.imports {
                if !imp.path.contains(&constraint.import_matches) {
                    continue;
                }

                // source_must_match: at least one declaration must contain the substring
                if !constraint.source_must_match.is_empty()
                    && !decl_names
                        .iter()
                        .any(|n| n.contains(&constraint.source_must_match))
                {
                    violations.push(Violation::new(
                        "NAMING001",
                        "naming-rule",
                        constraint.severity,
                        Location::new(analysis.file_path.clone(), imp.line, imp.column + 1),
                        &constraint.message,
                    ));
                }

                // source_must_not_match: no declaration should contain the substring
                if !constraint.source_must_not_match.is_empty()
                    && decl_names
                        .iter()
                        .any(|n| n.contains(&constraint.source_must_not_match))
                {
                    violations.push(Violation::new(
                        "NAMING001",
                        "naming-rule",
                        constraint.severity,
                        Location::new(analysis.file_path.clone(), imp.line, imp.column + 1),
                        &constraint.message,
                    ));
                }
            }
        }

        violations
    }

    fn check_constraints(&self, analysis: &FileAnalysis) -> Vec<Violation> {
        let package = match &analysis.package {
            Some(p) => &p.path,
            None => return Vec::new(),
        };

        let from_layer = match self.resolver.resolve(package) {
            Some(l) => l.to_owned(),
            None => return Vec::new(),
        };

        let mut violations = Vec::new();

        for constraint in &self.config.constraints {
            if constraint.kind != "no-import-pattern" {
                continue;
            }
            if !constraint.in_layers.iter().any(|l| l == &from_layer) {
                continue;
            }

            for imp in &analysis.imports {
                if imp.path.contains(&constraint.pattern) {
                    violations.push(Violation::new(
                        "PATTERN001",
                        "import-pattern",
                        constraint.severity,
                        Location::new(analysis.file_path.clone(), imp.line, imp.column + 1),
                        &constraint.message,
                    ));
                }
            }
        }

        violations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ArchConfig, Constraint, LayerDef};
    use crate::extractor::{FileAnalysis, ImportInfo, PackageInfo};
    use std::path::PathBuf;

    fn test_config() -> ArchConfig {
        ArchConfig {
            root: ".".into(),
            exclude: vec![],
            layers: vec![
                LayerDef {
                    name: "domain".into(),
                    packages: vec!["com.example.domain".into()],
                },
                LayerDef {
                    name: "app".into(),
                    packages: vec!["com.example.app".into()],
                },
                LayerDef {
                    name: "infra".into(),
                    packages: vec!["com.example.infra".into()],
                },
            ],
            dependencies: [
                ("domain".into(), vec![]),
                ("app".into(), vec!["domain".into()]),
                ("infra".into(), vec!["domain".into(), "app".into()]),
            ]
            .into_iter()
            .collect(),
            constraints: vec![],
        }
    }

    fn make_analysis(pkg: &str, imports: &[&str]) -> FileAnalysis {
        FileAnalysis {
            file_path: PathBuf::from("test.kt"),
            package: Some(PackageInfo {
                line: 1,
                path: pkg.into(),
            }),
            imports: imports
                .iter()
                .enumerate()
                .map(|(i, p)| ImportInfo {
                    line: i + 2,
                    column: 0,
                    path: (*p).into(),
                })
                .collect(),
            declarations: vec![],
        }
    }

    #[test]
    fn allows_valid_dependency() {
        let engine = ArchRuleEngine::new(test_config());
        let a = make_analysis("com.example.app.service", &["com.example.domain.User"]);
        assert!(engine.check(&a).is_empty());
    }

    #[test]
    fn detects_forbidden_dependency() {
        let engine = ArchRuleEngine::new(test_config());
        let a = make_analysis("com.example.domain.model", &["com.example.infra.db.Repo"]);
        let v = engine.check(&a);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "LAYER001");
        assert!(v[0].message.contains("domain -> infra"));
    }

    #[test]
    fn same_layer_import_is_ok() {
        let engine = ArchRuleEngine::new(test_config());
        let a = make_analysis(
            "com.example.domain.model",
            &["com.example.domain.event.Created"],
        );
        assert!(engine.check(&a).is_empty());
    }

    #[test]
    fn unknown_import_target_is_ok() {
        let engine = ArchRuleEngine::new(test_config());
        let a = make_analysis("com.example.domain.model", &["kotlinx.coroutines.Flow"]);
        assert!(engine.check(&a).is_empty());
    }

    #[test]
    fn no_package_skips_check() {
        let engine = ArchRuleEngine::new(test_config());
        let a = FileAnalysis {
            file_path: PathBuf::from("script.kt"),
            package: None,
            imports: vec![ImportInfo {
                line: 1,
                column: 0,
                path: "com.example.infra.Foo".into(),
            }],
            declarations: vec![],
        };
        assert!(engine.check(&a).is_empty());
    }

    fn make_pattern_constraint(pattern: &str, in_layers: &[&str], message: &str) -> Constraint {
        Constraint {
            kind: "no-import-pattern".into(),
            pattern: pattern.into(),
            in_layers: in_layers.iter().map(|s| (*s).into()).collect(),
            severity: Severity::Warning,
            message: message.into(),
            import_matches: String::new(),
            source_must_match: String::new(),
            source_must_not_match: String::new(),
        }
    }

    fn make_naming_constraint(
        import_matches: &str,
        source_must_match: &str,
        source_must_not_match: &str,
        in_layers: &[&str],
        message: &str,
    ) -> Constraint {
        Constraint {
            kind: "naming-rule".into(),
            pattern: String::new(),
            in_layers: in_layers.iter().map(|s| (*s).into()).collect(),
            severity: Severity::Error,
            message: message.into(),
            import_matches: import_matches.into(),
            source_must_match: source_must_match.into(),
            source_must_not_match: source_must_not_match.into(),
        }
    }

    fn make_analysis_with_decls(pkg: &str, imports: &[&str], decl_names: &[&str]) -> FileAnalysis {
        use crate::extractor::{DeclInfo, DeclKind};
        FileAnalysis {
            file_path: PathBuf::from("test.kt"),
            package: Some(PackageInfo {
                line: 1,
                path: pkg.into(),
            }),
            imports: imports
                .iter()
                .enumerate()
                .map(|(i, p)| ImportInfo {
                    line: i + 2,
                    column: 0,
                    path: (*p).into(),
                })
                .collect(),
            declarations: decl_names
                .iter()
                .enumerate()
                .map(|(i, n)| DeclInfo {
                    line: i + 10,
                    name: (*n).into(),
                    kind: DeclKind::Class,
                    package: pkg.into(),
                })
                .collect(),
        }
    }

    #[test]
    fn pattern_constraint_triggers() {
        let mut config = test_config();
        config.constraints.push(make_pattern_constraint(
            "java.sql",
            &["domain"],
            "No JDBC in domain",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis("com.example.domain.model", &["java.sql.Connection"]);
        let v = engine.check(&a);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "PATTERN001");
        assert_eq!(v[0].severity, Severity::Warning);
    }

    #[test]
    fn pattern_constraint_ignores_other_layers() {
        let mut config = test_config();
        config.constraints.push(make_pattern_constraint(
            "java.sql",
            &["domain"],
            "No JDBC in domain",
        ));

        let engine = ArchRuleEngine::new(config);
        // infra layer using java.sql is fine
        let a = make_analysis("com.example.infra.db", &["java.sql.Connection"]);
        assert!(engine.check(&a).is_empty());
    }

    // --- naming-rule tests ---

    /// Config that allows app → infra (for testing naming rules in isolation)
    fn test_config_with_infra() -> ArchConfig {
        let mut config = test_config();
        config
            .dependencies
            .get_mut("app")
            .unwrap()
            .push("infra".into());
        config
    }

    #[test]
    fn naming_rule_source_must_match_allows_service() {
        // UserService importing UserRepositoryImpl → OK (Service can use Repository)
        let mut config = test_config_with_infra();
        config.constraints.push(make_naming_constraint(
            "RepositoryImpl",
            "Service",
            "",
            &["app"],
            "Only Service can import RepositoryImpl",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis_with_decls(
            "com.example.app.service",
            &["com.example.infra.db.UserRepositoryImpl"],
            &["UserService"],
        );
        assert!(engine.check(&a).is_empty());
    }

    #[test]
    fn naming_rule_source_must_match_rejects_non_service() {
        // OrderController importing UserRepositoryImpl → VIOLATION (not a Service)
        let mut config = test_config_with_infra();
        config.constraints.push(make_naming_constraint(
            "RepositoryImpl",
            "Service",
            "",
            &["app"],
            "Only Service can import RepositoryImpl",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis_with_decls(
            "com.example.app.handler",
            &["com.example.infra.db.UserRepositoryImpl"],
            &["OrderController"],
        );
        let v = engine.check(&a);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "NAMING001");
        assert!(v[0].message.contains("Only Service"));
    }

    #[test]
    fn naming_rule_source_must_not_match() {
        // UseCase importing another UseCase → VIOLATION
        let mut config = test_config();
        config.constraints.push(make_naming_constraint(
            "UseCase",
            "",
            "UseCase",
            &["app"],
            "UseCase should not depend on other UseCases",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis_with_decls(
            "com.example.app.usecase",
            &["com.example.app.usecase.CreateUserUseCase"],
            &["DeleteUserUseCase"],
        );
        let v = engine.check(&a);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, "NAMING001");
    }

    #[test]
    fn naming_rule_ignores_non_matching_import() {
        // UserService importing domain.User (not RepositoryImpl) → no trigger
        let mut config = test_config();
        config.constraints.push(make_naming_constraint(
            "RepositoryImpl",
            "Service",
            "",
            &["app"],
            "Only Service can import RepositoryImpl",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis_with_decls(
            "com.example.app.handler",
            &["com.example.domain.model.User"],
            &["OrderController"],
        );
        assert!(engine.check(&a).is_empty());
    }

    #[test]
    fn naming_rule_ignores_other_layers() {
        // infra layer importing RepositoryImpl → no trigger (rule only for app)
        let mut config = test_config();
        config.constraints.push(make_naming_constraint(
            "RepositoryImpl",
            "Service",
            "",
            &["app"],
            "Only Service can import RepositoryImpl",
        ));

        let engine = ArchRuleEngine::new(config);
        let a = make_analysis_with_decls(
            "com.example.infra.db",
            &["com.example.infra.db.UserRepositoryImpl"],
            &["UserRepositoryConfig"],
        );
        assert!(engine.check(&a).is_empty());
    }
}
