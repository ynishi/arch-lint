# Declarative Rules (No Code Required)

Define architecture constraints directly in `arch-lint.toml`. When a review catches a pattern you want to prevent, add a TOML block — no Rust code needed. [Back to README](../README.md).

## Scope-Based Dependency Control

```toml
# Define architectural scopes
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[scopes]]
name = "infra"
paths = ["src/infra/**"]

[[scopes]]
name = "handler"
paths = ["src/handler/**"]

# Domain must not import infrastructure
[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain layer must not depend on infrastructure. Use ports/adapters."

# Domain must not use database crates directly
[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::*", "diesel::*", "sea_orm::*"]
message = "Database access belongs in the infra layer."
```

Both `[[restrict-use]]` and `[[deny-scope-dep]]` check `use` imports **and**
inline qualified paths (e.g. `sqlx::query(...)` or `crate::infra::Db::new()`
written without a `use`). To restrict a rule to `use` statements only, set
`check-inline = false` on that rule.

`[[restrict-use]]` also resolves the names a `use` brings into scope, so a deny
that points deeper than `use` can import still catches the short form. With
`deny = ["uuid::Uuid::new_v7"]`, `use uuid::Uuid;` followed by
`Uuid::new_v7()` is reported (as `` `Uuid::new_v7` (resolves to
`uuid::Uuid::new_v7`) ``), and so are `use uuid::Uuid as U;` with `U::new_v7()`
and `use chrono::{self, Utc};` with `Utc::now()` against `chrono::Utc::now`.
Names are looked up the way Rust does it: a `mod` does not see the imports of
the enclosing module, a block sees them and its own `struct` / `enum` /
`trait` / `type` / `mod` (or a type parameter) hides one of the same name,
and `Self` in an `impl` stands for the self type.

A path is matched as written first, then through the import. The path through
the import is tried only when the import itself (`uuid::Uuid`) is matched by
neither `deny` nor `except` of that rule: the `use` line has already decided
it. So with `deny = ["sqlx::**"]`, `use sqlx::Pool;` is the one report and
`Pool::connect()` after it adds none, and an allow on the `use` line still
covers the places the name is used. A path is reported once.

What follows from that, and what is not followed:

- `except` is asymmetric. With `deny = ["uuid::**"]` and
  `except = ["uuid::Uuid"]`, `use uuid::Uuid;` makes `Uuid::new_v7()`
  acceptable, while `uuid::Uuid::new_v7()` written in full is reported. To
  forbid a function of a type, deny it by name rather than excepting the type.
- A name that resolves to an item of the same file (`mod uuid { .. }`) is
  matched as written only, so it can hit a deny for an external crate of the
  same name, as before.
- Glob imports (`use uuid::*;`, `use super::*;` in a nested `mod`), chains of
  `use super::X` / `use crate::X`, `pub use` re-exports, type aliases,
  qualified self types (`<Uuid>::new_v7()`), method call syntax and the inside
  of macros are not resolved. A glob in an inner block can hide a named import
  of the outer scope without being noticed; suppress such a report with an
  allow.

`[[restrict-use]]` accepts an `except` list — a path that matches it is never
reported, even when it matches `deny`. `[[scopes]]` accepts an `exclude` list —
a file matching it is not part of the scope even when it matches `paths`, and
that applies to every rule kind that uses the scope:

```toml
[[scopes]]
name = "domain"
paths = ["src/domain/**"]
exclude = ["src/**/tests/**"]   # test helpers are not domain code

[[restrict-use]]
name = "no-db-in-domain"
scope = "domain"
deny = ["sqlx::**"]
except = ["sqlx::types::*"]     # the type wrappers are fine
message = "Database access belongs in the infra layer."
```

## Ordered Layers

```toml
[[layering]]
name = "clean-arch"                    # optional, defaults to "layering"
order = ["handler", "domain", "infra"] # scope names, top to bottom
adjacent_only = false                  # optional, default false
message = "Layers may only depend downward."
severity = "error"                     # optional, default error
check-inline = true                    # optional, default true
```

