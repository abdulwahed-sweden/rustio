# Phase 12 — AI-Native Project Scaffolding Plan

## Core Idea

> RustIO doesn't just generate a project.
> RustIO understands the project, reads its config, preserves its design,
> and gives AI agents clear context so they don't break the system.

---

## End-State Goal

When a developer runs:

```bash
rustio startproject clinic
cd clinic
```

They get a structured project:

```txt
clinic/
├── .ai/
│   └── context.md
├── .rustio/
│   ├── project.lock
│   └── overrides.lock
├── templates/
│   ├── overrides/
│   │   ├── brand.html
│   │   ├── footer.html
│   │   └── admin/
│   │       └── base.html
│   ├── home.html
│   ├── api.html
│   └── docs.html
├── src/
├── migrations/
└── Cargo.toml
```

---

## The Three-Layer Philosophy

### Layer 1 — Core (owned by RustIO)
- admin system
- auth
- RBAC
- original templates
- design system
- migrations engine

### Layer 2 — Project (owned by the developer)
- models
- apps
- home page
- project templates
- branding overrides

### Layer 3 — AI Context (consumed by Claude Code / any AI agent)
- What is the project?
- What is allowed to be modified?
- What is forbidden?
- Where do files go?
- What is the design system?
- How do migrations work?
- How is a new app added?

---

## Execution Order

The order is non-negotiable: scaffold first, then doctor recognizes it,
then tooling around overrides, then AI-aware commands. We do **not**
start with `templates upgrade` because it depends on every layer below.

---

## Phase 12/a — AI Context Scaffold

**Goal:** any AI agent that enters a freshly-generated project understands
it from the first read.

### Files added by `rustio startproject <name>`

#### 1. `.ai/context.md`

A human-and-AI-readable description of the project:

- Project name (derived from CLI arg)
- Domain (placeholder, prompts the developer to fill in)
- Main resources (placeholder list)
- **Do** rules:
  - Add new apps with `rustio startapp <name>`
  - Put model code in `src/apps/<app>/models.rs`
  - Add migrations in `migrations/`
  - Follow the existing admin design system
  - Use `templates/overrides/` only for intentional framework overrides
- **Do Not** rules:
  - Do not modify `rustio-core` directly
  - Do not rewrite admin templates unless explicitly requested
  - Do not invent new colors
  - Do not change design tokens randomly
  - Do not add SQLite support
  - Do not auto-seed production users
  - Do not modify `.rustio/*.lock` manually
- Design system block (primary brand color, admin classes)
- Template rules (which paths are project-level vs framework-override)
- Database (PostgreSQL only)
- Common commands (`rustio doctor`, `rustio user create`, `cargo run`)

#### 2. `.rustio/project.lock`

Machine-readable metadata, not for the developer:

```toml
[project]
name = "clinic"
rustio_version = "1.7.1"
created_with_cli = "1.7.1"

[database]
backend = "postgres"

[design]
brand = "#0d9488"
design_system = "rustio-admin-v1"

[ai]
context = ".ai/context.md"
```

#### 3. README update

Document `.ai/` and `.rustio/` directories so developers don't delete
them as "weird hidden folders".

### Implementation steps

1. Add a `templates/scaffold/` directory inside `rustio-cli/` with
   the seed files (`context.md.tmpl`, `project.lock.tmpl`).
2. Extend `startproject` to render those templates with the project
   name and current CLI version substituted in.
3. Add a sandbox unit test that runs `startproject` into a tempdir
   and asserts every expected file exists and parses (TOML for
   `project.lock`, non-empty content for `context.md`).
4. Update `docs/architecture.md` with a one-paragraph note on the
   three-layer model.

### Outcome

Any AI agent that opens a fresh project can answer "what is this
project, what may I touch, what must I leave alone?" from a single
file read.

---

## Phase 12/b — Template Structure

**Goal:** `templates/` is no longer empty and confusing; the override
boundary is visible from the directory layout itself.

### Files added by `startproject`

| Path | Purpose |
|---|---|
| `templates/home.html` | Public landing page; developer edits freely |
| `templates/api.html` | API info page; developer edits freely |
| `templates/docs.html` | Project docs page; developer edits freely |
| `templates/overrides/brand.html` | Framework brand block override |
| `templates/overrides/footer.html` | Framework footer override |
| `templates/overrides/admin/base.html` | Admin shell override (advanced) |

Each file ships with an HTML comment at the top explaining whether
it is a project page or a framework override, and a link back to
`.ai/context.md`.

### Implementation steps

1. Add the seed templates under `rustio-cli/templates/scaffold/`.
2. Wire them into the `startproject` renderer.
3. Confirm the generated project's `cargo run` actually serves
   `templates/home.html` at `/` (this requires a route; if the
   route doesn't exist yet, scope is to add a thin `home_handler`
   in the generated `src/main.rs`).
4. Sandbox test: render each scaffolded template with a minimal
   minijinja context and assert key fragments — same triple rule
   as `CLAUDE.md`'s "Adding a new admin template" section.

### Outcome

A developer cloning a fresh project sees a populated `templates/`
folder with a clear convention: top-level = mine, `overrides/` = theirs.

---

## Phase 12/c — Doctor Integration

**Goal:** `rustio doctor` becomes the single command that tells an
operator "is this project healthy and complete?".

### New checks added to `rustio doctor`

In addition to the existing `DATABASE_URL`, PostgreSQL, and
Meilisearch checks:

1. **Project root** — there is a `.rustio/project.lock` in the cwd.
2. **AI context** — `.ai/context.md` exists and is non-empty.
3. **Project lock** — `.rustio/project.lock` parses as TOML and
   contains a `[project]` block with `name` and `rustio_version`.
4. **Overrides lock** — `.rustio/overrides.lock` exists (may be
   empty if no overrides taken yet).
