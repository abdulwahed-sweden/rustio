# Upgrading RustIO

A per-release migration guide. Items here only cover externally-observable changes; minor refactors without a user-visible surface are not listed.

---

## Unreleased — the admin design is ported from rustio-lite

The admin is restyled from [rustio-lite](https://github.com/abdulwahed-sweden/rustio-lite).
Three things are externally visible:

- **`/admin/assets/admin.css` is gone** (the file behind it,
  `rustio-core/assets/admin.css`, is deleted). Every admin page now links
  `/admin/static/admin.css`. If your project referenced the old URL, point it
  at the new one.
- **Dark mode is removed** — no `[data-theme]`, no toggle, no
  `localStorage.rio-theme`. A stored `dark` preference is ignored; the admin
  is light.
- **The admin class names changed wholesale**, from `.rio-*` BEM to
  rustio-lite's flat vocabulary (`.card`, `.button-primary`, `.badge-active`).
  A template you override under your project's `templates/admin/…` still
  renders, but any class names inside it must be re-pointed to match
  `docs/design-system.md`. Templates you have not overridden need no action.

---

## Unreleased — `init` creates an empty project

The five domain templates (clinic, blog, shop, crm, tasks), the setup menu that
offered them, and `rustio start` are removed: `rustio init <name>` now creates
an empty project and prints the next commands — run `rustio add model <name>`
to add models. `rustio init --preset basic|blog|api` is removed with them —
run `rustio add model <name>` instead (`--model <name>` still scaffolds one
model during `init`). Code calling `rustio_core::ai::{sketch, FieldSketch,
ModelSketch, ProjectSketch}` (the `ai::intake` module) no longer compiles;
those types only described the templates and have no replacement.

---

## Unreleased — `evolve` is now `change`

**No action required.** The old spelling still works for one release.

| Before | Now |
|---|---|
| `rustio evolve "add author as String to Book"` | `rustio change "add author as String to Book"` |
| `rustio evolve "rename title to name in Member"` | `rustio change "rename title to name in Member"` |
| `rustio evolve "change priority to Integer in Book"` | `rustio change "change priority to Integer in Book"` |
| `rustio evolve "add relation from Loan to Book"` | `rustio change "add relation from Loan to Book"` |
| `rustio evolve --why` | `rustio change --why` |

Running the old spelling prints one line and then does exactly what `change`
does:

```text
note: `evolve` is now `change` — same command.
```

Scripts keep working unchanged; exit codes and both confirmation prompts are
the same. Nothing else about the command moved — the five accepted shapes, the
planner, the typo-correction prompt, the refusal screen and the migration it
writes are all as they were. `rustio ai plan` / `ai review` / `ai apply` are
untouched.

---

## Unreleased — `new app` is now `add model`

**No action required.** The old spelling still works, and no file moves.

### The command

```bash
rustio add model book      # was: rustio new app book
```

`rustio new app <name>` (and `rustio new model <name>`) still scaffold a
model; they print one line saying the command has been renamed, then do it.
The alias is kept for one release. `rustio init --app <name>` is likewise
accepted as the retired spelling of `--model <name>`.

### The directory

Projects scaffolded from this release on keep their models in `models/<name>/`
and their `main.rs` declares `mod models;`.

**Existing projects keep `apps/`.** Nothing is moved and nothing needs to be:
the CLI and the AI executor both resolve the layout from disk, so `add model`,
`change`, `doctor` and the bare `rustio` status line all read whichever
directory your project has. Mixed fleets are fine.

If you *want* to move an existing project onto the new layout, it is three
steps and entirely optional:

```bash
git mv apps models
sed -i '' 's/^mod apps;/mod models;/; s/\bapps::/models::/g' main.rs
cargo build
```

Anything in your own code that says `crate::apps::…` needs the same rename.
There is no deadline; the `apps/` layout stays supported.

### `rustio explain app` → `rustio explain layout`

The explainer was about where a model's files live, so it is now named for
that. `rustio explain model` is unchanged.

---

## Unreleased — CLI first-run journey, `--port`, real Users page

Three externally-visible changes. None require action; the second and third
are worth knowing about.

### `rustio run --port <n>` is refused on a pre-0.11 `main.rs`

The flag reaches the project binary through `RUSTIO_PORT`, which is read by
the bind block a freshly scaffolded `main.rs` carries:

```rust
let port: u16 = std::env::var("RUSTIO_PORT")
    .ok()
    .and_then(|p| p.parse().ok())
    .unwrap_or(8000);
let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
if std::env::var_os("RUSTIO_QUIET").is_none() {
    eprintln!("serving on http://{addr}");
}
Server::bind(addr).serve_router(router).await?;
```

A project scaffolded before that block existed ignores the variable and binds
8000 whatever is passed. Rather than print a banner for a port nothing is
listening on, `rustio run --port 8001` on such a project **exits 1 and starts
nothing**:

```text
error: --port needs the RUSTIO_PORT block in main.rs — see UPGRADING.md
```

Paste the block above into your `main.rs` to adopt the flag. Plain
`rustio run` is unaffected: it serves 8000 as before, and its banner is
correct. (The same `RUSTIO_QUIET` guard applies to the `--dump-schema`
branch's "wrote rustio.schema.json" line.)

### The `/admin/users` page now shows your real users

It was backed by a demo table (`admin_new_demo_users`) with invented `Doctor`
and `Salary` columns. It is now backed by `rustio_users` and shows Email, Role,
Active — the same rows `rustio user create` and the login form use. The demo
table is no longer created or read; if one exists in your database it is simply
orphaned, and you can drop it at your convenience:

```sql
DROP TABLE IF EXISTS admin_new_demo_users;
```

Creating a user from the admin is now refused (hidden "+ Add", 403 on the
create routes): a user row without a hashed password can't sign in, so users
are created with `rustio user create` and edited from the admin.

### `rustio change` applies its own migration

`change` now asks "Apply the migration now?" after writing one. Answering no
prints `rustio migrate apply` and changes nothing else — scripts that ran
`change` followed by `migrate apply` keep working (the second call finds
nothing pending).

---

## Unreleased — i18n L4a (per-user language preference)

Adds a per-user UI language preference. **Action required for existing projects.**

### Required: back-port the new column

i18n L4a adds a `preferred_language TEXT NOT NULL DEFAULT ''` column to the
`rustio_users` table. The column is created by `auth::ensure_core_tables`, which
the migration driver runs — **the server does not migrate on boot**. So an
existing project on a database created before L4a must run:

```bash
rustio migrate apply
```

This is an `O(1)` `ALTER TABLE … ADD COLUMN` (the same back-port mechanism used
for `rustio_sessions.csrf_token`); existing rows default to `''` (no
preference). Fresh databases get the column from `CREATE TABLE` automatically
and need no action.

**Symptom if skipped:** setting a language (`POST /admin/language`) returns a
500 with `no such column: preferred_language`. Rendering is unaffected until
then — with no preference, the active language is the view's `default_language`,
exactly as before.

### No data or ViewSpec changes

Setting a language is per-user state only; it never modifies a `<model>.view.json`
or its `default_language`, and never translates stored data (the iron rule).

---

## 0.9.0 → 0.9.1

Destructive-op gate lights up. No action required for projects that don't plan `remove_field` / `remove_relation`.

### New behaviour (opt-in)

- **`rustio ai apply <plan> --force`** now unlocks the two destructive primitives the planner has always been allowed to emit:
  - `remove_field` — drops a column from both the Rust struct and the database.
  - `remove_relation` — drops a belongs_to FK column.

  Without `--force`, both continue to refuse with `primitive `remove_field` is destructive — re-run `rustio ai apply` with `--force` to open the destructive gate`.

- **`remove_model`** stays refused regardless of `--force` until 0.9.2. Scope cap — dropping a whole model has to coordinate the struct, the admin registration, and every downstream FK.

### What `--force` does NOT bypass

Three gates live one layer above the destructive-op gate and are **not** reachable via `--force`:

- **Critical-risk plans** — review-layer decisions like "this plan touches core models" or "this plan mixes add + remove on the same field". Regenerate the plan under a changed posture.
- **Developer-only primitives** (e.g. `create_migration`) — these are never allowed through `rustio ai apply`, by design.
- **PII policy refusals** — removing a field flagged under `rustio.context.json` as personal data escalates to Critical before the destructive gate even runs.

### SQL generation

The `remove_field` / `remove_relation` migration is FK-aware: every surviving relation on the same table keeps its `REFERENCES <parent>(id) ON DELETE <policy>` clause during the recreate-table. Operations on tables that have no FKs also work (the guard path was only in `change_field_type`; `remove_field` uses its own recreate).

### Compatibility

- **API.** `ExecuteOptions.allow_destructive` existed as a field in 0.8.x but was silently ignored. In 0.9.1 it becomes load-bearing. Code that set `allow_destructive: true` on 0.8.x saw no effect; on 0.9.1 it does. Review callers that construct `ExecuteOptions` directly before upgrading.
- **CLI.** `rustio ai apply` gains `--force`. Existing `--yes` / `--dry-run` composition is unchanged.
- **Tests.** Crate-internal tests that asserted "remove_field is refused even with allow_destructive" have been flipped; if you forked that test, flip it the same way.

---

## 0.8.x → 0.9.0

Phase 2 close-out. Ships SQL foreign-key enforcement and a retrofit path for existing projects.

### New behaviour (automatic)

- **New `belongs_to` relations emit a real SQL `FOREIGN KEY`.** Running `rustio ai apply <plan>` on a plan that contains `AddRelation { kind: BelongsTo, .. }` now generates a migration that says `REFERENCES <parent>(id) ON DELETE RESTRICT` instead of just adding a bare `<via>_id` column. The generated migration also emits `PRAGMA foreign_keys = ON;` so the constraint is enforced against the connection running the migration.

- **The FK column is nullable by default.** SQLite cannot add a `NOT NULL + REFERENCES` column via `ALTER TABLE` (the implicit `DEFAULT NULL` is what makes existing rows satisfy the new FK without a backfill). If you need a required FK, use the retrofit path below.

- **New natural-language grammar** for relation phrases accepts trailing options:
  - `link A to B` — nullable FK, `ON DELETE RESTRICT` (default).
  - `link A to B required` — `NOT NULL` FK. **Refuses at executor** — use the retrofit.
  - `link A to B on_delete:cascade` — deletes children when the parent is deleted.
  - `link A to B on_delete:set_null` — nulls children when the parent is deleted.
  - Combinations allowed: `link A to B required on_delete:set_null`.
  - Unknown options / policies **refuse**, never silently default.

### Retrofit for existing 0.8.x projects

If your project was generated on 0.8.x, your `_id` columns exist but have no SQL FK constraint. Run:

```bash
rustio migrate add-fks           # dry run — prints what would change
rustio migrate add-fks --write   # commits one migration per affected table
```

The command reads `rustio.schema.json`, finds every `belongs_to` relation that lacks `on_delete` metadata, and generates one migration per affected table using the SQLite recreate-table pattern (`CREATE TABLE <t>__new … FOREIGN KEY …` + `INSERT … SELECT` + `DROP` + `RENAME`).

After writing, **review each generated SQL file** before running `rustio migrate apply`. The recreate-table pattern drops and rebuilds the table — any column or index you added outside of `rustio.schema.json` will be lost unless it's reflected in the schema.

### Breaking changes

- **`schema::Relation` gains two fields**, `required: Option<bool>` and `on_delete: Option<String>`. Both are serde-default, so 0.8.x `rustio.schema.json` files parse unchanged. Rust code that constructs `Relation { … }` literally (only in-tree tests and tooling) needs those two fields added.

- **`ai::AddRelation` gains two fields**, `required: bool` and `on_delete: OnDelete`. Both are serde-default, so saved 0.8.x plan documents still deserialise. Rust code that constructs `AddRelation { … }` literally needs both added; `required: false` + `on_delete: OnDelete::Restrict` matches the 0.8.0 implicit behaviour.

### Review-layer risk

- Default (`required: false`, `ON DELETE RESTRICT`): **Low**.
- Either `required: true` OR `ON DELETE CASCADE`: **Medium** — new warning in `warnings_for`.
- Both `required: true` AND `ON DELETE CASCADE`: **High** — the cascade-with-strict-FK combination is the most destructive policy combination.

### Verified against medflow

The example clinic system has 27 `belongs_to` relations across 13 tables. `rustio migrate add-fks` identifies all 27 and writes 13 retrofit migrations. Nullability on each FK column is preserved from the existing schema (e.g. `department_id` stays nullable, `patient_id` stays `NOT NULL`).

---

## Earlier releases

See `CHANGELOG.md` for the full history.