Each entry of `order` must be a declared `[[scopes]]` name; at least two,
no duplicates. A layer may depend on any layer **below** it — referencing a
layer above it is a violation (ALD004), and with `adjacent_only = true` so is
skipping past the next layer down. One `[[layering]]` replaces the
`[[deny-scope-dep]]` matrix you would otherwise write by hand.

> Use `[[layering]]`, not `[[layers]]`: the latter key selects the
> tree-sitter engine.

## Attributes on Items

```toml
[[deny-attr]]
name = "no-orm-derives-in-domain"
scope = "domain"                       # or files = [...]
attrs = ["derive(sqlx::FromRow)", "serde::Serialize"]
on = ["struct", "enum"]                # optional, default: every kind
message = "Domain types must stay ORM-agnostic."
severity = "error"

[[require-attr]]
name = "instrument-handlers"
scope = "handler"
attrs = ["tracing::instrument"]        # the item must carry at least one
on = ["fn"]
visibility = "pub"                     # optional: "pub" (default) | "any"
message = "Handlers must be instrumented."
severity = "warning"                   # optional, default warning (deny-attr defaults to error)
```

`on` accepts `struct`, `enum`, `fn`, `trait`, `impl`, `mod`, `type`, `const`,
`static`, `union`. Only module-level and impl-level items are inspected —
items declared inside a function body are ignored.

**How an attribute pattern matches.** The attribute's own path is rendered with
`::` (`#[tracing::instrument]` → `tracing::instrument`, `#[derive(..)]` →
`derive`) and compared to the pattern. A pattern written as `derive(P)` matches
only derive lists; a bare pattern matches the attribute path *or* an entry of a
derive list. A derive entry matches when it equals the pattern in full or when
its **last segment** equals the pattern — so `FromRow` matches both
`#[derive(FromRow)]` and `#[derive(sqlx::FromRow)]`, while `sqlx::FromRow`
matches only `#[derive(sqlx::FromRow)]`.

**Visibility.** `visibility = "pub"` selects items declared `pub`. Items of a
trait definition inherit the trait's visibility, and items of a `impl Trait for
T` block (and `impl` blocks themselves) count as public.

## Naming

```toml
[[naming]]
name = "domain-ports"
scope = "domain"
item = "trait"                          # struct|enum|trait|fn|mod|type|const|static
pattern = "^[A-Z][A-Za-z0-9]*(Repository|Port)$"
visibility = "pub"                      # optional: "pub" (default) | "any"
message = "Domain traits are ports: name them *Repository or *Port."
severity = "warning"                    # optional, default warning
```

Every item of that kind whose identifier does not match the regex is reported
(ALD007). An invalid regex is a config error, not a silent skip.

## Return Types

```toml
[[require-return]]
name = "handlers-return-app-result"
scope = "handler"
visibility = "pub"                      # optional, default "pub"
async = true                            # optional: only async (or only sync) fns
pattern = "^Result<.*,\\s*AppError>$"
message = "Handlers must return Result<_, AppError>."
severity = "error"
```

The return type is rendered from the AST and normalized before matching: no
spaces around `<`, `>` and `::`, exactly one space after `,`. So
`Result < Vec < u8 > , AppError >` is matched as `Result<Vec<u8>, AppError>`,
and a function with no return type is matched as `()`. Free functions and
impl/trait methods are checked; nested functions are not.

## Crate Preference Enforcement

```toml
# Team decided: use tracing, not log
[[require-use]]
name = "prefer-tracing"
files = ["src/**"]
prefer = "tracing"
over = ["log"]
message = "Use tracing instead of log for structured logging."
```

## Cycles between scopes

```toml
[[acyclic]]
scopes = ["domain", "app", "infra"]   # omitted = every declared scope
severity = "error"                    # optional, default error
```

Project-wide, not per-file: after every file has been analyzed, `[[acyclic]]`
(ALD009) looks at the dependency graph between `[[scopes]]` and reports one
violation per cycle, however many scopes it passes through — the message
names every scope in the cycle plus the concrete path in order, and the text
report shows the other edge(s) of the cycle as `= note:` lines (the JSON
report carries the same information as `Violation.labels`), e.g.:

```text
ALD009 acyclic at src/domain/service.rs:1:23
  error: Cycle between scopes domain, infra: domain → infra → domain
  = note: infra → domain at src/infra/db.rs:1:29
