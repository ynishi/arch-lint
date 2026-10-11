# Suppression

arch-lint provides multiple ways to suppress violations at different scopes: line, block, file, or project, with mandatory reasoning for error-severity rules. [Back to README](../README.md).

## Scope Overview

| Scope | Method | Use Case |
|-------|--------|----------|
| Line | Comment | Single expression |
| Block | `#[arch_lint::allow(...)]` | Function, impl, module |
| File | Configuration (`exclude_files`) | Entire file |
| Global | Configuration (`enabled = false`) | Project-wide exclusion |

Both directives apply to the built-in rules (AL001-AL013) **and** to the
declarative rules from `arch-lint.toml` (ALD001-ALD011); see
[Declarative rules](#declarative-rules-ald001-ald011) below for the names a
declarative rule answers to.

## Line-level (Comment)

Use inline comments for single expressions, on the violating line or the
line above it:

```rust
// arch-lint: allow(no-sync-io) reason="Startup initialization only"
let config = std::fs::read_to_string("config.toml")?;

// arch-lint: allow(no-unwrap-expect) reason="Guaranteed by loop invariant"
let value = some_option.unwrap();

let value = some_option.unwrap(); // arch-lint: allow(no-unwrap-expect) reason="Guaranteed by loop invariant"
```

The two placements are not interchangeable for *which* line they cover: a
comment on the line **above** the violation applies to the line below it,
while a trailing comment applies only to **its own** line — it never also
reaches the line after it, even if that next line has the same kind of
violation.

## Block-level (Attribute)

Use attributes for functions, impl blocks, or modules:

```rust
#[arch_lint::allow(no_unwrap_expect, reason = "All inputs validated at entry point")]
fn parse_validated_config(input: &str) -> Config {
    let value = input.parse().unwrap();
    let count = input.len().try_into().unwrap();
    Config { value, count }
}

#[arch_lint::allow(no_sync_io, reason = "CLI startup, not in async context")]
mod startup {
    // All sync I/O allowed in this module
}
```

## Mandatory Reasoning

For `Severity::Error` rules (AL001, AL002, AL003, AL005), the `reason`
parameter is **required on the comment path**: a comment directive without a
`reason` on an error rule suppresses the violation and generates a
`Severity::Warning` violation (`Allow directive for '<rule>' is missing
required reason`) in its place:

```rust
// BAD — reason omitted on the comment path → generates a Warning violation
// arch-lint: allow(no-unwrap-expect)
let value = some_option.unwrap();

// GOOD — reason provided
// arch-lint: allow(no-unwrap-expect) reason="Config validated at startup"
let value = some_option.unwrap();
```

This ensures that critical suppressions are always documented and justified.

An `#[arch_lint::allow(...)]` **attribute** suppresses silently either way —
the missing-reason warning above is specific to the comment path, not the
attribute one:

```rust
// No warning: an attribute never requires a reason, even on an error rule.
#[arch_lint::allow(no_unwrap_expect)]
fn parse_validated_config(input: &str) -> Config {
    let value = input.parse().unwrap();
    let count = input.len().try_into().unwrap();
    Config { value, count }
}
```

## Declarative rules (ALD001-ALD011)

The declarative rules honor the same two directives, with the same semantics.
A declarative rule answers to three names, each in kebab-case or `snake_case`:

| Name | Example | Suppresses |
|------|---------|------------|
| Rule kind | `restrict-use` / `restrict_use` | every `[[restrict-use]]` rule |
| TOML `name` | `no-sqlx-in-domain` | only that one TOML rule |
| Rule code | `ALD001` | every rule reported under that code |

`allow(all)` suppresses everything, as it does for the built-in rules.

`[[acyclic]]` (ALD009), `[[independent]]` (ALD010) and `[[allow-dep]]`
(ALD011) are the three project-wide exceptions: an `allow` directive — a
comment on the line, an attribute on the `use` item, or an attribute on an
enclosing `mod`/`fn`/`impl`/`trait` — removes that one site from the scope
graph before cycles (or offending edges) are computed, rather than
suppressing a violation already found on that line (see
[Cycles between scopes](declarative-rules.md#cycles-between-scopes),
[Independent scopes](declarative-rules.md#independent-scopes) and
[Allow-list dependencies](declarative-rules.md#allow-list-dependencies)). Reporting an
`allow(acyclic)`, `allow(independent)` or `allow(allow-dep)` directive that
no longer has anything to suppress as unused is not implemented.

```rust
// Comment on the violating line or the line above it
// arch-lint: allow(no-sqlx-in-domain) reason="legacy row mapping"
use sqlx::Pool;

// Attribute on the offending item
#[arch_lint::allow(no_orm_derives_in_domain, reason = "legacy row mapping")]
#[derive(sqlx::FromRow)]
pub struct User { pub id: i64 }

// Attribute on an enclosing mod / fn / impl, covering everything inside it
#[arch_lint::allow(layering, reason = "composition root wires the layers")]
pub fn wire() {
    let _ = crate::handler::api::list_users;
}
```

Unlike some built-in rules, declarative rules are **not** switched off inside
`#[cfg(test)]` modules or `#[test]` functions: they express architecture
constraints, so test code is held to them too. Take a directory out of a scope
with `exclude` on `[[scopes]]` if that is not what you want.

## Clippy Compatibility

Standard Clippy attributes are also recognized:

```rust
#[allow(clippy::unwrap_used)]
fn allowed_unwrap() {
    value.unwrap()  // OK - recognized by arch-lint
}
```

## Configuration File

Disable rules globally or per-file in `arch-lint.toml`:

```toml
[rules.no-unwrap-expect]
enabled = false  # Disable entirely

[rules.no-sync-io]
exclude_files = ["src/startup.rs", "src/cli/**"]
```

