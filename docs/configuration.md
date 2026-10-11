# Configuration

Create `arch-lint.toml` in your project root — analyzer options, per-rule tables, and the generated JSON Schema are below. [Back to README](../README.md).

```toml
# Preset: "recommended" (default), "strict", or "minimal"
preset = "recommended"

# Fail threshold: "error" (default), "warning", or "info"
fail_on = "error"

[analyzer]
root = "./src"
exclude = [
    "**/target/**",
    "**/generated/**",
    "**/vendor/**",
]
respect_gitignore = true
# include = ["src/app/**", "src/domain/**"]

[rules.no-unwrap-expect]
enabled = true
severity = "error"
allow_in_tests = true

[rules.no-sync-io]
enabled = true

[rules.handler-complexity]
enabled = true
max_handler_lines = 150
max_match_arms = 20

[rules.require-thiserror]
enabled = true
severity = "warning"

[rules.require-tracing]
enabled = true
severity = "warning"

[rules.tracing-env-init]
enabled = true
severity = "warning"

# --- Declarative rules (no Rust code needed) ---

[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "handler"
paths = ["src/handler/**"]
exclude = ["src/**/tests/**"]

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infrastructure."

[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::*", "diesel::*"]
message = "Direct DB access is not allowed in domain layer."

[[require-use]]
name = "prefer-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing instead of log."

[[layering]]
order = ["handler", "domain", "infra"]
message = "Layers may only depend downward."

[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"
attrs = ["derive(sqlx::FromRow)"]
on = ["struct", "enum"]
message = "Domain types must stay ORM-agnostic."

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]
on = ["fn"]
message = "Handlers must be instrumented."

[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
message = "Domain traits are ports."

[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
async = true
pattern = "^Result<.*,\\s*AppError>$"
message = "Handlers must return Result<_, AppError>."
```

## `[analyzer]` options

| Key | Default | Meaning |
|-----|---------|---------|
| `root` | `"."` | Directory to walk. |
| `respect_gitignore` | `true` | Skip files git ignores. |
| `include` | `[]` (all `*.rs`) | Glob patterns; when non-empty, only matching files are analyzed. |
| `exclude` | `["**/target/**", "**/vendor/**"]` | Glob patterns dropped from the result. |
| `parallelism` | unset | **Reserved** — parsed, but analysis currently runs sequentially. |

Unknown keys inside `[analyzer]`, inside any declarative rule table
(`[[scopes]]`, `[[restrict-use]]`, ..., `[[require-return]]`) and inside any
per-rule option table (`[rules.<name>]`) are a config error, so a typo such as
`denyy = [...]` or `allow_in_test = true` cannot silently disable a rule or be
ignored. Each rule declares its own option keys; `enabled`, `severity` and
`exclude_files` are accepted next to them for every rule.

File discovery runs in this order:

1. Walk `root` (hidden files and directories are traversed, matching the
   historical `**/*.rs` glob behavior).
2. If `respect_gitignore = true`, skip anything ignored by a `.gitignore` at
   any level, by `.git/info/exclude`, or by the global gitignore. Ignore rules
   are applied even when `root` is **not** inside a git repository, so a
   scaffolded project or a fixture directory carrying a `.gitignore` is
   filtered the same way a checked-out repository is. Non-git ignore files
   (`.ignore`, `.rgignore`) are never consulted.
3. Keep `*.rs` files matching at least one `include` pattern (all of them when
   `include` is empty).
4. Drop files matching any `exclude` pattern.

`include` and `exclude` patterns are matched against each file's path
**relative to `root`** as well as against its absolute path, so both
`exclude = ["src/generated/**"]` and `exclude = ["**/generated/**"]` work. A
leading `./` on a pattern is accepted (`./src/**` ≡ `src/**`).

`Cargo.toml` discovery — used by project-wide rules such as `required-crate` —
follows the same `respect_gitignore` and `exclude` settings, but ignores
`include`, which scopes source files only.

## JSON Schema

`arch-lint schema` prints a JSON Schema (draft 2020-12) for the whole
configuration file — the top-level keys, `[analyzer]`, every built-in rule's
option keys with their documentation, and the declarative sections:

```bash
cargo arch-lint schema > arch-lint.schema.json
cargo arch-lint schema --out arch-lint.schema.json   # same, written directly
```

The same schema is committed at `schema/arch-lint.schema.json` and served
from the raw file on the default branch — `make schema-check` (CI runs the
equivalent as part of `cargo test -p arch-lint-cli`) fails if that file ever
drifts from what the code generates:

```
https://raw.githubusercontent.com/ynishi/arch-lint/main/schema/arch-lint.schema.json
```

That URL is also the schema's `$id`. Once the
[SchemaStore](https://www.schemastore.org/) catalogue entry for arch-lint is
merged (issue #16), VS Code with
[Even Better TOML](https://marketplace.visualstudio.com/items?itemName=tamasfe.even-better-toml)
(built on [taplo](https://taplo.tamasfe.dev/)) picks up `arch-lint.toml` and
`.arch-lint.toml` automatically from the catalogue, no local configuration
needed — this is the only editor path confirmed here. Other editors that
also read the SchemaStore catalogue should pick it up the same way; check
each editor's own docs.

Until then, point your editor at it with the `#:schema` directive that taplo
and Even Better TOML understand — the first line of `arch-lint.toml`:

```toml
#:schema https://raw.githubusercontent.com/ynishi/arch-lint/main/schema/arch-lint.schema.json

preset = "recommended"

[rules.no-unwrap-expect]
allow_in_tests = true
```

The `main` URL above tracks the default branch, so it moves — it is not a
pinned copy. To pin a specific version, point the directive at a release tag
instead (`https://raw.githubusercontent.com/ynishi/arch-lint/<tag>/schema/arch-lint.schema.json`)
or at a local file generated with `arch-lint schema --out <path>`.

You then get completion for rule names and option keys, and a warning on a
misspelled key before the linter ever runs. `arch-lint list-rules --json`
prints the same per-rule option schemas on their own, for tooling that wants
the rule registry rather than the config file shape.

Tables under `rules` whose name is not a built-in rule are left open, so
configuration for custom rules still validates. Tree-sitter configuration
(`[[layers]]`, `[dependencies]`, `[[constraints]]`) is not described by this
schema; those keys are accepted because the top level of the document stays
open.

