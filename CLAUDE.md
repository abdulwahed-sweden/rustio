# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

RustIO is a **strict-by-construction system builder** with an AI layer that understands its own rules. Not a Django clone, not a generic web framework. The positioning is in `README.md` and the full plan is in `ROADMAP.md`; read both before proposing non-trivial changes.

The three-phase progression is strictly sequential:

1. **Foundation** (v0.4.0) — typed core, auth, `rustio.schema.json`.
2. **Intelligence** (v0.5.0) — AI layer that reads the schema and emits a fixed set of primitives.
3. **Systems** (v0.6.0+) — pre-built vertical templates (`clinic`, `crm`, `inventory`, …).

`rustio.schema.json` is the **only** interface external tooling — including the Phase 2 AI layer — is allowed to use. Treat its shape as stable across patch releases; changes require a version bump and a CHANGELOG note.

## Design filter

Every feature must answer: *Does it make building a real system faster, clearer, or safer?* If no, it doesn't belong in RustIO. See `ROADMAP.md` "What RustIO is NOT" for the explicit out-of-scope list — Django API compatibility, template engines, frontend frameworks, sync runtime, microservice tooling, MySQL/Oracle/SQL Server.

## Development commands

```bash
cargo fmt --all --check                              # formatting (CI gate)
cargo clippy --workspace --all-targets -- -D warnings # lint (CI gate)
cargo test --workspace --all-targets                 # full test suite

cargo test -p rustio-core <name_substring>           # one crate, one test
cargo test -p rustio-core --lib schema::tests        # a module
```

Smoke-testing scaffolded projects against the local crate tree:

```bash
RUSTIO_CORE_PATH=$(pwd)/rustio-core cargo run --quiet -p rustio-cli -- init scratch --preset blog
```

`RUSTIO_CORE_PATH` makes generated `Cargo.toml` point at the workspace copy of `rustio-core`; without it, it pins to a crates.io version that may not yet be published.

## Workspace shape

Three crates with a strict dependency chain. Publish order is always `rustio-macros` → `rustio-core` → `rustio-cli`.

- **`rustio-macros`** — proc macros (`#[derive(RustioAdmin)]`). Must stay lean; introspects `syn::Type` and emits code referencing `::rustio_core::...`. Knows the field-type vocabulary.
- **`rustio-core`** — the runtime library. Hyper-backed server, router, middleware, ORM over SQLite (via sqlx, hidden from user code), admin, migrations, schema exporter, AI primitive definitions.
- **`rustio-cli`** — the `rustio` binary. Scaffolding, migrations driver, `rustio run`, `rustio schema`, `rustio ai` stub.

When publishing from a machine with `~/.cargo/config.toml` pinned to `protocol = "git"`, export `CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse` before each `cargo publish` — otherwise downstream crates can't see the just-published upstream.

## How user projects are generated

`rustio init` / `rustio new app` writes files from `const`-string templates in `rustio-cli/src/main.rs`. New apps are **mechanically edited into** `apps/mod.rs` via marker comments:

```
// -- modules --
// -- end modules --
    // -- end admin installs --
    // -- end view registrations --
```

`register_app_in_mod` searches for these markers and inserts before them. If you change the shape of the generated `apps/mod.rs`, the markers **must stay in the same form** or every existing project's `rustio new app` breaks. The template split into `build_admin()` + `register_all()` exists specifically so `main.rs --dump-schema` can introspect the admin without touching the DB or binding a port.

## The macro ↔ core contract

`#[derive(RustioAdmin)]` emits code that references `::rustio_core::admin::AdminField`, `::rustio_core::admin::parse_datetime_local`, `::rustio_core::Error`, etc. Both crates must stay in lockstep:

- A new `FieldType` variant means: update `admin::FieldType` (non_exhaustive), `schema::field_type_name` (exhaustive match by design), `orm::Value` + `bind_value`, macro's `FieldKind` + `classify_type` + `from_form_assignment` + `display_arm`.
- Forgetting any step produces either a schema lie or a compile error at the user's site. The `schema::field_type_name` match is deliberately exhaustive to catch this.

## Versioning + backward compatibility

Pre-1.0 — breaking changes are allowed in minor releases and documented in `CHANGELOG.md`. But: do not casually break scaffolded projects. If the `main.rs` / `apps/mod.rs` template shape changes, older projects need a migration note (see the 0.4.0 note in `CHANGELOG.md` for the pattern). Marker comments in `apps/mod.rs` are part of the stable surface between CLI releases.

## AI layer boundary (important)

The `rustio_core::ai` module ships in 0.4.0 as **definitions only** — the `Primitive` enum and its variants are the complete vocabulary the 0.5.0 executor will be allowed to emit. The hard rule for Phase 2 is: if a change cannot be expressed as one of these primitives, it is rejected. No free-form code generation, no "close enough" fallback, no partial writes.

When extending the primitive set, keep `Primitive` `#[non_exhaustive]` and update the CHANGELOG with the new variants before writing the executor that handles them.

## Performance constraints

These are honest limits, not aspirational — any release that regresses them must not ship:

- ≥50,000 req/s on a simple endpoint
- 10–30 MB resident memory
- <50 ms cold start
- ~15 MB stripped binary

`ROADMAP.md` "Technical constraints" is the authoritative reference.

## Contributing flow

`CONTRIBUTING.md` applies. For Phase 1 (Foundation) work, align on design first — open an issue before writing code. Required checks before opening a PR match the three commands at the top of this file. CI runs the same checks on every push.