```

`scopes` restricts the check to a subset of the declared scopes (a cycle that
only passes through an excluded scope is not reported); omit it to check
every declared scope. Overlapping `[[acyclic]]` blocks each report the same
cycle once — one violation per block that includes it, not a single merged
one.

An `// arch-lint: allow(acyclic) reason="..."` comment on the line, or an
`#[arch_lint::allow(acyclic, reason = "...")]` attribute on the `use` item
itself or on an enclosing `mod`/`fn`/`impl`/`trait`, removes that one site
from the graph before cycles are computed — same as `[[deny-scope-dep]]`
honors `allow` directives. Whether a `reason` is required depends on where
the directive is: an attribute never needs one, on an item or an enclosing
scope, same as everywhere else in arch-lint; a comment does, on an
error-severity rule. A comment directive without a reason that suppresses a
site on an edge lying on a cycle (computed before suppression) reports the
usual missing-reason warning at that site instead of silently dropping the
violation, even if another, reasoned `allow` anywhere else on the cycle also
breaks it.

## Independent scopes

```toml
[[independent]]
scopes = ["billing", "shipping", "catalog"]
severity = "error"   # optional, default error
```

Project-wide, like `[[acyclic]]`: after every file has been analyzed,
`[[independent]]` (ALD010) looks at the dependency graph between
`[[scopes]]` and reports one violation per offending edge — any reference
from one of `scopes` to another, in either direction — rather than only a
cycle. This is the "these N scopes must not depend on each other at all"
rule teams otherwise write as `N × (N - 1)` `[[deny-scope-dep]]` entries
that drift as soon as a scope is added; `[[independent]]` needs no other
config change when a scope joins the set. `scopes` is required and needs at
least two entries — unlike `[[acyclic]]`, there is no "every declared scope"
default, since independence is only meaningful for a named set.

```text
ALD010 independent at src/billing/service.rs:1:29
  error: Independent scopes billing and shipping must not depend on each other: billing → shipping
```

Each direction is reported separately: `billing → shipping` and
`shipping → billing` are two violations, one per edge, each at the first
non-suppressed site of that edge, with the edge's other sites attached as
`= note:` lines (same as `[[acyclic]]`'s other-edge labels, but here they
are other occurrences of the *same* edge).

An `// arch-lint: allow(independent) reason="..."` comment on the line, or
an `#[arch_lint::allow(independent, reason = "...")]` attribute on the `use`
item itself or on an enclosing `mod`/`fn`/`impl`/`trait`, removes that one
site before edges are computed — same mechanism, same reason requirement,
as `[[acyclic]]` (unlike `[[acyclic]]`, the missing-reason warning does not
depend on the site lying on a cycle: every in-scope edge is an offence).

## Allow-list dependencies

```toml
[[allow-dep]]
from = "domain"
to = ["shared"]          # the complete list of declared scopes `domain` may depend on
severity = "error"       # optional, default error
```

Project-wide, like `[[acyclic]]` and `[[independent]]`: after every file has
been analyzed, `[[allow-dep]]` (ALD011) looks at the dependency graph
between `[[scopes]]` and reports one violation per edge `from → X`
where `X` is a declared scope not in `to` — the inverse of
`[[deny-scope-dep]]`'s deny-list, for teams that would rather enumerate what
`from` *may* depend on than what it must not. A dependency on a path that
resolves to no declared scope (an external crate, an unscoped file) is
never a violation, the same as `[[deny-scope-dep]]`. `to = []` forbids
every declared-scope dependency from `from`. Several `[[allow-dep]]` blocks
may share the same `from`; each is checked independently against its own
`to`, so one block's allow-list never widens another's. Give each a `name`
when they do — otherwise they share the display name `allow-dep:<from>`,
and an `allow(allow-dep:<from>)` comment suppresses all of them at once.

