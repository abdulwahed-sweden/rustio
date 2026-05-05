# Freelance — RustIO Phase 14 showcase

End-to-end demo of the Phase 14 schema contract pipeline:

```text
Model → Schema → Validator → Doctor → Admin → Search
```

Three realistic models — `Client`, `Project`, `Invoice` —
defined entirely with `#[derive(RustioModel)]`. **No manual
`AdminModel` impl, no manual `Searchable` impl, no `AdminEntry`
builder calls.** Every field flag flows through to admin and
search via the bridges introduced in Phase 14 commits 5 and 6.

---

## Quick start

Prerequisites: a running Postgres on port 5432.

```sh
createdb rustio_freelance
export DATABASE_URL=postgres://postgres:dev@localhost/rustio_freelance

# Apply the three migrations.
psql "$DATABASE_URL" -f migrations/0001_create_clients.sql
psql "$DATABASE_URL" -f migrations/0002_create_projects.sql
psql "$DATABASE_URL" -f migrations/0003_create_invoices.sql

# Run the demo binary — connects, validates each schema,
# derives search + admin configs, and logs the pipeline.
cargo run
```

Output (one block per model):

```text
=== projects ===
validator: status=Ok errors=0 warnings=0
search:    enabled (index=projects)
           searchable = ["name", "description"]
           filterable = ["client_id"]
           sortable   = ["name", "client_id", "budget_cents", "created_at"]
admin:     6 fields auto-generated
           - id             label="id"             editable=false flags=pk,readonly
           - name           label="name"           editable=true  flags=searchable,sortable
           - description    label="description"    editable=true  flags=searchable,textarea
           - client_id      label="client_id"      editable=true  flags=filterable,sortable
           - budget_cents   label="Budget (cents)" editable=true  flags=sortable
           - created_at     label="created_at"     editable=false flags=sortable,readonly
```

---

## Schema check (the doctor)

The doctor's subprocess hook is wired in `main.rs` via
`contract_doctor::maybe_handle_subprocess`. From the workspace
root, the CLI invocation is:

```sh
rustio doctor --check-schema           # human-readable
rustio doctor --check-schema --json    # CI-friendly JSON
```

Exit codes:

- `0` — every schema validates clean (or only warnings)
- `1` — one or more schemas have errors

---

## How the `#[rustio(...)]` flags drive behaviour

Every flag flows from the model declaration through the bridges
to the appropriate framework subsystem. Nothing is wired by hand.

| Field attribute     | Admin bridge effect                  | Search bridge effect             |
|---------------------|--------------------------------------|----------------------------------|
| `searchable`        | (passed through on `BridgedField`)   | added to `searchable_attributes` |
| `filterable`        | (passed through on `BridgedField`)   | added to `filterable_attributes` |
| `sortable`          | (passed through on `BridgedField`)   | added to `sortable_attributes`   |
| `readonly`          | `editable = false`                   | n/a                              |
| `label = "..."`     | `AdminField.label` (overrides name)  | n/a                              |
| `widget = "..."`    | preserved on `BridgedField.widget`   | n/a                              |
| `references = "..."`| (FK metadata, captured)              | n/a                              |
| `sql = "..."`       | drives the validator's PG comparison | drives the validator's PG comparison |

Order matters: the bridges preserve the field declaration order
exactly. Meili weights the first searchable attribute highest, so
the order in the source struct is the order users see.

---

## Validator gating (fail-safe)

`search::from_schema::enable_search::<T>` only enables search
when the validator returns `Ok` or `Warning`:

| `validate_schema::<T>` status | `enable_search::<T>` outcome |
|-------------------------------|------------------------------|
| `Ok`                          | `Enabled` — config returned  |
| `Warning`                     | `Enabled` — config returned (warnings logged) |
| `Error`                       | `Disabled` — refuses, returns the report |

A schema that drifts from the live database **never** gets a
config back. Indexing against a drifted schema would silently
produce malformed Meili documents; the gate refuses loudly
instead.

---

## Why this example is a separate workspace

The Phase 14 commit 7 spec strictly forbids modifying the parent
`Cargo.toml`'s `members` list (the constraint reads "ONLY add
files inside `examples/freelance/`"). Cargo would otherwise warn
about a subdirectory with its own `Cargo.toml` that's neither in
`members` nor `exclude`. The `[workspace]` block in this
directory's `Cargo.toml` makes Cargo treat `examples/freelance/`
as the root of a fresh workspace, disconnecting it cleanly
without touching parent files.

This means:

- `cargo check --workspace` from the repo root **does not**
  build this example. To verify it compiles, run `cargo check`
  from inside `examples/freelance/`.
- The example's own tests (`cargo test` inside this directory)
  are isolated from the parent workspace's test surface.

---

## What this example does (after commit 8)

The Phase 14 runtime integration unlocks a true zero-config
flow:

- **Three models** with `#[derive(RustioModel)]` and `T::SCHEMA`
  — no `AdminModel` impl, no `Searchable` impl, no `Model` impl.
- **`search_index` is auto-derived** by the macro from the
  presence of any `#[rustio(... searchable ...)]` flag — no
  per-model `with_search_index(...)` override required.
- **`Admin::new().from_schemas(&[ModelSchema])`** registers
  every schema as a fully-functional `AdminEntry`, backed by a
  generic `SchemaOps` that drives CRUD via raw SQL using only
  the schema's column metadata.
- **`search::from_schema::indexer_from_schema::<T>(...)`**
  validates each schema against the live DB and spawns an
  indexer only when the validator's verdict is `Ok` /
  `Warning` — drift in PG silently disables indexing rather
  than producing malformed Meili documents.
- **Doctor subprocess hook** stays the same:
  `rustio doctor --check-schema [--json]` invokes the binary
  with the magic flag and intercepts before any server work.

## What it does *not* yet do

- Mount the populated `Admin` builder onto an HTTP router.
  That's a one-liner via `register_admin_routes` (see the
  `examples/blog/` example) — it's omitted here to keep the
  runtime path minimal and avoid pulling in templates +
  migrations bootstrap.
- Auto-detect models for indexer registration. Each model
  is registered explicitly via `indexer_from_schema::<T>`;
  a future "register every `T: HasSchema` from a registry"
  helper will collapse this further.

---

## Tests

```sh
cd examples/freelance
cargo test
```

The test suite covers:

- Schemas compile and carry the expected `(table, columns, pk)`.
- Search bridge produces the right attribute lists per model.
- Validator gate disables search on errors and enables on `Ok`.
- Admin bridge preserves widget overrides and label fallbacks.
- Doctor subprocess flag literals match between core and CLI.
