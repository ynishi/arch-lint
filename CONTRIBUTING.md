# Contributing to arch-lint

Thanks for your interest in improving arch-lint. This document covers the
local workflow and what CI expects.

## Prerequisites

- Rust 1.88 or newer (the workspace `rust-version`)
- `rustfmt` and `clippy` components (`rustup component add rustfmt clippy`)

## Local checks

The same checks run in CI (`.github/workflows/ci.yml`):

```bash
make fmt-check     # cargo fmt --all -- --check
make clippy        # cargo clippy --workspace -- -D warnings
make test          # cargo test --workspace
make preflight     # all of the above
cargo doc --workspace --no-deps --exclude arch-lint-cli
```

Prefer `cargo test -p <crate>` while iterating on a single crate.

## Adding a rule

1. Create `crates/arch-lint-rules/src/<rule_name>.rs` implementing
   `arch_lint_core::Rule` with unit tests.
2. Export it from `crates/arch-lint-rules/src/lib.rs`.
3. Register it in `crates/arch-lint-rules/src/presets.rs`: add the name to
   `ALL`, map its code in `canonical_rule_name`, and wire any `[rules.<name>]`
   options in `build_rule`. This is what `list-rules`, `--rules`, and the
   `strict` preset use, so a rule missing here is invisible to users.
4. Document it in `README.md` (rule table + "Rule Details") and `CHANGELOG.md`
   under `[Unreleased]`.

## Pull requests

- Keep commits focused; one logical change per PR.
- Add or update tests for behavior changes.
- Update `CHANGELOG.md` for user-visible changes.

## Reporting issues

Use the issue templates under `.github/ISSUE_TEMPLATE/` for bug reports and
rule proposals.

## License

By contributing you agree that your contributions are licensed under
MIT OR Apache-2.0, matching the project.