```text
ALD011 allow-dep:domain at src/domain/b.rs:1:23
  error: Scope `domain` may only depend on [shared]: domain → infra
```

Each offending edge is reported at its first non-suppressed site, with the
edge's other non-suppressed sites attached as `= note:` lines, the same as
`[[independent]]`. An `// arch-lint: allow(allow-dep) reason="..."` comment
on the line, or an `#[arch_lint::allow(allow-dep, reason = "...")]`
attribute on the `use` item itself or on an enclosing
`mod`/`fn`/`impl`/`trait`, removes that one site before edges are computed
— same mechanism, same reason requirement, as `[[independent]]`: every
in-scope edge is an offence, so the missing-reason warning does not depend
on the site lying on a cycle.

## How scope paths resolve

The rules that follow references between scopes — `[[deny-scope-dep]]`,
`[[layering]]` and the graph rules `[[acyclic]]`, `[[independent]]` and
`[[allow-dep]]` — map a `crate::a::b`, `self::a` or `super::a` path to the
file it names (`src/a/b.rs` or `src/a/b/mod.rs`) and look up the scope that
file belongs to.

- **Scope globs** (`paths`, `exclude`) are relative to the analyzer root,
  whatever the layout.
- **`crate::`, `self::` and `super::`** resolve against the `src/` of the
  package the *referencing* file belongs to: the nearest ancestor directory,
  within the analyzer root, whose `Cargo.toml` has a `[package]` table. A
  virtual manifest (`[workspace]` only) is not a package. A file under no
  package resolves against the analyzer root.

So one config at a workspace root covers its member crates, with scopes
written as paths from that root:

```toml
# arch-lint.toml at the workspace root; members live under crates/
[[scopes]]
name = "domain"
paths = ["crates/app-core/src/domain/**"]

[[scopes]]
name = "infra"
paths = ["crates/app-core/src/infra/**"]

[[deny-scope-dep]]
from = "domain"
to = ["infra"]
message = "Domain must not depend on infra."
```

`crate::infra::db::connect()` in `crates/app-core/src/domain/mod.rs` resolves
to `crates/app-core/src/infra/db.rs` (or `.../db/mod.rs`) and is reported.

Limitations:

- `[lib] path` and `[[bin]] path` overrides are ignored: the source root is
  always `<package>/src`.
- `crate::` in a file under `tests/`, `examples/`, `benches/` or
  `src/bin/*.rs` — each its own crate — still maps to `<package>/src`;
  `self::` and `super::` are not resolved there.
- Cross-crate references (`use other_crate::...`) and bare paths are not
  followed; neither are `pub use` re-export chains.
- When the analyzer root is inside a package (say `crates/app-core/src`), no
  manifest is seen and paths resolve against the analyzer root.
- A package whose `Cargo.toml` is excluded from the analysis (but whose
  sources are not) is not seen: its files are attributed to the enclosing
  package. A manifest that cannot be parsed is skipped the same way, with a
  warning.

## Real-World Example: Review Feedback to Rule

**Before** — a reviewer has to say this every time:

> "Don't call sqlx directly from the domain layer. Use the repository trait."

**After** — add to `arch-lint.toml`:

```toml
[[scopes]]
name = "domain"
paths = ["src/domain/**"]

[[restrict-use]]
name = "no-sqlx-in-domain"
scope = "domain"
deny = ["sqlx::*"]
message = "Use repository traits instead of direct DB access in domain."
```

Now `cargo test` catches it automatically. The reviewer never has to say it again. The AI never generates it unchecked again.

