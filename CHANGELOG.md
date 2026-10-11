# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Baseline (`--baseline` / `--update-baseline`)**: adopt arch-lint on an
  existing codebase by freezing today's violations and failing only on new
  ones. `arch-lint check --baseline <path> --update-baseline` runs the full
  analysis, prints the full report, and (re)writes `<path>` as a TOML file
  of `[[entry]]` records (`code`, `file`, `message`, `count`); exits `0`
  regardless of violations found, since it is an authoring step, not a
  check. `arch-lint check --baseline <path>` (without `--update-baseline`)
  requires the file to already exist (a missing one is exit `2` with a
  message pointing at `--update-baseline`), reports only violations beyond
  it, prints `baseline: N suppressed, M stale` to stderr (plus one
  `baseline: stale entry <code> <file> (<message>) xN` line per baseline
  entry whose recorded count exceeds what this run found — a fixed
  violation), and the exit status comes from the new violations only.
  `check!(baseline = "path")` (relative to the workspace root, like
  `config`) wires the same behavior into the `cargo test` gate; a missing
  baseline file panics with a message pointing at
  `arch-lint check --baseline <path> --update-baseline`, and a panic (new
  violations found) carries a trailing `baseline: N suppressed, M stale`
  line. New library API in the `arch-lint` facade: the `baseline` module
  (`Baseline`, `Entry`, `Applied`, `BaselineError`, `fingerprint`, `apply`)
  and `Session::{with_baseline, run_with_baseline}`.

  **Design decision**: a violation's baseline identity is computed
  centrally by `baseline::fingerprint`, not emitted per-rule. The issue
  proposed a `Violation.fingerprint: Option<String>` field populated by
  each rule; that was rejected because `Violation`'s fields are all `pub`
  and the struct is not `#[non_exhaustive]`, so a new field would have
  broken every existing `Violation::new(...)` call site and every
  downstream struct literal. Instead the identity is `(code, file,
  message)` plus a per-file count — exactly PHPStan's `ignoreErrors`
  scheme — where `file` is the root-relative path as it already appears in
  `Violation.location.file` and `message` is `Violation.message` verbatim.
  **Line numbers are never stored or matched on**: this is deliberate, so
  an unrelated edit above a baselined violation does not invalidate its
  entry (verified directly — inserting 10 blank lines above a baselined
  violation does not change the baseline result). An entry survives an edit
  as long as the edit does not change the message or move the violation to
  a different file: built-in rule messages are otherwise constant per pattern;
  `ALD003`/`ALD004` name the denied scope edge and the specific `use` path;
  `ALD010`/`ALD011` name the scope edge only. **`AL004`
  (`handler-complexity`) is the one exception shipped today** — its message
  embeds the measured size (`"... has 12 arms (max: 5)"`), so adding or
  removing an arm/line/variant in an already-baselined handler changes the
  fingerprint even though the handler's shape didn't otherwise change (the
  old entry goes stale, the new size is reported as new; re-run
  `--update-baseline` to pick it up — pinned by a CLI test). Moving that
  measurement out of `AL004`'s message is tracked separately, not done
  here. **Severity is not part of the key**: a baselined warning stays
  suppressed after a later `[rules.<name>] severity` override changes it.
  **`ALD009` (`acyclic`)** attributes its one violation per cycle to the
  *first* edge's file in cycle-path order; a new reference that changes
  which edge sorts first moves the entry to a different `file` even when
  the cycle itself is unchanged. The same `fingerprint` function is meant
  to back SARIF `partialFingerprints` (issue #11), so one identity scheme
  serves both.

  **Accepted wording deviations from the issue**: the issue's acceptance
  text says a clean run reports `0 new violations (3 baselined)` and a
  stale entry "prints a warning"; the shipped wording is
  `baseline: 3 suppressed, 0 stale` and
  `baseline: stale entry <code> <file> (<message>) xN` — kept as the
  clearer, grep-able form rather than matched to the issue's prose.

## [0.7.0] - 2026-10-11

### Added

- **`arch-lint graph [PATH] --format mermaid|dot`**: a read-only subcommand
  (`syn` engine only — a `[[layers]]` configuration is rejected) that
  renders the `[[scopes]]` dependency graph as raw Mermaid flowchart text
  (default) or Graphviz DOT — pipeable to `dot -Tsvg` or pasteable into a
  Markdown fenced code block of the reader's choosing; the command does not
  wrap its own output in a fence (a deviation from the issue's acceptance
  wording, recorded here and in the README). Nodes are every declared
  scope, isolated ones included; edges are labelled with their site count;
  an edge that `ALD003`, `ALD004`, `ALD009`, `ALD010` or `ALD011` currently
  flags is styled red (a `linkStyle` line in Mermaid, `color="red"` in
  DOT). Every Mermaid node id is prefixed `s_` (e.g. `s_my_scope`, with the
  real name, hyphens included, kept as its label) so a scope literally
  named `end` or `style` — a Mermaid flowchart keyword — cannot be parsed
  as that keyword instead of a node and silently break the diagram.
  Violations never fail the command — only `check` does. A project without
  `[[scopes]]` prints an empty graph and a one-line hint on stderr, exit
  `0` (`examples/sample-project` is one such project, which is why the
  README's example graph instead renders
  `crates/arch-lint-core/tests/fixtures/declarative`, the second recorded
  deviation from the issue's acceptance wording). New library API in the
  `arch-lint` facade crate: `arch_lint::graph::{render, violating_edges,
  GraphFormat}` (plus a re-export of `ScopeName`), both plain string
  writers with no external dependency and deterministic (the same
  node/edge order `ScopeGraph` already iterates in).
  `violating_edges` compares only a `Location`'s `file`/`line`/`column`,
  not `offset`/`length`, so a rule that starts filling those in later does
  not silently stop matching. There is no `--include-unscoped` option: the
  collector behind `ScopeGraph` never records an unscoped reference target,
  so there is nothing for the graph to draw for it.

- **`[[acyclic]]` (ALD009)**: a project-wide rule that reports a cycle
  between `[[scopes]]` — Tarjan's strongly-connected-components algorithm
  over the sub-graph induced by `scopes` (every declared scope when
  omitted); every component of more than one node is one violation, whose
  message names every scope in the component plus the shortest concrete
  cycle through its smallest member, in order (e.g. `Cycle between scopes
  domain, infra: domain → infra → domain`), with the primary `Location` at
  the first site of the first edge and the other edges attached as
  `Violation` labels (now rendered by the text report too, as
  `= note: <message> at <file>:<line>:<column>`, not only JSON). The first
  project-wide rule built on the `ScopeGraph`: paired with the new
  `declarative::create_project_rules_shared` /
  `declarative::create_project_rules` (the `ProjectRuleBox` counterparts of
  `create_rules_shared` / `create_rules`), which `Session::run` now wires up
  alongside the per-file declarative rules. Overlapping `[[acyclic]]` blocks
  each report the same cycle once.
  - An `// arch-lint: allow(acyclic) reason="..."` comment (or
    `#[arch_lint::allow(acyclic, reason = "...")]` attribute, on the `use`
    item itself or an enclosing `mod`/`fn`/`impl`/`trait`) removes that site
    from the graph before cycles are computed. Whether a `reason` is
    required depends on where the directive came from, exactly as it does
    for the per-file rules: an attribute (on the item or an enclosing
    scope) suppresses silently either way; a comment on an error-severity
    rule needs one. A comment directive without a reason that suppresses a
    site on an edge lying on a cycle (computed before suppression) gets the
    same missing-reason warning `Suppressions::decide` reports for the
    per-file rules, in place of the violation — one warning per such site,
    not per edge, even if another, reasoned `allow` anywhere else on the
    cycle also breaks it. A rule's own `name` is honored in either
    kebab-case or
    `snake_case`, matched against either spelling of a comment or attribute
    directive. Reporting an `allow(acyclic)` directive that no longer
    suppresses a cycle as unused is not implemented.
  - `EdgeSite` gained a private `allow_directives` field (every
    `AllowDirective` in scope at that site, each tagged with a
    `DirectiveSource` — `Scope`, `Item` or `Comment` — saying where it came
    from), read through `EdgeSite::allow_directive` / `allow_directives`,
    plus an `EdgeSite::new` constructor (the struct is `#[non_exhaustive]`)
    and `EdgeSite::with_allow_directive`, which normalizes the directive's
    rule names to `snake_case` as it stores them.
    `utils::allowance::parse_allow_directive` and
    `utils::attributes::parse_arch_lint_allow_attr` are now `pub(crate)` so
    the collector can reuse them directly.
  - `[[acyclic]]` gained a `doc` field (a document reference), matching the
    other eight declarative rule kinds; it is attached to the violation the
    same way `[[deny-scope-dep]]`'s does.
  - `arch-lint list-rules` (text and `--json`) now lists all nine
    declarative rules (`ALD001`-`ALD009`), each `--json` entry adding a
    `table` key naming its `[[<table>]]` section and an `options` schema
    inlined from `arch-lint.toml`'s generated JSON Schema (previously a bare
    unresolved `$ref`).
- **`[[independent]]` (ALD010)**: a project-wide rule that reports one
  violation per offending edge `(from, to)` between the scopes named in
  `scopes` (`from, to ∈ scopes`, `from != to`), in either direction — the
  "these N scopes must not depend on each other at all" rule previously
  only expressible as `N × (N - 1)` `[[deny-scope-dep]]` entries that drift
  as soon as a scope is added. `scopes` is required and needs at least two
  entries (a load-time error otherwise, same as an unknown scope name in
  it); unlike `[[acyclic]]` there is no "every declared scope" default.
  Each violation is located at the edge's first non-suppressed site, with
  the edge's other non-suppressed sites attached as `Violation` labels; the
  message names both scopes plus the arrow (e.g. `Independent scopes
  billing and shipping must not depend on each other: billing → shipping`).
  Built the same way `[[acyclic]]` is: paired into
  `declarative::create_project_rules_shared` /
  `declarative::create_project_rules` alongside `AcyclicRule`, honors the
  same `// arch-lint: allow(independent) reason="..."` comment /
  `#[arch_lint::allow(independent, reason = "...")]` attribute directives
  with the same reason requirement on error-severity rules (unlike
  `[[acyclic]]`, the missing-reason warning does not depend on the site
  lying on a cycle: every in-scope edge is an offence), and reuses the
  per-site suppression classification extracted out of `AcyclicRule` into
  `declarative::rules::graph_suppress` for the two to share. `arch-lint
  list-rules` (text and `--json`) and `arch-lint schema` now cover it
  alongside the other nine declarative rules. `scopes` rejects a repeated
  scope name (it would otherwise collapse to one graph node and the rule
  would silently check nothing against it).
- **`[[allow-dep]]` (ALD011)**: a project-wide rule that reports one
  violation per edge `from → X` in the dependency graph where `X` is
  a declared scope not in `to` (`X != from`) — the inverse of
  `[[deny-scope-dep]]`'s deny-list, for teams that would rather enumerate
  what a scope *may* depend on. `to = []` forbids every declared-scope
  dependency from `from`; a dependency on a path that resolves to no
  declared scope (an external crate, an unscoped file) is never a
  violation, same as `[[deny-scope-dep]]`. Several `[[allow-dep]]` blocks
  may share the same `from`; each is checked independently against its own
  `to`. Each violation is located at the edge's first non-suppressed site,
  with the edge's other non-suppressed sites attached as `Violation`
  labels; the message names the allow-list plus the arrow (e.g. `Scope
  \`domain\` may only depend on [shared]: domain → infra`). Built the same
  way `[[acyclic]]` and `[[independent]]` are: paired into
  `declarative::create_project_rules_shared` /
  `declarative::create_project_rules` alongside `AcyclicRule` and
  `IndependentRule`, honors the same `// arch-lint: allow(allow-dep)
  reason="..."` comment / `#[arch_lint::allow(allow-dep, reason = "...")]`
  attribute directives with the same reason requirement on error-severity rules
  (like `[[independent]]`, the missing-reason warning does not depend on
  the site lying on a cycle), and reuses the per-site suppression
  classification in `declarative::rules::graph_suppress`. `arch-lint
  list-rules` (text and `--json`) and `arch-lint schema` now cover it
  alongside the other ten declarative rules. `to` rejects an entry equal to
  `from` and a repeated entry (a load-time error, same shape as
  `[[independent]]`'s repeated-scope check).
- **`arch-lint check --exit-zero`** reports violations and exits `0`, for
  report-only CI steps
- **`ScopeGraph`**: the dependency graph between declared `[[scopes]]`
  (nodes = scope names, edges = scope-crossing `use`/inline references with
  the `Location` of each site), collected once per `Analyzer::analyze()` by a
  visitor that runs inside the existing per-file pass (no second parse). Path
  resolution is the one ALD003 uses — `crate::`, `self::` and `super::`
  paths relative to the file's module, inline `mod` blocks included (see
  Changed, below, for the ALD003 / ALD004 behavior change this enables);
  unlike ALD003 the graph records every reference regardless of
  `check_inline` — rules built on the graph decide what to filter (each
  `EdgeSite` does carry every `allow` directive in scope at it, for a rule
  built on the graph to act on, see ALD009 above). Reachable through
  `ProjectContext::scope_graph()`
  (for a `ProjectRule`), the standalone `Analyzer::scope_graph()` (builds the
  graph without running any rule) and `Session::scope_graph()` (the SDK
  driver). Enabled by the new
  `AnalyzerBuilder::declarative(Arc<DeclarativeConfig>)` builder method,
  paired with the new `declarative::load_config_from_toml` (parses TOML into
  a `DeclarativeConfig` without building rules) and
  `declarative::create_rules_shared` (builds rules from a shared `Arc`, so the
  same config can also be handed to the analyzer). Prerequisite for
  independence and allow-list rules, and `arch-lint graph` (separate issues)

### Changed

- **`Format::Text` now prints `Violation::labels`**, one `= note: <message>
  at <file>:<line>:<column>` line per label after the `= help:` suggestion
  (if any); `Format::Json` already serialized them. Affects any custom SDK
  rule that attaches labels — no built-in or declarative rule did before
  `[[acyclic]]` (ALD009, above)
- **`deny-scope-dep` (ALD003), `layering` (ALD004) and the `ScopeGraph`** now
  also resolve `self::` and `super::` paths relative to the file's module
  (inline `mod` blocks included), not just `crate::`; bare paths and `pub use`
  re-exports remain unresolved
- **ALD001 `restrict-use` inline checks now also see `self::` / `super::`
  paths**, matched verbatim against the `deny` / `except` patterns, as `use`
  items already were
- **`ProjectContext` is now `#[non_exhaustive]`** and has a new `scope_graph`
  field; construct it with `ProjectContext::new` plus the `with_*` builders,
  not a struct literal. `DeclarativeConfigDto` also gained public fields in
  this release (`acyclic`, `independent`, and now `allow_dep`); construct it
  via `Default` or TOML deserialization, not a struct literal, for the same
  reason
- **`ModelError` is now `#[non_exhaustive]`**; new variants
  `IndependentTooFewScopes` and `DuplicateIndependentScope` (added for
  `[[independent]]`, ALD010, above) would otherwise have broken exhaustive
  matches downstream. Bundled with the `ProjectContext` breaking change
  above, in the same release
- **Exit status `2` when the CLI could not run.** Configuration errors,
  unknown `--rules` names, unreadable paths and I/O errors now exit `2`;
  `1` is reserved for "ran, and found violations at or above `fail_on`".
  Previously both cases exited `1`. The table is in `arch-lint check --help`
  and the README "CI Integration" section
- `CONTRIBUTING.md` rewritten around the workflow: label table with branch
  prefixes, issue template (`.github/ISSUE_TEMPLATE/issue.md`: Problem /
  Evidence / Proposal / Acceptance), pull request template with an acceptance
  table, verification through the installed binary, commit subject prefixes.
  New `PUBLIC_DEVELOPMENT.md` states the public-by-default policy and what
  must not appear in a public artifact

### Fixed

- **CI no longer depends on the newest dependency versions.** The workspace
  uses `resolver = "3"` (MSRV-aware, Rust 1.84+) and `Cargo.lock` is now
  committed, so the MSRV (1.88) job checks a fixed, MSRV-buildable tree with
  `--locked` instead of resolving transitive dependencies that have raised
  their `rust-version` (`textwrap`, `tree-sitter-language`). A non-blocking
  `latest deps` job runs `cargo update` on stable so upstream breakage stays
  visible. `cargo install --locked arch-lint-cli` is the documented install
  line
- clippy: `.ok().is_some_and(..)` replaced by `.is_ok_and(..)` in
  `async-trait-send-check`, which newer stable clippy rejects under
  `-D warnings`
- **CLI logs no longer print to stdout.** `arch-lint check --format json`
  (and `--format compact`) previously printed `tracing` `INFO` lines ahead
  of the report, so the captured stdout did not parse as JSON. The
  subscriber now writes to stderr, the default level is `warn` (silent on a
  successful run), and `--verbose` raises it to `debug`; a non-empty
  `RUST_LOG` overrides `--verbose`, invalid directives are reported on
  stderr (`ignoring ...`) and skipped, and if none remain the default
  applies. Invalid `RUST_LOG` directives are now reported on stderr instead
  of silently disabling logging
- **A trailing `// arch-lint: allow(...)` comment, after code on the
  violating line, is now recognized.** `parse_allow_directive` previously
  required the comment to start the (trimmed) line, so
  `use crate::infra::db::Pool; // arch-lint: allow(deny-scope-dep)
  reason="..."` was silently ignored even though the README already
  documented "comment on the violating line or the line above it." The
  parser now scans the line for the first `//` that is not inside a
  `"..."` string literal or a `'.'` char literal and parses the directive
  from there, so both forms work. The line *above* the violation still
  only supplies a directive when it is comment-only
  (`line.trim_start().starts_with("//")`): a trailing directive is scoped
  to its own line, so a single `allow` after code on line N can no longer
  also suppress a violation on line N+1. Raw strings (`r"..."`,
  `r#"..."#`) and block comments (`/* ... */`) are out of scope. Fixes
  every rule that honours comment directives (built-in, declarative and
  the scope-graph rules), which all go through the one shared lookup
  (`utils::allowance::directives_for_line`)
- **`#[arch_lint::allow(...)]` now recognizes a kebab-case rule name.**
  `parse_allow_attr_tokens` rendered the attribute's token stream with
  `to_string()` before matching against it, so `deny-scope-dep` — five
  tokens, `deny`, `-`, `scope`, `-`, `dep` — printed as `deny - scope -
  dep` and never matched anything; only the `snake_case` spelling
  (`deny_scope_dep`) or a rule code (`ALD003`) worked in attribute form,
  while the comment form (`// arch-lint: allow(deny-scope-dep)`) already
  accepted both, as the README promised for every form. The parser now
  walks `syn::MetaList::tokens` structurally instead: each
  comma-separated argument is either `reason = "<literal>"` (the literal
  read through `syn::Lit`, so a raw string `r"..."` works too) or a rule
  name built by concatenating its `Ident`/`Punct('-')`/`Punct(':')`
  tokens in order, plus a digit-only `Literal` segment (so a scope name
  like `core-2` — scope names may contain `[a-z0-9-]` — comes through,
  e.g. `allow-dep:core-2`) — the latter also makes the `allow-dep:<scope>`
  display name (`[[allow-dep]]`, ALD011) work in attribute form for the
  first time. At most one `:` is accepted, with a name on each side (so
  `foo::bar` is dropped, not read as a name). An argument that is neither
  shape (stray punctuation, a group) is dropped rather than rejecting the
  whole attribute. Rule-name
  normalization (`-` vs. `_`) is unchanged, at the existing
  comparison/storage sites (`check_arch_lint_allow`,
  `EdgeSite::with_allow_directive`), so this only fixes what the parser
  hands them

## [0.6.0] - 2026-09-11

### Added

- **`arch_lint::Session` - the SDK driver for the syn engine.** The
  "root + config → `LintResult`" pipeline that `check!()` and the CLI each
  reimplemented is now one public, non-panicking API.
  `Session::discover(root)` resolves the config file
  (`<root>/arch-lint.toml` → `<root>/.arch-lint.toml` → `$ARCH_LINT_CONFIG_DIR`
  or `~/.arch-lint/` `config.toml` → defaults) and records the winner in
  `ConfigSource`, reachable via `session.config_source()`.
  `Session::from_config_path(root, path)` and
  `Session::from_config_str(root, toml)` cover explicit and in-memory
  configuration, and `Session::from_source(root, source)` takes an
  already-resolved `ConfigSource`. Overrides are builder-style —
  `with_preset`, `with_fail_on`, `with_rules`, `with_rule_filter` (names or
  codes; unknown names are `SessionError::UnknownRule`) and `with_exclude` —
  alongside the `root()` / `config()` / `config_source()` / `preset()` /
  `fail_on()` accessors. `run()` returns `Result<LintResult, SessionError>`;
  `SessionError` is a `thiserror` enum that preserves the underlying config,
  declarative-rule and analyzer errors as sources.
  `Session::find_workspace_root(path)` exposes the `[workspace]` walk that
  `check!()` uses
- **`arch_lint::report::render`** - renders a `LintResult` as text, JSON or
  compact output, the same three formats the CLI emits. ANSI colours are
  opt-in through `Format::Text { color }` and default to off, so library
  callers and captured output stay plain; the JSON is the same
  `serde_json::to_string_pretty` document as `--format json`
- **`arch_lint_ts::TsSession`** - the same driver shape for the tree-sitter
  engine. `TsSession::new(config, root)` starts with the Kotlin extractor
  registered, `.with_extractor(..)` adds languages, `.respect_gitignore(bool)`
  overrides the config, and `.run()` validates the config, discovers files,
  runs the engine and returns a sorted `LintResult` or a `TsSessionError`
- **`[analyzer] respect_gitignore` for the tree-sitter engine** (default
  `true`), matching the syn analyzer. File discovery uses the same ignore
  sources with `require_git(false)`, so the rules apply outside a git
  repository too
- **`arch_lint_core::AnalyzerError` is re-exported** from the crate root, so
  callers can match on analyzer failures rather than only print them
- **`[analyzer] respect_gitignore` and `include` are now honored** - file
  discovery walks the root with the `ignore` crate instead of a bare
  `**/*.rs` glob. With the default `respect_gitignore = true`, gitignored
  files are skipped (`.gitignore` at any level, `.git/info/exclude`, global
  gitignore); ignore rules apply even outside a git repository, so scaffolded
  projects and fixture directories are filtered the same way. A non-empty
  `include` restricts analysis to matching files, and `Cargo.toml` discovery
  for project-wide rules follows the same gitignore and `exclude` settings.
  `include` / `exclude` patterns now match the path **relative to the root**
  as well as the absolute path, so `exclude = ["src/generated/**"]` works as
  written. New `AnalyzerBuilder::respect_gitignore(bool)` and
  `AnalyzerBuilder::includes(..)` methods; an explicit builder call overrides
  the configured `respect_gitignore`. `[analyzer] parallelism` remains parsed
  but unused
- **Suppression for declarative rules** - ALD001-ALD008 now honor the same
  directives as the built-in rules: `#[arch_lint::allow(<name>, reason = "...")]`
  on the offending item or an enclosing `mod` / `fn` / `impl`, and
  `// arch-lint: allow(<name>) reason="..."` on the violating line or the line
  above it. `<name>` may be the rule kind (`restrict-use`), the TOML rule name
  (`no-sqlx-in-domain`) or the code (`ALD001`), in kebab-case or `snake_case`;
  a TOML name suppresses only that rule. Error-severity rules require a reason
  on the comment path, reporting the same "missing required reason" warning as
  the built-in rules. Declarative rules keep firing inside `#[cfg(test)]`
- **ALD004 `[[layering]]`** - declare an ordered stack of scopes
  (`order = ["handler", "domain", "infra"]`); a layer may only depend on layers
  below it, and `adjacent_only = true` also forbids skipping a layer. Replaces
  the hand-written `[[deny-scope-dep]]` matrix
- **ALD005 `[[deny-attr]]` / ALD006 `[[require-attr]]`** - forbid or require
  attributes on items of a scope (`attrs = ["derive(sqlx::FromRow)"]`,
  `attrs = ["tracing::instrument"]`), narrowed by `on = ["struct", "fn", ...]`
  and, for `require-attr`, by `visibility`. `deny-attr` defaults to `error`,
  `require-attr` to `warning`
- **ALD007 `[[naming]]`** - require identifiers of one item kind to match a
  regex (`item = "trait"`, `pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"`);
  defaults to `warning`
- **ALD008 `[[require-return]]`** - require functions to return a type matching
  a regex, with optional `async` and `visibility` filters; the return type is
  normalized (`Result < T , E >` → `Result<T, E>`, no return type → `()`)
  before matching
- **`except` on `[[restrict-use]]`** - a path matching it is not reported even
  when it matches `deny`
- **`exclude` on `[[scopes]]`** - a file matching it is not part of the scope
  even when it matches `paths`; applies to every rule kind using that scope

- **Per-rule options are now honored** - `[rules.<name>]` keys documented in the README
  (`allow_in_tests`, `allow_expect`, `allow_patterns`, `max_handler_lines`, `max_match_arms`,
  `max_enum_variants`, `patterns`, `runtime_mode`, `require_*_docs`, `allow_ok`,
  `allow_let_underscore`) are applied when building preset rules, both in `check!()` and the CLI.
  Previously they were parsed and ignored
- **`exclude_files`** per-rule option - skip a rule for files matching the given globs
  (relative to the analyzer root)
- **CLI `check` parity with `check!()`** - reads `preset` and `fail_on` from `arch-lint.toml`,
  loads declarative rules (`[[restrict-use]]`, `[[require-use]]`, `[[deny-scope-dep]]`),
  and exits non-zero on violations at or above `fail_on`
- Rule registry (`arch_lint_rules::rule_by_name` / `rule_from_config` / `canonical_rule_name`)
  and `FromStr` for `Preset` / `Severity`
- CI workflow (rustfmt, clippy, per-crate tests, MSRV, rustdoc, cargo-deny), `deny.toml`,
  `CONTRIBUTING.md`
- **`arch-lint schema [--out <path>]`** - prints a JSON Schema (draft 2020-12) of
  `arch-lint.toml`: the top-level keys, `[analyzer]`, every built-in rule's
  `[rules.<name>]` table (common `enabled` / `severity` / `exclude_files` plus the
  rule's own option keys, each with its documentation and preset-dependent
  default) and the declarative sections. Point taplo / Even Better TOML at the
  generated file with `#:schema ./arch-lint.schema.json` for completion and
  typo warnings. Built as `arch_lint::schema::config_json_schema()` for SDK use
- **Typed per-rule options** - each built-in rule now has an options struct
  (`arch_lint_rules::options::*Options`, `JsonSchema`-derived), exposed through
  `rule_option_schemas()` / `rule_option_schema(name_or_code)` and
  `validate_rule_tables(&Config)`. `arch-lint list-rules` prints every rule's
  option keys with type and default; `list-rules --json` prints the registry
  with each rule's option schema
- `JsonSchema` derives on `Config`, `AnalyzerConfig`, `RuleConfig`, `Severity`
  and the declarative config DTOs (new `schemars` dependency)

### Changed

- **CLI colours are emitted only when stdout is a terminal.** `--format text`
  severity labels and the summary line, and the `tracing` log lines, are plain
  when the output is piped or redirected. Terminal output is unchanged
- **The CLI is now a wrapper over the facade.** `arch-lint-cli` depends on
  `arch-lint` and delegates to `Session` and `report::render`;
  `src/config_resolver.rs` and `src/commands/output.rs` moved into
  `arch_lint::session` and `arch_lint::report`, and `commands/check_ts.rs`
  delegates to `arch_lint_ts::TsSession`. Behaviour, exit codes and output are
  unchanged apart from the colour policy above; error messages may now carry
  additional `SessionError` context
- **The tree-sitter `exclude` patterns are matched as globs** against the
  root-relative path (`src/test/**` now works as written). The previous
  substring behaviour is kept as a fallback, so existing configs such as
  `**/generated/**` keep matching
- **`strict` preset now includes every rule** (AL009-AL012 were missing); `list-rules` and
  `--rules` now accept AL009-AL012 as well
- `--rules` with an unknown name is an error instead of a silent warning
- Engine auto-detection parses the config as TOML; a commented-out `[[layers]]` no longer
  switches to the tree-sitter engine
- `LanguageExtractor::analyze` returns `Result<FileAnalysis, ExtractError>` instead of
  panicking when the grammar fails to load
- `rust-version` raised to 1.88 (required by current `ignore` / `home` releases)
- `arch-lint init` template: fixed docs URL and `max_handler_lines` key, added `preset` /
  `fail_on` / `exclude_files` examples
- Makefile `publish` targets include `arch-lint-ts` and the `arch-lint` facade;
  `bump-version` works with GNU sed
- **Built-in rules share their visitor scaffolding** - the `#[arch_lint::allow(...)]` /
  `#[cfg(test)]` / `#[test]` suppression state that nine rules each tracked by hand now lives
  in `arch_lint_core::utils::scope::ScopeTracker`, and their `visit_item_mod` /
  `visit_item_fn` / `visit_item_impl` overrides delegate to it. New
  `arch_lint_core::test_utils` (behind the `test-utils` feature) replaces the `FileContext`
  literal that was copied into every rule's test module. No change to rule behavior, codes,
  messages, severities, or public rule APIs
- **`Preset::rules_with_config` and `rule_from_config` are fallible** - they return
  `Result<_, RuleOptionsError>` so that an invalid `[rules.<name>]` table surfaces
  as an error; `Session` maps it to the new `SessionError::RuleOptions`. An
  unrecognised `runtime_mode` value for `async-trait-send-check` is now an error
  instead of silently falling back to the default

### Removed

- Unused dependencies (`tokio`, `walkdir`, `insta`, and per-crate `serde` / `miette` /
  `tracing` / `ignore` where unused)
- Dead, unexported modules `prefer_utoipa` and `require_tracing_v2` in `arch-lint-rules`

### Fixed

- Unknown keys in `[analyzer]`, in every declarative rule table (`[[scopes]]`,
  `[[restrict-use]]`, ..., `[[require-return]]`) and in every built-in
  `[rules.<name>]` table are now a config error instead of being silently
  ignored, so a typo like `denyy = [...]` or `allow_in_test = true` no longer
  disables a rule. `[rules.<name>]` tables are validated even when the rule is
  not part of the active preset; tables for custom rules stay free-form
- AL002 `no-sync-io` no longer reports `is_file()` / `is_dir()` on `FileType` /
  `Metadata` values (results of `file_type()`, `metadata()`, `symlink_metadata()`,
  `fs::metadata(..)`, also through `?` / `unwrap()`, `let` bindings and closure
  parameters); only the path-receiver form performs I/O
- Documentation drift: README rule table lists AL013 and the declarative ALD001-ALD003 codes;
  ARCHITECTURE.md reflects the facade / macros / declarative rules; broken `CONTRIBUTING.md` /
  `LICENSE` links; facade crate doc pointed at `arch-lint = "0.4"`

## [0.5.0] - 2026-08-22

### Added

- **Inline qualified-path checking** for declarative rules (ALD001 `restrict-use`, ALD003 `deny-scope-dep`)
  - References like `sqlx::query(...)` or `crate::infra::Db::new()` written without a `use` statement are now detected, in expression, type, trait-bound, and macro positions
  - New per-rule option `check-inline` (default: `true`); set `check-inline = false` to restore the previous `use`-only behavior
  - `self::` / `super::` relative paths and single-segment local names remain out of scope (unchanged v1 limitation)

## [0.4.3] - 2026-02-20

### Fixed

- AL007 `tracing-env-init`: false positive on `EnvFilter::try_from_default_env()` fallback patterns

## [0.4.2] - 2026-02-20

### Added

- **AL013: no-silent-result-drop** - Forbids silently discarding `Result` error information
  (`let _ = result`, `.ok()` without handling). Included in the `recommended` preset;
  `strict` also forbids `.ok()` and test exceptions

## [0.4.1] - 2026-02-18

### Changed

- README rewritten around the day-1 adoption flow (`check!()` + `arch-lint.toml`) and
  the declarative rules showcase

## [0.4.0] - 2026-02-17

### Added

- **`arch_lint::check!()` macro** - runs arch-lint as a `#[test]` inside `cargo test`,
  with `preset` / `config` / `fail_on` arguments and workspace-root detection
- **Declarative architecture rules** in `arch-lint.toml`:
  - `[[scopes]]` - named path sets
  - ALD001 `[[restrict-use]]` - forbid `use` paths within a scope
  - ALD002 `[[require-use]]` - require a preferred crate over alternatives
  - ALD003 `[[deny-scope-dep]]` - forbid one scope depending on another
- `arch-lint` facade crate re-exporting core, rules, and macros
- **Global config fallback** - `ConfigResolver` falls back to `~/.arch-lint/arch-lint.toml`
  (or `$ARCH_LINT_CONFIG_DIR`) when no project config is found

### Fixed

- `ScopeDep.name`, `mod.rs` fallback for module paths, overlapping scope handling
- Workspace detection uses TOML parsing instead of string matching

## [0.3.0] - 2026-02-05

### Added

- **Tree-sitter engine** (`arch-lint-ts` crate) - Cross-language architecture layer enforcement
  - `LanguageExtractor` trait for pluggable language support
  - `KotlinExtractor` for Kotlin `.kt`/`.kts` files (package, imports, declarations)
  - `LayerResolver` with longest-prefix-match package-to-layer mapping
  - `ArchRuleEngine` with three rule types:
    - LAYER001 (layer dependency) - forbidden cross-layer imports
    - PATTERN001 (import pattern) - forbidden import path patterns per layer
    - NAMING001 (naming rule) - naming convention-based access control
  - `ArchConfig` TOML parser with `[[layers]]`, `[dependencies]`, `[[constraints]]`
  - Config validation (unknown layers, self-dependency, missing entries)
  - 30 unit tests

- **CLI dual engine support**
  - `--engine ts|syn` flag for explicit engine selection
  - Auto-detection from config: `[[layers]]` present triggers tree-sitter engine
  - `arch-lint init --ts` generates tree-sitter config template
  - `arch-lint check` dispatches to correct engine automatically

- **Documentation**
  - `docs/tree-sitter-engine.md` - Comprehensive tree-sitter engine guide

## [0.2.0] - 2025-12-14

### Added

- **AL009: async-trait-send-check** - Checks proper usage of `async_trait` Send bounds
  - Detects `#[async_trait]` without `?Send` in single-threaded contexts
  - Suggests using `#[async_trait(?Send)]` for single-threaded environments
  - Configurable runtime mode (single-thread/multi-thread)

- **AL010: prefer-from-over-into** - Prefers `From` trait implementation over `Into`
  - Detects `impl Into<T> for U` patterns
  - Recommends implementing `From` instead (automatically provides `Into`)
  - Follows Rust conventions and best practices

- **AL011: no-panic-in-lib** - Forbids panic macros in library code
  - Detects `panic!`, `todo!`, `unimplemented!`, `unreachable!` in library code
  - Allows panic macros in test code (configurable)
  - Suggests proper error handling with `Result` types
  - Default severity: Error

- **AL012: require-doc-comments** - Requires documentation comments on public items
  - Requires `///` documentation on public functions, structs, and enums
  - Configurable per item type (functions/structs/enums)
  - Improves API documentation quality

- **Issue Templates**
  - Added GitHub Issue template for rule proposals
  - Added GitHub Issue template for bug reports
  - Added configuration for community discussions

### Changed

- Bumped version to 0.2.0
- Improved code quality with clippy fixes

## [0.1.0] - 2025-12-14

### Added

- Initial release of arch-lint
- Core architecture linting framework
- **AL001: no-unwrap-expect** - Forbids `.unwrap()` and `.expect()` in production code
- **AL002: no-sync-io** - Forbids blocking I/O in async contexts
- **AL003: no-error-swallowing** - Forbids catching errors without propagation
- **AL004: handler-complexity** - Limits complexity of handler functions
- **AL005: require-thiserror** - Requires `thiserror` derive for error types
- **AL006: require-tracing** - Requires `tracing` crate instead of `log` crate
- **AL007: tracing-env-init** - Prevents hardcoded log levels in tracing initialization
- CLI tool with configuration support
- Allow directives via attributes and inline comments
- Comprehensive test coverage

[Unreleased]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.7.0...HEAD
[0.7.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.6.0...arch-lint-v0.7.0
[0.6.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.5.0...arch-lint-v0.6.0
[0.5.0]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.3...arch-lint-v0.5.0
[0.4.3]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.2...arch-lint-v0.4.3
[0.4.2]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.1...arch-lint-v0.4.2
[0.4.1]: https://github.com/ynishi/arch-lint/compare/arch-lint-v0.4.0...arch-lint-v0.4.1
[0.4.0]: https://github.com/ynishi/arch-lint/compare/v0.2.0...arch-lint-v0.4.0
[0.3.0]: https://crates.io/crates/arch-lint/0.3.0
[0.2.0]: https://github.com/ynishi/arch-lint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ynishi/arch-lint/releases/tag/v0.1.0
