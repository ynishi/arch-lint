<!--
This file is the rule for a pull request's body, not only its shape.
CONTRIBUTING.md links here instead of restating it.

Fill in every section and delete the comments. Do not leave a section empty:
write "none" and say why.
-->

## What changed

<!--
What the change does, in terms a reader of the code can check: the types,
functions and files it touches, and what each now does. Then why this approach
and not the alternative the issue raised; give the alternative and the reason.
-->

## Acceptance

<!--
One row per acceptance criterion of the issue named in `Refs #N` below. Quote
each criterion word for word and number it as the issue does. A criterion this
PR does not meet goes under "Not covered", not here.

- "Expected" is the correct output, written BEFORE running anything, from the
  criterion and the docs. Writing it after looking at the output approves
  whatever came out.
- "Actual" is what the installed binary (`cargo install --locked --path
  crates/arch-lint-cli`) or `cargo test` printed: the lines themselves, minus
  machine paths (PUBLIC_DEVELOPMENT.md). Not "the output was correct".
- "Fails before" applies to a fix: does the same check fail on the build before
  this change, and what did it print there?

Not evidence: "unchanged from the previous build" (shows nothing is right),
"tests pass" on its own (say which test checks which criterion).
-->

| # | Criterion (quoted from the issue) | Expected | Actual | Fails before |
|---|---|---|---|---|
| 1 |  |  |  |  |

## Checks

<!--
One line per item: what was done and what came of it ("`make preflight`:
passed, 557 tests"). "n/a" with a reason when an item does not apply. These are
the work the change needed, not the evidence that it is right; that is the
table above.
-->

- Tests added or changed:
- `make preflight` (fmt, clippy, tests):
- `cargo +1.88 check --workspace --locked` (MSRV):
- Run through the installed binary on a project (CONTRIBUTING.md, Verification):
- Docs updated (README / doc comments / CHANGELOG `[Unreleased]`):

## Not covered

<!--
What this PR deliberately leaves out: criteria of the issue it does not meet,
and problems found on the way that belong elsewhere. Name the issue that takes
each, or say that none does yet.
-->

<!--
The last line is `Refs #N`. Do not write close / closes / fix / fixes /
resolve / resolves before `#N` anywhere in this body: GitHub reads them and
closes the issue on merge, before the acceptance table has been read.
-->
Refs #
