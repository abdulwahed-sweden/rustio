# CLAUDE.md

Project-specific guidance for Claude Code sessions working in this
repository. Loaded automatically; treated as durable, override-the-
defaults instruction. If a rule here conflicts with a generic
default, this rule wins.

---

## Repository identity

**RustIO** is a production-grade, strict-by-construction Rust web
framework. Single-binary deploys (templates + CSS baked via
`include_str!`). PostgreSQL + sqlx, Meilisearch, hyper, minijinja,
argon2id sessions, granular permissions. The framework is the
business — projects (e.g. `examples/blog`, future `tolkhuset`) are
consumers.

Workspace layout:

```
rustio-core/            ~10k LOC — the framework (lib)
rustio-macros/          ~400 LOC — #[derive(RustioAdmin)]
rustio-cli/             ~600 LOC — `rustio` CLI (binary)
examples/blog/          ~150 LOC — reference consumer
docs/architecture.md    how the modules fit together
docs/phases/            one PHASE{N}.md per major phase + index
```

---

## Working style — phase-based porting

This repo is a phase-by-phase port from an OLDER codebase to a NEW
one. Each phase ships as one or more commits with a `phase {N}/{x}:`
prefix. The user drives the phase plan; Claude implements each
sub-phase, runs tests, commits, and reports.

The user typically:
- Hands a self-contained spec for the next sub-phase (Parts A–F).
- Expects a written audit if the spec might be wrong before any
  implementation begins.
- Wants test counts before/after, clippy clean, and a written
  decision log of any deviation.
- Reviews each commit before approving the next sub-phase.

The user does **NOT** want:
- Speculative refactoring outside the spec.
- New abstractions invented during implementation. If the spec says
  "add a function", add a function — don't add a trait.
- Phase reports drafted before the phase is done. (See
  `PHASE{N}.md commit hygiene` below.)

---

## Commit hygiene

### One phase, one phase report

`PHASE{N}.md` lands in **its own phase's final commit**, staged
explicitly by name. Never via `git add -A`. Past incident:
PHASE4.md got swept into a Phase 5a commit; required a follow-up
standalone commit to detach.

### Combined-vs-split commits

