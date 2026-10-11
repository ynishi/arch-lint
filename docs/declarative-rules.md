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

