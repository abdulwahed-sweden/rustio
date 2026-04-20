# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

RustIO is a **strict-by-construction system builder** with an AI layer that understands its own rules. Not a Django clone, not a generic web framework. The positioning is in `README.md` and the full plan is in `ROADMAP.md`; read both before proposing non-trivial changes.

Phase 1 (Foundation, 0.4.x) and Phase 2 (Intelligence, 0.5.x–0.8.x) have shipped; Phase 3 (Systems — pre-built verticals like `clinic`, `crm`, `inventory`) is v1.0+. The `README.md` "What's shipped" table is authoritative for which capabilities exist in which version — prefer it over restating version numbers here, since they drift.

`rustio.schema.json` is the **only** interface external tooling — including the AI layer — is allowed to use. Treat its shape as stable across patch releases; changes require a version bump and a CHANGELOG note. Two sibling inputs feed the AI pipeline alongside the schema:

- `rustio.design.json` — visual-only admin identity (project name, logo initial, primary/accent colour, density). Loaded once per process via `admin::design::Design::global`. Bad values fall back to defaults at render time; it cannot change page structure, routing, or form semantics.
- `rustio.context.json` — country / industry / compliance. Drives PII detection and policy refusals in the planner + review layers.

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

## Admin is framework-owned

The admin HTML shell, layout, forms, tables, auth pages, and error states have **no template override hook**. A generated project's `templates/` and `static/` directories are for public site pages only. Visual customisation of the admin flows exclusively through `rustio.design.json` → `admin::design::Design`. When a request lands to "theme the admin" or "override the admin template", redirect it through `Design` fields or reject — don't add an escape hatch.

The admin submodules under `rustio-core/src/admin/` carry most of the Phase 2 admin behaviour:

- `schema_cache.rs` — process-local `RwLock<Option<Schema>>` reread at runtime. `/admin/schema/reload` and a successful `ai apply` both refresh it, so the dashboard + suggestion engine reflect schema changes without a restart. A poisoned lock degrades to "cache empty", never panics.
- `intelligence.rs` + `suggestions.rs` — role classification, filters, search intent, masking, and suggestion confidence. These pattern-match on `FieldType` and on context, so they're a required update site when the type vocabulary changes.
- `entry_builder.rs` — constructs `AdminEntry` lists dynamically from the cached schema.
- `audit.rs`, `design.rs` — audit logging and visual-identity config.

## The macro ↔ core contract

`#[derive(RustioAdmin)]` emits code that references `::rustio_core::admin::AdminField`, `::rustio_core::admin::parse_datetime_local`, `::rustio_core::Error`, etc. Both crates must stay in lockstep:

- A new `FieldType` variant means: update `admin::FieldType` (non_exhaustive), `schema::field_type_name` (exhaustive match by design), `orm::Value` + `bind_value`, `admin::suggestions` + `admin::intelligence` (pattern-match on types for role/confidence), macro's `FieldKind` + `classify_type` + `from_form_assignment` + `display_arm`.
- Forgetting any step produces either a schema lie or a compile error at the user's site. The `schema::field_type_name` match is deliberately exhaustive to catch this.

## Versioning + backward compatibility

Pre-1.0 — breaking changes are allowed in minor releases and documented in `CHANGELOG.md`. But: do not casually break scaffolded projects. If the `main.rs` / `apps/mod.rs` template shape changes, older projects need a migration note (see the 0.4.0 note in `CHANGELOG.md` for the pattern). Marker comments in `apps/mod.rs` are part of the stable surface between CLI releases.

## AI layer boundary (important)

`rustio_core::ai` is a three-stage pipeline — **plan → review → apply** — with a fixed `Primitive` vocabulary as the boundary between every stage:

- `ai/planner.rs` (`rustio ai plan`) — rule-based grammar parses the user's request into a typed `Plan` of `Primitive` ops. No free-form code generation. If the grammar can't match, the planner **refuses** rather than guesses.
- `ai/review.rs` (`rustio ai review`) — purely deterministic risk / impact / warnings from a saved plan document. No LLM, no heuristic softening.
- `ai/executor.rs` (`rustio ai apply`) — atomic writes to the project tree (`models.rs` + migration files). Destructive ops (e.g. `drop_model`, `drop_field`) refuse without an explicit flag. `ParsedModelsFile` round-trips the file so edits are surgical.
- `ai/industry.rs` — industry-specific schema hints consumed by the planner alongside `rustio.context.json`.

The hard rule is unchanged from Phase 1: if a change cannot be expressed as a `Primitive`, it is **rejected**. Every serde type in `ai.rs` uses `deny_unknown_fields`; the validator (`validate_primitive`, `Plan::validate`) simulates the plan end-to-end before any file write. A project whose shape can't be described in this vocabulary is a project the AI layer will refuse to touch.

When extending the primitive set, keep `Primitive` `#[non_exhaustive]`, add the variant to `validate_primitive`, teach `executor.rs` how to apply it, teach `review.rs` how to classify its risk/impact, and update the CHANGELOG **before** wiring it up in the planner grammar.

## Performance constraints

These are honest limits, not aspirational — any release that regresses them must not ship:

- ≥50,000 req/s on a simple endpoint
- 10–30 MB resident memory
- <50 ms cold start
- ~15 MB stripped binary

`ROADMAP.md` "Technical constraints" is the authoritative reference.

## Contributing flow

`CONTRIBUTING.md` applies. For Phase 1 (Foundation) work, align on design first — open an issue before writing code. Required checks before opening a PR match the three commands at the top of this file. CI runs the same checks on every push.