Default to one commit per sub-phase. Combine only when:
- The fix is intrinsic to a sub-phase that hasn't shipped yet
  (e.g. /f's template-registry fix landed inside the original /f
  commit because /f hadn't been pushed).
- The sub-phases are inseparable in the spec (e.g. /a + a follow-up
  schema migration that would crash the example without the enum).

When in doubt, ask. The cost of asking is low; the cost of a
mis-grouped history is high.

### Always create new commits, never amend

The CLAUDE Code default already says this. In this repo it's
load-bearing — a pre-commit hook failure means the commit didn't
happen, so `--amend` would touch the *previous* commit and drop
work. Re-stage, new commit.

### Stage by name, not by globbing

```bash
git add rustio-core/src/auth/users.rs \
        rustio-core/src/auth/mod.rs \
        rustio-core/assets/templates/admin/user_view.html
git commit -m "..."
```

Not `git add -A`. The phase-report rule depends on this.

### No `--no-verify`, no `--no-gpg-sign`, no `--no-edit`

Don't skip hooks. If a hook fails, fix the underlying issue.

---

## Test discipline

### Two test surfaces

```bash
cargo test --workspace --lib            # sandbox, ~325 tests, no DB
RUSTIO_TEST_DB=1 cargo test --lib -- --ignored   # ~41 PG-gated
```

Sandbox tests run on every commit. PG-gated tests run when the
spec involves DB-shaped behavior (cascades, race conditions, real
permission lookups). Both must be green before commit.

### Patterns — copy these, don't reinvent

#### Test that mutates `std::env`

```rust
use crate::auth::TEST_ENV_LOCK as ENV_LOCK;

#[tokio::test]
#[ignore = "needs `RUSTIO_TEST_DB=1` + a running postgres"]
async fn my_test() {
    let _env = ENV_LOCK.lock().await;
    std::env::set_var("RUSTIO_DEMO_MODE", "1");
    // ... test body ...
    std::env::remove_var("RUSTIO_DEMO_MODE");
}
```

The lock is `tokio::sync::Mutex<()>` — `std::sync::Mutex` is
forbidden by clippy's `await_holding_lock` lint. Defined once in
`auth/mod.rs`; import as `crate::auth::TEST_ENV_LOCK`.

#### Test that creates DB rows it must clean up

Use unique-tag emails (PID + nanos + random) so concurrent runs
don't collide. Clean up explicitly via
`DELETE FROM rustio_users WHERE id = $1`.

```rust
let email = format!(
    "test_{}_{}_{}@example.test",
    std::process::id(),
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
    rand::random::<u32>(),
);
```

#### Test that needs an isolated developer pool

`isolate_developers(&db, &[keep_id])` flips every other active
developer's `is_active` to FALSE for the test duration. Restore via
`restore_active_devs` at the end. Pattern is in
`auth/users.rs::tests`.

### Sandbox tests for templates

Every new admin template gets a sandbox render test that calls
`Templates::new(None).render("admin/<name>.html", &ctx)` with a
`serde_json::json!` context and asserts key fragments. This catches
**both** missing registry entries (the embedded loader returns "not
found") AND template syntax errors. The two together are the
single edit unit `(file, registry, render-test)` — see the
`feedback_template_registry` memory.

---

## Adding a new admin template — the triple

Three changes, one logical edit:

1. **Create the file** at
   `rustio-core/assets/templates/admin/<name>.html`.
2. **Register it** in `EMBEDDED_TEMPLATES` (`rustio-core/src/templates.rs`)
   with an `include_str!(...)` line.
3. **Add a render test** in the same `templates::tests` module:

```rust
#[test]
fn <name>_renders_with_basic_context() {
    let t = Templates::new(None).unwrap();
    let ctx = serde_json::json!({ /* every field the template reads */ });
    let body = t.render("admin/<name>.html", &ctx).unwrap();
    assert!(body.contains("...stable-string-from-template..."));
}
```

Without step 2, the template renders fine in dev mode (the disk
loader picks it up via `RUSTIO_TEMPLATE_DIR=...`) but the production
single-binary path returns 500 at request time. Cargo build,
clippy, and unit tests do NOT catch this; only browser smoke does
— and by then the commit has shipped.

This rule has fired twice:
- `coming_soon.html` in Phase 7a/0.5/e — caught before commit.
- `user_confirm_delete.html` in Phase 7a/0.5/f — caught only via
  browser smoke, after the commit. Required `f-fix1` to attach the
  registry line.

---

## Defense-in-depth across all five layers

When implementing a guard against a destructive action, all five
layers must agree:

| Layer | Where | What |
|---|---|---|
| Template conditional | `assets/templates/admin/*.html` | `{% if guard_flag %}disabled{% endif %}` on the submit |
| Context flag | `*Ctx` struct in `admin/builtin.rs` | `guard_flag: bool` |
| Show handler | `show_*` function | Compute the flag from DB / identity |
| Submit handler | `do_*` function | **Re-check** server-side, return `Err(BadRequest)` on bypass |
| Visual layer (CSS) | `admin/static/css/admin.css` | `:disabled { opacity, cursor: not-allowed }` |

Skipping any one is a bug. The visual layer (#5) was the late
discovery in `/f-fix2` — disabled buttons looked clickable and
operators read that as "guard isn't working" even when it was.

A unit test that greps the rendered HTML for `disabled` does not
prove the user sees a disabled button. Smoke test the rendered
colour, not just the attribute.

---

## Authorization mental model

Two parallel grammars, never conflated:

- **Role** answers "which surfaces can this user reach?". Linear
  ladder: User < Staff < Supervisor < Administrator < Developer.
  Use `role_guard(min: Role)` at the route layer.
- **Permission** answers "which row can this user mutate?".
  Bag-of-codenames (`posts.add_post`, `posts.change_post`, ...).
  Use `perm_guard(perm: &str)` at the route layer.

`Administrator` and `Developer` bypass permission checks
(`bypasses_group_checks()`); they are the "trusted operator" tier.
`Staff` and `Supervisor` go through the permission machinery.

`is_active = FALSE` short-circuits both. Always check `is_active`
**before** the bypass — that's the `/sec2` lesson.

---

## Cache invalidation

Permission cache: 60s TTL, keyed by `user_id`, in-process `DashMap`.

Helpers in `auth/permissions.rs`:
- `add_user_to_group` / `remove_user_from_group` — invalidate
  internally.
- `invalidate_user_cache(user_id)` — explicit (pub(crate)).
- `invalidate_group_cache(db, group_id)` — fire-and-forget tokio
  task that loops over members.

**The rule:** any wholesale write that bypasses the per-pair
helpers needs an explicit `invalidate_user_cache` call.
Specifically:
- `DELETE FROM rustio_user_groups WHERE user_id = $1` →
  `invalidate_user_cache(user_id)` immediately after.
- `DELETE FROM rustio_groups WHERE id = $1` (cascades through M2M)
  → capture member ids before the DELETE, loop
  `invalidate_user_cache` over them after.

This is the `/sec3` lesson; it's load-bearing because the cache
is the only place stale permissions can live for up to a minute.

---

## Route registration order matters

The router (`rustio-core/src/router.rs`) matches in **insertion
order, first match wins**. `:id` is a wildcard that will swallow
any literal segment registered AFTER it.

Pattern:

```rust
// Specific paths FIRST
let r = r.get("/admin/users/new",    handler);
let r = r.get("/admin/users/:id/edit", handler);
let r = r.get("/admin/users/:id/delete", handler);
// Wildcard LAST
let r = r.get("/admin/users/:id",    handler);
```

Reverse the order and `/admin/users/new` resolves to `:id="new"`,
which fails parse_id, returning a 400 instead of the new-user form.

---

## CLI escape hatches

When a UI guard refuses a destructive action, there should usually
be a CLI command that lets an operator perform the same action with
explicit confirmation. The pattern in `/f`:

```rust
// rustio-cli/src/main.rs
RoleAction::Set { email, role, yes, db } => {
    if would_orphan && !yes && !confirm_orphan(&email)? {
        return Err("aborted".into());
    }
    // proceed
}
```

`confirm_orphan` reads `I UNDERSTAND` (long, paste-prevention) from
stdin. `--yes` skips the prompt for scripted operators. The CLI
must support both interactive and stdin-piped use.

---

## Demo mode

`RUSTIO_DEMO_MODE=1` is the **only** runtime control for demo user
seeding. Never gate it behind a `cfg!` or compile-time flag. The
same binary serves demo and prod — the env var is the toggle.

Demo users:
- One per role (5 users).
- Email pattern: `<role>@<branding.domain>`.
- Password is the role slug itself (paste-testable).
- `is_demo = TRUE`, `demo_label = Some("...")` flag the row.
- Demo banner renders on every page when `is_demo_session` in
  `BaseContext` is true.

---

## Memory system

There's a project-scoped memory system at
`~/.claude/projects/-Users-mansour-Documents-rustio/memory/`.
Three durable feedback files live there as of Phase 7a/0.5:

- `feedback_phase_reports.md` — phase-report commit hygiene rule.
- `feedback_pg_create_table_race.md` — `tokio::sync::OnceCell` for
  shared-table init in parallel tests; also the env-lock pattern.
- `feedback_template_registry.md` — the (file, registry,
  render-test) triple + the visual-layer defense-in-depth rule.

Read these before assuming you've seen all the project's
conventions. New rules learned during a session should be saved
there immediately.

---

## Hard stops — actions that require explicit user approval

Some actions are not reversible by `git reset` and have blast radius
beyond the local clone. They are **hard stops**: never run them on
your own initiative, even when a previous instruction in the same
spec implies they're expected. Approval given for one such action in
one conversation does **not** carry over.

### Repo operations

- Creating a new GitHub repo (`gh repo create`).
- Renaming or deleting a remote repo (`gh repo rename`, `gh repo
  delete`).
- Pushing a new branch to origin (`git push -u origin <branch>`),
  including the first push of `main` to a fresh remote.
- Force-pushing anywhere (`git push --force`, `git push +<ref>`).
- Adding, removing, or changing a git remote
  (`git remote add/remove/set-url`).
- Visibility changes (public ↔ private).

Each requires the user to confirm in the current conversation.
"User has a public repo with this name on their account" is not
sufficient context — ask.

### Documentation operations

- Moving phase reports between directories (e.g.
  `docs/phases/` ↔ root).
- Renaming `STATUS.md` / `PROGRESS.md` / `PHASE*.md`.
- Rewriting `docs/architecture.md`, `CLAUDE.md`, `CHANGELOG.md`,
  `README.md` for content (typo fixes are fine).
- Deleting any historical doc, even if it looks superseded.
- Adding new top-level docs at the repo root.

The phase-report files in particular are deliverables; treat them
like code, not like notes.

### Database operations

- `DROP TABLE`, `DROP DATABASE`, `TRUNCATE`.
- `DELETE FROM rustio_users` without a `WHERE` clause, or with a
  `WHERE` that covers more than the test fixtures the user
  acknowledged.
- Restoring or rolling back the dev DB (e.g. dropping and
  re-bootstrapping).
- Migrations against a non-test DB.
- Anything that would invalidate the user's open sessions
  (`DELETE FROM rustio_sessions`).

The dev DB (`rustio_dev` on local Postgres) is the user's working
state; don't reset it without explicit go-ahead even when "starting
fresh" looks like the obvious move.

---

## When in doubt — ask

The user has explicitly said they prefer being asked over having
Claude guess wrong. The cost of one extra round-trip is much lower
than the cost of a misdirected commit, a destructive `git reset`,
or a phase report drafted prematurely.

Specific places to pause and ask:
- Combined commit vs split commit when both are defensible.
- Renaming or deleting a remote (e.g. GitHub repo).
- Force-pushing anywhere.
- Skipping any of the five defense-in-depth layers.
- Diverging from a written spec, even if the divergence is
  obviously better.

The user is also fine with Claude **disagreeing** with the spec —
write the audit, lay out the trade-off, and let them decide.

---

## What "done" means for a sub-phase

A sub-phase is done when **all** of these are true:

1. `cargo check --workspace` clean.
2. `cargo test --workspace --lib` green (sandbox).
3. `cargo clippy --workspace --all-targets` clean.
4. PG-gated tests green where applicable
   (`RUSTIO_TEST_DB=1 cargo test --ignored`).
5. Browser smoke matrix (curl-based, no human in loop) green
   where the sub-phase touches HTTP surfaces.
6. The commit message documents deviations from the spec, the
   reasoning, and the test counts before/after.
7. The commit was created with explicit `git add <files>`, not
   `-A`.

Anything less, the sub-phase is **in_progress**.

---

*Last updated: end of Phase 7a/0.5.*