5. **CLI version match** — installed `rustio-cli` version is
   compatible with `project.lock.project.rustio_version`. Warn
   (not error) on minor mismatch.

### Output format

```txt
RustIO doctor — checking your project

✓ Project root       clinic
✓ DATABASE_URL       postgres://clinic:***@localhost/clinic_dev
✓ PostgreSQL         connected
✓ Meilisearch        reachable
✓ AI context         .ai/context.md
✓ Templates          overrides locked
✓ CLI version        1.7.1

Status: READY ✓
Next: cargo run
```

### Implementation steps

1. Add a `project::detect()` helper to `rustio-cli` that loads
   `.rustio/project.lock` and returns a structured value or `None`.
2. Extend the existing `doctor::run()` flow to call `detect()`
   first, then layer the new checks.
3. Add a `--json` flag for machine consumption (the existing CLI
   already mostly supports this; just keep parity).
4. Sandbox test that doctor returns the expected check set against
   a fixture project tree.

### Outcome

Operators can type one command after `git clone` and know whether
the project is fully wired.

---

## Phase 12/d — Template Override Tooling

**Goal:** developers can take a framework template, customize it,
and still receive upstream updates safely.

### `.rustio/overrides.lock` format

```toml
[overrides]
"brand.html"        = { version = "1.7.1", hash = "abc123" }
"footer.html"       = { version = "1.7.1", hash = "def456" }
"admin/base.html"   = { version = "1.7.1", hash = "ghi789" }
```

The hash is a SHA-256 of the upstream template at the version the
override was taken from. The lock is updated whenever a developer
re-syncs an override.

### New CLI subcommands

#### `rustio templates doctor`

Walks `templates/overrides/*`, reads `overrides.lock`, and reports
per-override status:

```txt
Template health check

✓ brand.html is up to date
⚠ admin/base.html is based on 1.7.1, latest is 1.8.0

Run:
  rustio templates diff admin/base.html
  rustio templates upgrade admin/base.html
```

#### `rustio templates diff <path>`

Prints a unified diff between the developer's override and the
current upstream template. Read-only.

#### `rustio templates upgrade <path>`

Three-way merge:
- base = upstream at `lock.version`
- ours = current override
- theirs = upstream at current CLI version

On clean merge: rewrites the override, bumps the lock entry.
On conflict: writes `<path>.conflict` with conflict markers and
exits non-zero.

#### `rustio templates take <upstream-path>`

Copies an upstream template into `templates/overrides/<upstream-path>`
and records a fresh entry in `overrides.lock`. This is the "I want
to customize this" entry point.

### Implementation steps

1. Embed the upstream template versions in the CLI binary at build
   time (already partially possible via `EMBEDDED_TEMPLATES`; expose
   an iterator + content getter).
2. Add `rustio-cli/src/templates/` module with `take`, `diff`,
   `upgrade`, `doctor` submodules.
3. Use `similar` crate for diff/merge.
4. Sandbox tests for each subcommand against a fixture project.

### Outcome

Customizing the framework stops being a "fork-and-pray" operation.

---

## Phase 12/e — AI Commands

**Goal:** the CLI itself becomes an AI-aware tool, not just an
operator's tool.

### New CLI subcommands

#### `rustio ai context`

- With no args: prints `.ai/context.md` to stdout (so an AI can
  shell out and read it without parsing paths).
- `--regenerate`: re-renders `context.md` from `project.lock`
  and the current CLI's defaults, preserving the developer's custom
  Do/Do-Not lines. Asks for confirmation before overwriting.

#### `rustio ai explain`

Prints a human summary of the project: name, domain, registered
apps (by reading `src/apps/*`), migrations applied, demo mode
state. No network calls.

#### `rustio ai plan <feature>`

Generates a plan-only (no file writes) for adding a new feature:
which files would be created, which migrations, which routes.
Reads `.ai/context.md` to respect the project's Do/Do-Not rules.

This subcommand is **plan-only**; it never writes code. It is the
bridge between an AI agent and the framework's conventions.

### Implementation steps

1. Add `rustio-cli/src/ai/` module.
2. Each subcommand is a pure function that reads project state and
   prints to stdout.
3. Sandbox tests: each subcommand runs against a fixture project
   and produces deterministic output.

### Outcome

> "Read `.ai/context.md`, then add an `appointments` app without
> touching core or the design system."

…becomes a one-line instruction an operator gives an AI agent, and
the agent has every command it needs locally.

---

## Cross-Phase Concerns

### Versioning

Every artifact (`project.lock`, `overrides.lock`, `context.md`)
records the CLI version that produced it. The doctor warns on
mismatch but never auto-migrates without explicit consent.

### Backwards compatibility

Existing projects (created before Phase 12) have neither `.ai/`
nor `.rustio/`. Doctor must detect this gracefully and offer:

```txt
⚠ Legacy project detected (no .rustio/project.lock)

Run:
  rustio init --upgrade

to scaffold AI context and project metadata into this project.
```

`rustio init --upgrade` is added in Phase 12/a as a one-shot
migrator for legacy projects.

### Test discipline

Per `CLAUDE.md`: every new template gets the (file, registry,
render-test) triple. Every new CLI subcommand gets a sandbox unit
test. PG-gated tests where DB state is involved (none expected in
Phase 12, but doctor's PG check still runs).

### Hard stops to confirm before any commit

- Adding new top-level docs at the repo root → ask first.
- Modifying `package.json` / Tailwind config → ask first.
- Any change to `rustio-core` for design-system tokens → ask first.

---

## Recommended Starting Point

```txt
Phase 12/a — generate .ai/context.md and project metadata
```

Smallest blast radius, biggest immediate win for AI agents,
unblocks every later sub-phase.
