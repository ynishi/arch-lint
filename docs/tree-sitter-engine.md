# Tree-sitter Engine

Cross-language architecture enforcement powered by Tree-sitter.

## Overview

The tree-sitter engine (`arch-lint-ts` crate) extends arch-lint beyond Rust.
It parses source files with Tree-sitter, extracts package/import/declaration info
into a language-agnostic intermediate representation, then evaluates architecture
layer rules defined in TOML.

**Key difference from the syn engine:**

| | syn engine | tree-sitter engine |
|---|---|---|
| Scope | Rust only | Any language with Tree-sitter grammar |
| Parser | `syn` (Rust AST) | Tree-sitter (incremental, language-agnostic) |
| Rules | Per-file AST lint rules (AL001-AL012) | Layer dependency + pattern + naming constraints (LAYER001, PATTERN001, NAMING001) |
| Config | `[rules.*]` sections | `[[layers]]` + `[dependencies]` + `[[constraints]]` |
| Activation | Default (no `[[layers]]` in config) | Auto when `[[layers]]` present, or `--engine ts` |

Both engines share `arch-lint-core` types (`Violation`, `Severity`, `Location`, `LintResult`).

## Engine Auto-detection

The CLI automatically selects the engine based on config content:

```
arch-lint.toml contains [[layers]]  -->  tree-sitter engine
arch-lint.toml without [[layers]]   -->  syn engine
```

Override with `--engine`:

```bash
arch-lint check --engine ts    # Force tree-sitter
arch-lint check --engine syn   # Force syn
```

## Quick Start

```bash
# Generate tree-sitter config template
arch-lint init --ts

# Edit layers and dependencies for your project
$EDITOR arch-lint.toml

# Run check (auto-detects tree-sitter engine)
arch-lint check

# Explicit engine selection
arch-lint check --engine ts

# JSON output for CI
arch-lint check --format json

# Compact one-line-per-violation output
arch-lint check --format compact
```

## Configuration (TOML DSL)

Full example:

```toml
[analyzer]
root = "./src"
exclude = ["**/test/**", "**/build/**", "**/generated/**"]

# Layer definitions
# Each layer maps to one or more package prefixes.
# Files whose package matches a prefix belong to that layer.

[[layers]]
name = "domain"
packages = ["com.example.domain"]

[[layers]]
name = "application"
packages = ["com.example.app", "com.example.usecase"]

[[layers]]
name = "infrastructure"
packages = ["com.example.infra"]

[[layers]]
name = "presentation"
packages = ["com.example.api", "com.example.handler"]

# Dependency rules: which layers may depend on which.
# Same-layer imports are always allowed (implicit).
# An empty list means the layer has no external dependencies.

[dependencies]
domain = []                                            # domain depends on nothing
application = ["domain"]                               # application -> domain OK
infrastructure = ["domain", "application"]             # infra -> domain, app OK
presentation = ["domain", "application", "infrastructure"]

# Custom constraints (optional)
# Pattern-based import restrictions applied to specific layers.

[[constraints]]
type = "no-import-pattern"
pattern = "java.sql"
in_layers = ["domain", "application"]
severity = "warning"
message = "Direct JDBC usage forbidden in upper layers"

[[constraints]]
type = "no-import-pattern"
pattern = "kotlinx.coroutines.GlobalScope"
in_layers = ["domain", "application", "infrastructure", "presentation"]
severity = "error"
message = "GlobalScope is forbidden, use structured concurrency"

# Naming convention enforcement
# Only Service classes may import RepositoryImpl.
[[constraints]]
type = "naming-rule"
import_matches = "RepositoryImpl"
source_must_match = "Service"
in_layers = ["application"]
severity = "error"
message = "Only Service classes may import RepositoryImpl"

# UseCase should not depend on other UseCases.
[[constraints]]
type = "naming-rule"
import_matches = "UseCase"
source_must_not_match = "UseCase"
in_layers = ["application"]
severity = "warning"
message = "UseCase should not depend on other UseCases directly"
```

### Configuration Reference

#### `[analyzer]`

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `root` | string | `"."` | Project root directory (relative to config file location) |
| `exclude` | string[] | `[]` | Glob patterns for files/dirs to skip |

