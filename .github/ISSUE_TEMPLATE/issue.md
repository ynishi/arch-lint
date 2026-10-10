---
name: Issue
about: A bug, a feature, a refactor, tooling or docs. Pick the label from CONTRIBUTING.md.
---

<!--
This file is the rule for an issue's body, not only its shape. CONTRIBUTING.md
links here instead of restating it.

Fill in every section and delete the comments. Give the issue exactly one label
from CONTRIBUTING.md's table (bug / enhancement / refactor / chore /
documentation); the branch prefix follows it.

What may appear in a public artifact is PUBLIC_DEVELOPMENT.md's subject. The
short version: describe a project the evidence came from by its shape, never by
its name; no machine paths anywhere, quoted output included; write so that
somebody who has never seen your machine can act on it.
-->

## Problem

<!--
What happens, on what input. For a bug, the command and the output, verbatim.
For a report from running arch-lint on a project, the project's shape (crates,
lines, which rules / preset, which arch-lint version) and not its name. For an
enhancement, what cannot be done today. For a refactor, what in the code is
wrong while the behaviour is right.
-->

## Evidence

<!--
Measured, not assumed. A claim read from the code gives `file:line` and says
it was read, not reproduced. A timing says which build (debug or release)
produced it and on what. A comparison with another tool links the page it
came from.
-->

## Proposal

<!--
Optional. Say which kind of answer the issue asks for: a patch, an architecture
change, a narrower requirement, or "not now". Alternatives and why they lose
belong here too.
-->

## Acceptance

<!--
A numbered list. Each item is a criterion somebody can check, stated as what
`arch-lint check` / `check!()` / the library prints or does when the issue is
done, with the expected output where there is one.

The pull request quotes these items by number in its Acceptance table and
shows, for each, the expected and the actual output
(.github/pull_request_template.md). "Works better" or "is cleaner" is not a
criterion; say what would be printed instead.

For a refactor, one item is "behaviour unchanged", with what is compared to
show it.
-->

1. 
