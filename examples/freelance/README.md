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

## What this example does NOT do

To stay within the commit 7 constraint envelope (no
`rustio-core` modifications, no `rustio-macros` modifications,
no `rustio-cli` modifications), this example:

- Does **not** start a full admin HTTP server. The framework's
  `Admin::new().model::<T>()` requires `T: AdminModel`, which is
  manual config that the spec forbids. A future commit will add
  `Admin::new().from_schema::<T>()` to wire `HasSchema` directly,
  at which point this example's `main.rs` gets four extra lines
  and a real admin UI.
- Does **not** run a Meilisearch index. Same shape — the bridge
  produces a `SearchConfig`, but plugging it into the existing
  `Indexer` requires the still-pending macro extension that
  emits `search_index` automatically. Until then, a one-line
  `with_search_index("clients")` override (in `lib.rs::all_schemas`)
  bridges the gap.

What it *does* do, end-to-end:

- Three models with `#[derive(RustioModel)]` and `T::SCHEMA`.
- Doctor subprocess hook (works today via
  `rustio doctor --check-schema`).
- Validator reports per model (works today against any live PG).
- Search bridge `SearchConfig` (works today, demonstrably).
- Admin bridge `BridgedField` list (works today, demonstrably).

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
