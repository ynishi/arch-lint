# Public development and information boundaries

arch-lint is developed in public. This document says what a public artifact
must contain to be worth reading, and what must not cross into one. It outranks
[CONTRIBUTING.md](CONTRIBUTING.md), and it is the policy a contributor or a
coding agent applies before writing an issue, a pull request, a commit message
or documentation.

## Default

Everything about arch-lint is public by default: the design, the code, the
plans, the measurements and the failures. That includes work that went nowhere
and decisions that were reversed, which are usually the parts worth reading.

What is private is what is not arch-lint's to publish: the contents of a
project that uses arch-lint and is not itself public, credentials, and
non-public security reports. Private does not mean important. It means somebody
else has not agreed to publication, or publishing would expose a credential or
an unfixed vulnerability.

## Two questions, deliberately separate

A public artifact is checked for two different things, and only one of them is
an incident:

- **Confidentiality**: does it disclose something that was not ours to disclose?
- **Completeness**: can a reader who has never seen the author's machine
  understand it and act on it?

An opaque reference is not a leak. It is a public artifact that fails the second
question, which is its own defect: it costs every reader something and gives
them nothing.

## Classification

### ALLOW

These belong in public issues, pull requests, commits, comments and docs:

- the design, the alternatives, the decision and what it cost;
- measurements, with what they were measured on and which build produced them;
- diagnostics and program output, quoted verbatim;
- `file:line` into this repository, and the quoted line;
- a description of a project that is not public, by its shape: "a 4-crate
  workspace of ~30,000 lines with `[[scopes]]` for domain / app / infra";
- an environment fact that is a property of the tool rather than of one
  machine (an environment variable arch-lint reads, a Cargo profile, the MSRV).

### WARN

Not disclosures, not blocked, worth fixing because the artifact does not stand
on its own without the fix:

- an identifier only the author can resolve, in place of the description a
  reader needs: the name of a project that is not public, a directory layout
  particular to one checkout, a symbol prefix belonging to one codebase;
- evidence that exists only on the author's machine ("on my project it
  reports 40 violations") without the shape that says whether it applies to
  the reader;
- a rationale that lives only in a private note, leaving the public artifact
  with the conclusion and none of the argument.

The fix is to add what the reader needs. When the opaque token carried nothing
to begin with, drop it; the description beside it was doing the work.

Do not build a review process to guarantee that no WARN is ever published.
Finding and fixing incomplete context is ordinary documentation work.

### BLOCK

These must not be copied into a public artifact:

- the contents of a project that is not public (its source, its config, its
  lint output with file names that reveal structure) unless each part has been
  cleared for publication on its own;
- credentials, tokens, signed URLs, and URLs carrying access-bearing parameters;
- material covered by somebody else's confidentiality;
- non-public vulnerability details, before there is a fix to disclose;
- absolute paths on a machine (`/Users/…`, `/home/…`, `C:\Users\…`). They name
  a person's account and resolve nowhere else.

Found before publication: remove it and carry on; nothing else is owed. Found
after: see [After a disclosure](#after-a-disclosure).

## What each artifact is for

- **Issue**: the problem, the evidence, the alternatives and the acceptance
  criteria ([`.github/ISSUE_TEMPLATE/issue.md`](.github/ISSUE_TEMPLATE/issue.md)).
- **Pull request and commit**: what changed, why this and not the alternative,
  and for each acceptance criterion the expected and the actual output
  ([`.github/pull_request_template.md`](.github/pull_request_template.md)).
- **Code documentation**: the current contract, what a type is, what a function
  promises, and why it is built that way.
- **README**: the front door, what arch-lint is, how to install it, one example
  of each way in, and where the reference is.

A reader with this repository and nothing else should be able to follow all
four.

## Writing up evidence from a project that is not public

Most findings about a linter come from running it on something, and that
something is usually not published. The report is still expected to stand on
its own:

- **Give the shape**: how many crates and lines, which rules or preset, which
  arch-lint version produced the result.
- **Quote the output verbatim**, minus machine paths; replace revealing file
  names with stand-ins and say that they are.
- **Name the arch-lint-side location** (`crates/…/file.rs:120`, a rule code, a
  CLI flag), since that is what the reader can go and look at.
- **Do not name the project.** It cannot be cloned or searched, so it answers
  no question a reader has.

## Agent publication check

Before writing anything destined for the remote, in this order:

1. **BLOCK**: does the draft contain any of the above?
2. **Completeness**: can a reader who has never seen this machine understand
   the problem and act on it? If the answer rests on a name only the author
   knows, replace the name with what it stands for.
3. **Machine paths**: none, anywhere, including in quoted output.
4. **Redistribution**: does the change commit a file that originated elsewhere?
   That is a licensing question; state its origin and terms beside the file,
   or do not commit it.

This check is deliberately narrow. It is not a search for every string that
looks private.

## After a disclosure

Once protected content is published, it is disclosed. Deleting the commit,
force-pushing over it, or making the repository private does not undo that,
and treating any of them as the fix is the mistake this section exists to
prevent.

If the exposed thing can be **invalidated** (a credential, a token, a signed
URL), rotate it first, before touching history. Rewriting history afterwards is
a tidiness decision: it does not reach other people's clones.

If it **cannot** be invalidated (somebody's data, another party's confidential
material, an unfixed vulnerability), the work moves from the file to the
people affected: what was exposed, over what window, and how far it could have
travelled.

arch-lint publishes crates. `cargo yank` is not deletion: the version stays
downloadable and existing lockfiles keep resolving to it.

Write down the exposure window, the reach, and every takedown attempted. Where
that record is public it is an issue, and it closes when everything it
describes is done.
