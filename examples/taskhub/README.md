# taskhub

A small example project that shows how a real [RustIO](https://github.com/abdulwahed-sweden/rustio) system fits together. Two models with a foreign-key relationship, seed data so the admin renders something on first run, and an admin account ready to sign in with.

## What this example demonstrates

- **Two models with a relation.** A `Project` groups many `Task`s. The `Task.project_id` field carries `#[rustio(belongs_to = "Project", display = "name")]`, so the admin list page renders each task's `project_id` as a clickable project **name** instead of a raw integer.
- **The 0.10 admin.** Every page (dashboard, list, edit form, login, 403, 404, profile, audit log, suggestion review) is rendered through `minijinja` templates + Bootstrap 5. Drop a file at `templates/admin/list.html` to override the framework default without rebuilding.
- **RBAC out of the box.** The framework ships a `Role` system (`SuperAdmin` / `Admin` / `Editor` / `Viewer`) with per-model `view` / `create` / `edit` / `delete` permissions. Sign in as the demo admin and visit `/admin` — every model + every action is available. Demote the user to `Viewer` and the per-row `Edit` / `Delete` buttons disappear; opening the URL directly returns the framework 403 page.
- **The full field-type vocabulary.** `String`, `bool`, `i32` (priority), `i64` (FK), `DateTime<Utc>` (`created_at`), and `Option<DateTime<Utc>>` (`due_at` — nullable so backlog items can have no fixed deadline).
- **Deterministic schema export.** `rustio.schema.json` is the only interface external tooling — including the AI layer — is allowed to use. Regenerated on every `rustio migrate apply`.

## Run it (5 steps, ≈30 seconds)

```bash
# 1. From the repo root, point cargo at the in-tree rustio-core
#    (0.10.0 hasn't been published to crates.io from this clone yet).
cd examples/taskhub

# 2. Apply migrations: creates rustio_users + rustio_sessions + roles
#    + projects + tasks + the seed data (2 projects, 5 tasks).
RUSTIO_CORE_PATH="$(pwd)/../../rustio-core" \
  cargo run --manifest-path ../../Cargo.toml -p rustio-cli -- migrate apply

# 3. Start the server (one-time compile, then incremental).
cargo run

# 4. Open the admin: http://127.0.0.1:8000/admin
#    Sign in with:
#      Email:    admin@taskhub.local
#      Password: demo1234
#    (Created by the `rustio user create` step below — re-run if you
#    blow away app.db.)

# 5. Click around:
#    - /admin/projects        → 2 seeded projects, indigo Bootstrap chrome
#    - /admin/tasks           → 5 seeded tasks; project_id renders as
#                                "Website redesign" / "Mobile app v2"
#                                (FK display field, clickable to the
#                                parent record)
#    - /admin/tasks/3/edit    → form with String / Number / Boolean /
#                                DateTime / FK <select> inputs
#    - /admin/actions         → audit timeline of every change
```

If you blow away `app.db` and need to recreate the admin user:

```bash
RUSTIO_CORE_PATH="$(pwd)/../../rustio-core" \
  cargo run --manifest-path ../../Cargo.toml -p rustio-cli -- \
  user create --email admin@taskhub.local --password demo1234 --role admin
```

## File map

```
examples/taskhub/
├── Cargo.toml                 # standalone workspace; points at ../../rustio-core
├── main.rs                    # server entry; --dump-schema for `rustio schema`
├── apps/
│   ├── mod.rs                 # registers projects + tasks apps
│   ├── projects/
│   │   ├── admin.rs           # admin.model::<Project>()
│   │   ├── models.rs          # Project — name, description, is_active, created_at
│   │   ├── mod.rs
│   │   └── views.rs           # (empty; add public routes here)
│   └── tasks/
│       ├── admin.rs           # admin.model::<Task>()
│       ├── models.rs          # Task — incl. project_id FK + Option<DateTime> due_at
│       ├── mod.rs
│       └── views.rs
├── migrations/
│   ├── 0001_create_projects.sql
│   ├── 0002_create_tasks.sql  # FK → projects(id) ON DELETE RESTRICT + indexes
│   └── 0003_seed_demo_data.sql
└── rustio.schema.json         # regenerated on every `rustio migrate apply`
```

## Try the AI pipeline

```bash
# From within examples/taskhub:
RUSTIO_CORE_PATH="$(pwd)/../../rustio-core" \
  cargo run --manifest-path ../../Cargo.toml -p rustio-cli -- \
  ai plan "add completed_at as optional DateTime to tasks"

# Saves a reviewable Plan; preview the risk + warnings:
#   … ai plan "…" --save plan.json
#   … ai review plan.json
#   … ai apply plan.json --yes

# The planner refuses to guess: ask for something it can't express
# (e.g. "delete the task table") and it returns a typed refusal,
# never free-form code generation.
```

## What this example does *not* cover

- **Public-facing routes.** The `apps/{projects,tasks}/views.rs` files are empty stubs. Add `pub fn register(router: Router) -> Router { … }` and wire your own HTML/JSON endpoints there.
- **A real workflow.** `Task.status` is a free-text string ("todo" / "in_progress" / "done"). For a real product, replace it with an enum + custom widget; the framework doesn't ship one yet.
- **Authentication for the public site.** The `authenticate` middleware in `main.rs` covers `/admin/*` only by default; non-admin routes are open.

## Configuration

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`
- `NO_COLOR` — disable colored CLI output