#### `[[layers]]`

| Key | Type | Required | Description |
|-----|------|----------|-------------|
| `name` | string | yes | Layer identifier (used in `[dependencies]` and `[[constraints]]`) |
| `packages` | string[] | yes | Package prefixes belonging to this layer |

#### `[dependencies]`

Map of `layer_name = [allowed_dependency_layers]`.

- Every layer defined in `[[layers]]` **must** have an entry here.
- Same-layer imports are always allowed (implicit, no need to list self).
- Self-dependency (`domain = ["domain"]`) is rejected by validation.

#### `[[constraints]]`

Common fields:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `type` | string | required | `"no-import-pattern"` or `"naming-rule"` |
| `in_layers` | string[] | `[]` | Layers this constraint applies to |
| `severity` | string | `"error"` | `"error"`, `"warning"`, or `"info"` |
| `message` | string | `""` | Human-readable violation message |

Fields for `type = "no-import-pattern"`:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `pattern` | string | `""` | Substring to match against import paths |

Fields for `type = "naming-rule"`:

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `import_matches` | string | `""` | Import path must contain this substring to trigger |
| `source_must_match` | string | `""` | Source file must have a declaration name containing this (if set) |
| `source_must_not_match` | string | `""` | Source file must NOT have a declaration name containing this (if set) |

### Config Validation

`arch-lint check` validates the config before analysis:

- Unknown layer names in `[dependencies]` values
- Self-dependency in `[dependencies]`
- Missing `[dependencies]` entry for a declared layer
- Unknown layer names in `[[constraints]].in_layers`

## Rule Codes

| Code | Name | Severity | Description |
|------|------|----------|-------------|
| LAYER001 | `layer-dependency` | error | Import crosses a forbidden layer boundary |
| PATTERN001 | `import-pattern` | configurable | Import matches a forbidden pattern in a constrained layer |
| NAMING001 | `naming-rule` | configurable | Import violates a naming convention constraint |

### LAYER001: layer-dependency

Fires when a file in layer A imports from layer B, but B is not in A's allowed dependencies.

```
LAYER001 layer-dependency at src/.../BadDomain.kt:4:1
  error: domain -> infrastructure dependency not allowed
```

Resolution: either move the code to the correct layer, or update `[dependencies]` if the dependency is intentional.

### PATTERN001: import-pattern

Fires when an import matches a `[[constraints]]` pattern in the file's layer.

```
PATTERN001 import-pattern at src/.../Service.kt:3:1
  warning: Direct JDBC usage forbidden in upper layers
```

### NAMING001: naming-rule

Fires when an import matches `import_matches` and the source file's declarations
violate `source_must_match` or `source_must_not_match`.

Two modes:

**`source_must_match`** — Only classes with matching names may use the import:

```toml
[[constraints]]
type = "naming-rule"
import_matches = "RepositoryImpl"
source_must_match = "Service"
in_layers = ["application"]
message = "Only Service classes may import RepositoryImpl"
```

```
# BadOrderUseCase.kt imports UserRepositoryImpl → VIOLATION (not a Service)
NAMING001 naming-rule at src/.../BadOrderUseCase.kt:5:1
  error: Only Service classes may import RepositoryImpl

# UserService.kt imports UserRepositoryImpl → OK
```

**`source_must_not_match`** — Classes with matching names must NOT use the import:

```toml
[[constraints]]
type = "naming-rule"
import_matches = "UseCase"
source_must_not_match = "UseCase"
in_layers = ["application"]
severity = "warning"
message = "UseCase should not depend on other UseCases directly"
```

```
# DeleteUserUseCase.kt imports CreateUserUseCase → VIOLATION
NAMING001 naming-rule at src/.../DeleteUserUseCase.kt:5:1
  warning: UseCase should not depend on other UseCases directly
```

Typical use cases:
- `RepositoryImpl` is only imported by `Service` (not UseCase, not Controller)
- `UseCase` does not import other `UseCase` (extract shared logic to Service)
- `Controller` does not import `RepositoryImpl` (must go through Service)

## Layer Resolution

Package-to-layer mapping uses **longest-prefix-match**:

```toml
[[layers]]
name = "infra"
packages = ["com.example.infra"]

[[layers]]
name = "infra-db"
packages = ["com.example.infra.db"]
```

