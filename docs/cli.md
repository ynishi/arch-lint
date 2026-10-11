# CLI and CI Integration

Command reference for `arch-lint` / `cargo arch-lint`, exit codes, and CI integration (baseline adoption, GitHub Actions, SARIF, pre-commit). [Back to README](../README.md).

## CLI Usage

```bash
cargo install --locked arch-lint-cli

cargo arch-lint init                            # Generate arch-lint.toml
cargo arch-lint init --ts                       # Generate with tree-sitter layers
cargo arch-lint check                           # Run all checks
cargo arch-lint check --rules no-unwrap-expect  # Run specific rules
cargo arch-lint check --format json             # JSON output for CI
cargo arch-lint check --format sarif            # SARIF 2.1.0, for GitHub code scanning
cargo arch-lint check --engine ts               # Force tree-sitter engine
cargo arch-lint check --exit-zero               # Report, but never fail the step
cargo arch-lint check --baseline b.toml --update-baseline  # (re)write a baseline
cargo arch-lint check --baseline b.toml         # Report only violations beyond it
cargo arch-lint list-rules                      # Show available rules and their options
cargo arch-lint list-rules --json               # Same, as JSON (option schemas included)
cargo arch-lint schema                          # JSON Schema of arch-lint.toml
cargo arch-lint schema --out arch-lint.schema.json
cargo arch-lint graph                           # Mermaid graph of [[scopes]] (read-only)
cargo arch-lint graph --format dot              # Graphviz DOT instead
```

`cargo install --locked arch-lint-cli` installs two binaries: `cargo-arch-lint`
(so `cargo arch-lint ...`, Cargo's convention for external subcommands, works)
and a bare `arch-lint` binary that accepts the exact same arguments — the two
are interchangeable, use whichever reads better in your scripts.

Logs go to stderr, never stdout; a successful run prints nothing there (a
source file that fails to parse still logs a WARN line, an invalid
`RUST_LOG` directive prints `ignoring ...`, and exit `2` prints the `Error:`
line). `--verbose` turns on `INFO`/`DEBUG` logs; a non-empty `RUST_LOG`
overrides `--verbose` — invalid directives are reported on stderr
(`ignoring ...`) and skipped, and if none remain the default applies.

### Visualising the scope graph

`arch-lint graph [PATH] --format mermaid|dot` renders the same scope
dependency graph that `[[deny-scope-dep]]`, `[[acyclic]]`, `[[independent]]`
and `[[allow-dep]]` see: every declared scope is a node (an isolated one
included), every edge is a `from → to` reference labelled with its site
count, and an edge that a rule (`ALD003`, `ALD004`, `ALD009`, `ALD010` or
`ALD011`) currently flags is styled red. It never fails the command —
violations are styling, not an exit code — and only the `syn` engine has a
scope graph to draw: a configuration with `[[layers]]` is rejected.

Every Mermaid node id is `s_<name>` (hyphens mapped to `_`), with the real
scope name kept as the node's label (`s_my_scope["my-scope"]`). Without the
prefix, a scope literally named `end` or `style` would be parsed as a
Mermaid flowchart keyword instead of a node id, silently breaking the
diagram (`end -->|1| x` closes the surrounding block, it does not draw an
edge) — the prefix makes every id a non-keyword unconditionally, so there
is no keyword list to maintain.

Command output is the **raw** Mermaid or DOT text — `graph` does not wrap
it in a Markdown `mermaid` fenced code block itself, so it can be piped
straight to `dot -Tsvg` or pasted as-is into a fence of the reader's
choosing. The fence around the example below is this document's, not the
command's.

The fixture under
[`crates/arch-lint-core/tests/fixtures/declarative`](../crates/arch-lint-core/tests/fixtures/declarative)
(two scopes, one `[[deny-scope-dep]]`-denied reference) renders as:

```bash
cargo arch-lint graph crates/arch-lint-core/tests/fixtures/declarative --format mermaid
```

```mermaid
graph LR
    s_domain["domain"]
    s_infra["infra"]
    s_domain -->|1| s_infra
    linkStyle 0 stroke:#d33,stroke-width:2px
```

That fixture is the example here, instead of `examples/sample-project`,
because the sample project declares no `[[scopes]]` at all —
`cargo arch-lint graph examples/sample-project` prints an empty graph (`graph LR`
alone) and `no [[scopes]] declared in examples/sample-project/arch-lint.toml`
on stderr, which is correct but has nothing to draw.

There is no node for an unscoped file: the collector behind `ScopeGraph`
never records a reference whose source or target falls outside every
declared scope, so there is nothing for the graph to draw for it.

### Cross-language (tree-sitter engine)

```bash
cargo arch-lint init --ts
# Edit [[layers]] and [dependencies] in arch-lint.toml
cargo arch-lint check              # auto-detects engine from [[layers]]
cargo arch-lint check --engine ts  # explicit engine selection
```

See [tree-sitter-engine.md](tree-sitter-engine.md) for full documentation.


## CI Integration

### Adopting on an existing codebase

A linter's first run on an existing codebase often reports far more than can
be fixed before the gate is turned on. A baseline freezes today's violations
so CI fails only on new ones — three commands:

```bash
cargo arch-lint check --baseline arch-lint.baseline.toml --update-baseline
git add arch-lint.baseline.toml && git commit -m "chore: baseline existing violations"
cargo arch-lint check --baseline arch-lint.baseline.toml   # in CI from now on
```

The first command runs the full analysis, prints the full report (so the
starting point is visible once), and writes `arch-lint.baseline.toml` — a
TOML file of `[[entry]]` records keyed by `code`, `file` and `message`, each
with a count, no line numbers (an unrelated edit above a baselined violation
does not invalidate it):

```toml
# Generated by `arch-lint check --update-baseline`. Entries are matched by
# code, file and message, with a per-file count; line numbers are not stored.
[[entry]]
code = "AL001"
file = "src/legacy/parser.rs"
message = ".unwrap() is forbidden in production code"
count = 3
```

The third command — the one CI actually runs — reports only violations
beyond the baseline, and exits on those alone. It also prints one line to
stderr:

```
baseline: 3 suppressed, 0 stale
```

A baselined violation that gets fixed shows up as `stale` (`baseline: 1
suppressed, 1 stale`, plus a `baseline: stale entry <code> <file>
(<message>) x<count>` line per entry) — re-run the first command to drop it
from the file. `--baseline` without `--update-baseline` requires the file
to already exist; a missing one is exit `2`, naming
`--update-baseline` as the fix.

This wording is a deliberate deviation from the originating issue's
acceptance text (which described a clean run as `0 new violations
(3 baselined)` and a stale entry as a plain "warning"); `baseline: N
suppressed, M stale` and `baseline: stale entry ...` were kept instead as
the grep-able, scriptable form.

`--update-baseline` prints `baseline: wrote N entries to <path>` to stderr.
It writes whatever the run in front of it found, so combining it with
`--rules` or `--exclude` writes a **partial** baseline — only the
selected rules' (or unexcluded files') violations, not the full set — which
then under-baselines everything else. Run `--update-baseline` with the
same `--rules` / `--exclude` (or none) that CI's `--baseline` run uses.

