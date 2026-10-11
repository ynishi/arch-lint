# Contributing to arch-lint

Conventions for changes to this repository, for people and coding agents
alike. The disclosure and public-artifact policy in
[PUBLIC_DEVELOPMENT.md](PUBLIC_DEVELOPMENT.md) outranks this file.

## Prerequisites

- Rust 1.88 or newer (the workspace `rust-version`)
- `rustfmt` and `clippy` components (`rustup component add rustfmt clippy`)

## Issues

Work starts from a GitHub issue. Open one for anything that is more than a
typo: a bug, a feature, a refactor, a doc gap. An issue records the problem and
the evidence; the pull request records what was done about it.

### Labels

Assign exactly one when you open an issue. The branch prefix follows the label.

| Label           | The change                                     | Branch      |
| --------------- | ---------------------------------------------- | ----------- |
| `bug`           | behaviour contradicts what it promises         | `fix/`      |
| `enhancement`   | behaviour that does not exist yet              | `feat/`     |
| `refactor`      | behaviour unchanged                            | `refactor/` |
| `chore`         | production code untouched: CI, tests, tooling  | `chore/`    |
| `documentation` | prose only (README, doc comments)              | `docs/`     |

### What an issue says

Use [`.github/ISSUE_TEMPLATE/issue.md`](.github/ISSUE_TEMPLATE/issue.md)
(Problem / Evidence / Proposal / Acceptance). Its comments are the rules for
each section; they are not repeated here. The bug-report and rule-proposal
forms in the same directory are shortcuts that end up in the same shape.

## Branches

Never work on `main`. One branch per issue, named `<type>/<slug>` with the
prefix from the table above. How you keep `main` available while you work on it
(a worktree, a second clone, stashing) is your own business, and so is cleaning
it up afterwards.

## Local checks

The same checks run in CI (`.github/workflows/ci.yml`):

```bash
make fmt-check     # cargo fmt --all -- --check
make clippy        # cargo clippy --workspace -- -D warnings
make test          # cargo test --workspace
make preflight     # all of the above
cargo +1.88 check --workspace --locked          # MSRV, against the committed Cargo.lock
cargo doc --workspace --no-deps --exclude arch-lint-cli
```

Prefer `cargo test -p <crate>` while iterating on a single crate.

`Cargo.lock` is committed and the workspace uses the MSRV-aware resolver
(`resolver = "3"`). A dependency bump is a commit of its own with the
`Cargo.lock` diff in it; do not let an unrelated change carry one.

## Verification

Green is a precondition, not a verification. A change to a rule, the analyzer,
the config loader or the CLI is run against a real project before the pull
request is opened, through the binary a user installs, not the one the worktree
happens to have built:

```bash
cargo install --locked --path crates/arch-lint-cli
cargo arch-lint check examples/sample-project/src    # 2 AL001 errors (plus warnings), exit 1
```

`target/debug/arch-lint` is not that binary. `examples/sample-project` exists
so that not having a project at hand is not a reason to skip this; a change
that the sample does not exercise gets a fixture under the relevant crate's
`tests/` instead. What the pull request says about it is the Checks section of
[`.github/pull_request_template.md`](.github/pull_request_template.md).

## Adding a rule

1. Create `crates/arch-lint-rules/src/<rule_name>.rs` implementing
   `arch_lint_core::Rule` with unit tests. Track `#[arch_lint::allow(...)]` /
   `#[cfg(test)]` suppression with `arch_lint_core::utils::ScopeTracker` rather
   than hand-rolled `in_allowed_context` flags, and write tests against
   `arch_lint_core::test_utils::check_rule` (enabled by the crate's
   `test-utils` dev-dependency feature) rather than building a `FileContext`
   by hand.
2. Export it from `crates/arch-lint-rules/src/lib.rs`.
3. Register it in `crates/arch-lint-rules/src/presets.rs`: add the name to
   `ALL`, map its code in `canonical_rule_name`, and wire any `[rules.<name>]`
   options in `build_rule`. This is what `list-rules`, `--rules`, and the
   `strict` preset use, so a rule missing here is invisible to users.
4. Document it in `docs/rules.md` (rule table + "Rule Details") and
   `CHANGELOG.md` under `[Unreleased]`.

Declarative rules (`ALD*`) live in `crates/arch-lint-core/src/declarative/`:
one DTO in `config_dto.rs`, validation in `loader.rs`, a model type under
`model/`, and the visitor under `rules/`. `arch-lint schema` must reflect the
new table; `crates/arch-lint-cli/src/commands/schema.rs` has the tests that
catch a missing one.

## Documentation

The reference lives in the code, rustdoc style: the crate-level `//!` docs say
what the crate is for and how its parts fit, each type and function documents
what it promises, and `cargo doc` is where a reader is sent. A new flag is done
when its clap doc says what it does; a new rule when its `description()` and
its `docs/rules.md` entry do; a new config key when its field doc does.

README.md is the front door, not a reference: what arch-lint is, how to install
it, one example of each way in, and links to the rest. Write a rule once, where
the thing it constrains is defined, and link to it from anywhere else; a second
copy is the one nobody updates.

`CHANGELOG.md` is kept by hand under `[Unreleased]`, one entry per user-visible
change, moved under a version heading at release time.

## Commits

The subject line starts with the type from the label table (`feat:`, `fix:`,
`refactor:`, `chore:`, `docs:`); the body says why, in sentences. Formatting
and clippy fixes go in their own commits. Never commit `workspace/`, `target/`
or local agent state; if a commit needs `git add -f`, stop, something is filed
wrong.

On 0.x, cargo treats the minor as the compatibility unit. A change to a
published crate's public API (a signature, a public field, a trait method) is a
`feat:` and lands in a minor release, never a patch; the CHANGELOG entry names
what moved.

## Pull requests

One issue per pull request, against `main`. Before opening it, run
`make preflight` and the MSRV check on the final tree, plus the installed-binary
run above when the change calls for one.

Use [`.github/pull_request_template.md`](.github/pull_request_template.md) for
the body. Its comments are the rules for each section; they are not repeated
here. Longer bodies are easier to write as a file and pass with `--body-file`;
`workspace/` is gitignored and a fine place for one.

A pull request lands as a merge commit; every commit on the branch reaches
`main`, so a commit is a step of the work, not something to squash away.

## Releasing

Releases are manual: bump the workspace version, move `[Unreleased]` under the
new heading in `CHANGELOG.md`, merge, then tag `arch-lint-v<version>` and push
the tag. That push is what starts
[`.github/workflows/release.yml`](.github/workflows/release.yml): it builds
the prebuilt binaries for all six supported targets and opens a **draft**
GitHub Release with them attached — it never touches crates.io. Only once
that draft looks right does `make publish` run (crates in dependency order:
macros, core, rules, ts, arch-lint, cli); undraft the release afterwards, once
its assets (six archives, each with its own `.sha256` file — twelve assets)
have been checked. Only a maintainer does any of this.

## License

By contributing you agree that your contributions are licensed under
MIT OR Apache-2.0, matching the project.