- `com.example.infra.api.Client` resolves to `infra`
- `com.example.infra.db.UserRepo` resolves to `infra-db` (more specific prefix wins)
- `com.example.domains.Foo` does **not** match `com.example.domain` (no false prefix match)

## Supported Languages

| Language | Extractor | Extensions | Status |
|----------|-----------|------------|--------|
| Kotlin | `KotlinExtractor` | `.kt`, `.kts` | Stable |

### What the Kotlin Extractor Captures

- `package` declaration (`package com.example.domain.model`)
- `import` statements (fully qualified paths)
- Top-level declarations: `class`, `data class`, `sealed class`, `enum class`, `interface`, `object`

### Adding a New Language

Implement the `LanguageExtractor` trait:

```rust
pub trait LanguageExtractor: Send + Sync {
    /// Language identifier (e.g., "go", "java").
    fn language_id(&self) -> &'static str;

    /// File extensions (e.g., &[".go"]).
    fn extensions(&self) -> &'static [&'static str];

    /// Extract package, imports, and declarations from source.
    fn analyze(&self, source: &str) -> FileAnalysis;
}
```

The extractor produces a `FileAnalysis` (language-agnostic IR):

```rust
pub struct FileAnalysis {
    pub file_path: PathBuf,
    pub package: Option<PackageInfo>,  // package/module declaration
    pub imports: Vec<ImportInfo>,       // import statements
    pub declarations: Vec<DeclInfo>,    // classes, interfaces, etc.
}
```

Then register it in `check_ts.rs`:

```rust
let extractors: Vec<Box<dyn LanguageExtractor>> = vec![
    Box::new(KotlinExtractor::new()),
    Box::new(GoExtractor::new()),      // add here
];
```

## Architecture (Internal)

```
arch-lint-ts/src/
├── lib.rs          Public API re-exports
├── config.rs       ArchConfig, LayerDef, Constraint (TOML deserialization + validation)
├── extractor.rs    LanguageExtractor trait, FileAnalysis IR types
├── kotlin.rs       KotlinExtractor (Tree-sitter AST walking)
├── layer.rs        LayerResolver (longest-prefix-match)
└── engine.rs       ArchRuleEngine (LAYER001, PATTERN001, NAMING001 checks)
```

Data flow:

```
Source file (.kt)
    │
    ▼
KotlinExtractor.analyze(source)    ← Tree-sitter parse + AST walk
    │
    ▼
FileAnalysis { package, imports, declarations }
    │
    ▼
ArchRuleEngine.check(analysis)     ← LayerResolver + dependency rules
    │
    ▼
Vec<Violation>                     ← arch-lint-core types
```

## Output Formats

### Text (default)

```
LAYER001 layer-dependency at src/.../BadDomain.kt:4:1
  error: domain -> infrastructure dependency not allowed

Found 4 error(s), 0 warning(s), 0 info(s) in 7 file(s)
```

### JSON (`--format json`)

```json
{
  "violations": [
    {
      "code": "LAYER001",
      "rule": "layer-dependency",
      "severity": "error",
      "location": {
        "file": "src/.../BadDomain.kt",
        "line": 4,
        "column": 1
      },
      "message": "domain -> infrastructure dependency not allowed"
    }
  ],
  "files_checked": 7
}
```

### Compact (`--format compact`)

```
src/.../BadDomain.kt:4:1: error [LAYER001] domain -> infrastructure dependency not allowed
```

## CI Integration

```yaml
# GitHub Actions
- run: arch-lint check --format json > lint-results.json
- run: |
    if [ $(jq '.violations | length' lint-results.json) -gt 0 ]; then
      arch-lint check
      exit 1
    fi
```

## Typical Layered Architecture

```
presentation  ──→  application  ──→  domain
     │                  │
     └──→  infrastructure  ──→  domain
```

```toml
[dependencies]
domain = []
application = ["domain"]
infrastructure = ["domain", "application"]
presentation = ["domain", "application", "infrastructure"]
```

The domain layer is pure — no outward dependencies.
Application orchestrates domain. Infrastructure implements interfaces.
Presentation handles I/O (HTTP handlers, CLI).