The same baseline works from `check!()`, so the `cargo test` gate benefits
too — the path is relative to the workspace root, like `config`:

```rust,ignore
// tests/architecture.rs
arch_lint::check!(baseline = "arch-lint.baseline.toml");
```

### Exit status

| Code | Meaning |
|------|---------|
| `0` | Ran; no violation at or above `fail_on` (or `--exit-zero` was given) |
| `1` | Ran; violations at or above `fail_on` were found |
| `2` | Could not run: configuration error, unknown rule, unreadable path, I/O error |

A CI step can therefore fail on violations and still tell a misconfigured
linter (`2`) apart from a real finding (`1`). `--exit-zero` turns the step
into a report-only one.

`--format json`, `--format compact` and `--format sarif` on stdout are clean
machine-readable output with no log lines mixed in — tracing diagnostics go
to stderr, so there is no need to redirect it away (e.g. `2>/dev/null`)
before piping stdout to `jq`, uploading it, or writing it to a file.

### GitHub Actions

This repository publishes itself as a composite action
([`action.yml`](../action.yml), issue #15):

```yaml
name: Lint
on: [push, pull_request]

permissions:
  contents: read          # required by actions/checkout
  actions: read           # required by upload-sarif on private repos
  security-events: write  # required by upload-sarif

jobs:
  arch-lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ynishi/arch-lint@arch-lint-v0.9.0
```

Pin a release tag, for example `arch-lint-v0.9.0` — the first to ship
`action.yml`; 0.8.0 and earlier tags predate it.

Default `args` is `check --format sarif`: `upload-sarif` defaults to
`true`, so violations become code-scanning alerts (PR annotations that
survive re-runs, plus the Security tab — this is what the manual route
below uses `::error` workflow commands to avoid, since those are capped at
10 per severity per step and silently drop the rest), and the job still
fails on them — the action's own exit code is `cargo arch-lint`'s,
enforced *after* the SARIF upload runs, so a failing PR gets both the
alerts and a failed check in the same run. For a report-only run that
never fails the step, pass `args: check --exit-zero --format sarif` (and
drop `security-events: write` if nothing else in the workflow needs it).

All inputs: `args` (default `check --format sarif`), `version` (an
arch-lint-cli version, or `latest`, forwarded to the installer),
`working-directory` (default `.`), `upload-sarif` (default `true`),
`sarif-file` (default `arch-lint.sarif`, relative to `working-directory`).
Outputs: `exit-code`, `sarif-file`.

Pin the tag to an actual release, not a floating major — `arch-lint-v<version>`
as above, not `@v0` or `@main` (see [CONTRIBUTING.md](../CONTRIBUTING.md),
"Releasing": this project does not maintain a floating `v0` tag).

<details>
<summary>Installing the CLI by hand instead of the action</summary>

The action wraps [`taiki-e/install-action`](https://github.com/taiki-e/install-action);
calling it directly works the same way, without the action's exit-code
sequencing:

```yaml
- uses: taiki-e/install-action@v2
  with:
    tool: arch-lint-cli
```

`arch-lint-cli` is not in install-action's own manifest of tools it
downloads directly; for a tool it does not recognize, install-action hands
off to `cargo-binstall`, which reads the crate's
`[package.metadata.binstall]` table (issue #14) and downloads the matching
GitHub Release archive instead of compiling — trying `cargo-quickinstall`
(a separately hosted prebuilt-binary cache) next, and falling back to a
source build (`cargo install`) only when neither has a matching archive.
The `cargo arch-lint` step that follows then runs against whatever `cargo`
is already on the runner (GitHub-hosted runner images ship a Rust
toolchain by default); this step does not need a `dtolnay/rust-toolchain`
step either way.

The simplest form fails the step directly on violations:

```yaml
- uses: actions/checkout@v4
- uses: taiki-e/install-action@v2
  with:
    tool: arch-lint-cli
- run: cargo arch-lint check   # exit 1 on violations, 2 if it could not run
```

Or upload SARIF by hand instead of through the action (`artifactLocation.uri`
is relative to the path given to `check`, so run it against the repository
root — `arch-lint check . --format sarif`, as below — a narrower path
produces URIs that do not line up with the checkout):

```yaml
- uses: actions/checkout@v4
- uses: taiki-e/install-action@v2
  with:
    tool: arch-lint-cli
- run: cargo arch-lint check . --format sarif --exit-zero > arch-lint.sarif
  # --exit-zero: let upload-sarif turn violations into alerts instead of
  # failing the step before the upload runs — the step still exits 2 (and
  # fails) on a configuration error, so a broken config is not silently
  # swallowed. Gating the PR then comes from GitHub's code-scanning checks
  # or branch protection, not this step's exit code.
- uses: github/codeql-action/upload-sarif@v4
  with:
    sarif_file: arch-lint.sarif
```

</details>

### Pre-commit Hook

```bash
cargo install --locked arch-lint-cli   # or: cargo binstall arch-lint-cli
```

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/ynishi/arch-lint
    rev: arch-lint-v0.9.0
    hooks:
      - id: arch-lint
```

The hook (`.pre-commit-hooks.yaml`, issue #15) is `language: system`: it
installs nothing and runs `entry` as-is, so `arch-lint` must already be on
`PATH` (the `cargo install`/`cargo binstall` line above). `language: rust`
would be the more usual choice for a Rust tool's own hook, but it always
runs `cargo install --bins --root <env> --path .` against the checked-out
hook repo in addition to anything `additional_dependencies` adds, and this
repository's root `Cargo.toml` is a virtual manifest (`arch-lint-cli` lives
at `crates/arch-lint-cli`, not at the root) — that install has no package
to build and fails, regardless of `additional_dependencies`.

The trade-off: `language: system` hooks do not run on
[pre-commit.ci](https://pre-commit.ci/), which only provisions a fixed set
of languages in its own sandbox and does not trust an arbitrary system
`PATH`. A project that relies on pre-commit.ci rather than running hooks
locally or in its own CI needs a different hook definition than this one,
or arch-lint in its own CI step instead (see "GitHub Actions" above).

Pin a release tag, for example `arch-lint-v0.9.0`, for `rev` — the same one
as for the action above; it is the first to ship `.pre-commit-hooks.yaml`. See
[CONTRIBUTING.md](../CONTRIBUTING.md), "Verification", for the
`pre-commit try-repo` check a maintainer runs before relying on it.

A cross-language project (the tree-sitter engine, see
[tree-sitter-engine.md](tree-sitter-engine.md)) can add `kotlin` (or
another supported language) to `types_or` in its own
`.pre-commit-config.yaml` hook override, alongside `rust`.

<details>
<summary>A raw pre-commit hook instead</summary>

```bash
#!/bin/sh
# .git/hooks/pre-commit

cargo arch-lint check --format compact
if [ $? -ne 0 ]; then
    echo "arch-lint found violations. Please fix before committing."
    exit 1
fi
```

</details>

