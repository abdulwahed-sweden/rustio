# Phase 7a/0.5 — Authorization, Demo Users, View-First Navigation

This is the long-form report for Phase 7a/0.5 of the RustIO 1.0 port.
It covers twelve commits (`/a` through `/h`, plus the four `sec` patches
and the `f-fix2` follow-up) and brings the admin panel from
"functional but ad-hoc role checks" to a coherent five-tier
authorization system with a demo-mode bootstrap, a developer-only
diagnostic surface, hardened destructive operations, and a view-first
profile page that becomes the navigation hub for user management.

Built on top of Phase 7a/0 (commit `710f5d6` — `SiteBranding` API).
The next phase is Phase 7a/1 (the tolkhuset crate skeleton).

---

## Table of contents

1. [Where we were on /a-day](#where-we-were-on-a-day)
2. [The twelve commits at a glance](#the-twelve-commits-at-a-glance)
3. [`/a` — five-tier role hierarchy + schema upgrade](#a--five-tier-role-hierarchy--schema-upgrade)
4. [`/sec1` — group delete (Phase 6b/4 gap)](#sec1--group-delete)
5. [`/sec2` — `is_active` before bypass (defense-in-depth)](#sec2--is_active-before-bypass)
6. [`/sec3` — cache invalidation on wholesale group removal](#sec3--cache-invalidation-on-wholesale-group-removal)
7. [`/sec4` — sanitize Postgres errors on user creation](#sec4--sanitize-postgres-errors-on-user-creation)
8. [`/b` — `role_guard` + `perm_guard` + forbidden page](#b--role_guard--perm_guard--forbidden-page)
9. [`/c` — `SiteBranding.domain` + default groups + lazy permission attachment](#c--sitebrandingdomain--default-groups--lazy-permission-attachment)
10. [`/d` — five demo users + role-select UI + demo banner](#d--five-demo-users--role-select-ui--demo-banner)
11. [`/e` — guards across every admin handler + developer stubs](#e--guards-across-every-admin-handler--developer-stubs)
12. [`/f` — last-developer guard + user delete + CLI escape hatch](#f--last-developer-guard--user-delete--cli-escape-hatch)
13. [`/f-fix2` — disabled-button CSS + acceptance suite](#f-fix2--disabled-button-css--acceptance-suite)
14. [`/h` — user profile view + row-clickable navigation](#h--user-profile-view--row-clickable-navigation)
15. [Test surface evolution](#test-surface-evolution)
16. [Database schema changes](#database-schema-changes)
17. [Architectural decisions](#architectural-decisions)
18. [Lessons learned (memory references)](#lessons-learned-memory-references)
19. [Open security items + future work](#open-security-items--future-work)
20. [Verification at end-of-phase](#verification-at-end-of-phase)
21. [Phase 7a/1 readiness](#phase-7a1-readiness)

---

## Where we were on /a-day

End of Phase 7a/0 (commit `710f5d6`):

- **Auth.** `Role` was a 3-variant enum: `User`, `Staff`, `Admin`. The
  `Admin` variant was a superuser bypass — it skipped every permission
  check and was the only way to access the user/group management
  pages.
- **Permissions.** Direct user-permission grants and group-permission
  grants were both supported, with a 60s `DashMap` LRU cache.
  Permissions were emitted by `#[derive(RustioAdmin)]` per model
  (`<admin>.add_<model>` / `change_` / `delete_` / `view_`).
- **Admin entry-points.** Every admin handler used an ad-hoc
  `admin_only_guard` that just checked `identity.is_admin()`. There
  was no concept of "Staff with view-only on Posts but no delete" or
  "developer-only diagnostics".
- **Demo bootstrap.** A single hardcoded `admin@example.com / admin`
  seed for the blog example. No notion of demo users, no in-app
  signal that a session was a demo session.
- **Site branding.** `Admin::site_branding(SiteBranding { ... })` was
  available (Phase 7a/0) — `site_title`, `site_header`, `index_title`,
  `footer_copyright`. The `domain` field hadn't been added yet.
- **Built-in pages.** `/admin/users`, `/admin/users/:id/edit`,
  `/admin/users/new`, `/admin/groups`, `/admin/groups/:id/edit`,
  `/admin/groups/new` — but **no** group-delete, no user-delete, no
  user-view. The path `/admin/users/:id/edit` was the click target on
  the list.
- **Tests.** 291 passing / 21 ignored.

Pre-/0.5 audit (Step 1) flagged the gaps that the twelve commits
below close:

| Gap | Closed by |
|---|---|
| `Admin` is one role with bypass; no Supervisor / Administrator / Developer distinction | `/a` |
| `/admin/groups/:id/delete` doesn't exist | `/sec1` |
| `bypasses_group_checks` runs before `is_active` check | `/sec2` |
| Wholesale `DELETE FROM rustio_user_groups` doesn't invalidate the perm cache | `/sec3` |
| `create_user` leaks Postgres internals on duplicate-email | `/sec4` |
| `admin_only_guard` is the only granularity available | `/b` |
| Demo users hardcoded with rustio.local domain | `/c` |
| One demo user covers all roles | `/d` |
| Some handlers still ad-hoc — gates not uniform | `/e` |
| Last active developer can be demoted/deleted into a no-developer state | `/f` |
| Disabled button looks identical to enabled | `/f-fix2` |
| Clicking a list row sends you to /edit; no read-only view | `/h` |

---

## The twelve commits at a glance

```
7a6ad32  phase 7a/0.5/h:      user profile view page + row-clickable users list navigation
dfac6b5  phase 7a/0.5/f-fix2: visually disable .deletelink-button when [disabled]
9aaa23d  phase 7a/0.5/f:      last-developer guard (UI block + CLI escape) + user delete handler + template registry fix
e57e61c  phase 7a/0.5/e:      apply role/perm guards across all admin handlers + 3 developer stubs
907d435  phase 7a/0.5/d:      5 demo users + role-select UI + demo banner
cd23b39  phase 7a/0.5/c:      SiteBranding.domain + 6 default groups + lazy permission attachment
7a000e4  phase 7a/0.5/b:      role_guard + perm_guard + forbidden page
9b0ff4a  phase 7a/0.5/sec4:   sanitize create_user error to avoid Postgres detail leak
4578c85  phase 7a/0.5/sec3:   invalidate perm cache on wholesale group removal
f205056  phase 7a/0.5/sec2:   check is_active before bypass in permission check (defense-in-depth)
928875c  phase 7a/0.5/sec1:   group delete (closes Phase 6b/4 gap)
be6bec6  phase 7a/0.5/a:      5-tier role hierarchy + schema upgrade
```

The four `sec*` commits sit between `/a` and `/b` chronologically.
They were extracted from the audit so each fix lands as its own
reviewable change rather than getting buried in `/b`.

---

## `/a` — five-tier role hierarchy + schema upgrade

**Commit:** `be6bec6` — *5-tier role hierarchy + schema upgrade*

### What landed

A new `auth/role.rs` module replaces the old 3-variant `Role`:

```rust
pub enum Role {
    User,           // rank 2 — no admin access
    Staff,          // rank 3 — can access /admin
    Supervisor,     // rank 4 — view + edit, no destructive ops
    Administrator,  // rank 5 — full coverage; bypasses group checks
    Developer,      // rank 6 — Administrator + diagnostic surfaces
}

impl Role {
    pub fn rank(&self) -> u8 { ... }
    pub fn includes(&self, other: Role) -> bool { self.rank() >= other.rank() }
    pub fn parse(s: &str) -> Result<Self> { ... }
    pub fn as_str(&self) -> &'static str { ... }
    pub fn label(&self) -> &'static str { ... }
    pub fn can_access_panel(&self) -> bool { self.rank() >= Role::Staff.rank() }
    pub fn bypasses_group_checks(&self) -> bool { self.rank() >= Role::Administrator.rank() }
}
```

Six methods, one struct, one ladder check (`includes`) — that's the
entire surface. Every other piece of /0.5 code reads from these.

### Schema upgrade

The hardest part wasn't the enum — it was migrating an existing DB
that had `role = 'admin'` rows (Phase 6b's name) to the new
`'administrator'` value, **before** adding the `CHECK` constraint that
would otherwise reject the row. Three steps in `migrate_user_schema`:

1. `UPDATE rustio_users SET role = 'administrator' WHERE role = 'admin'`
2. `ALTER TABLE … ADD COLUMN IF NOT EXISTS is_demo`, `demo_label`
3. `DO $$ … IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'rustio_users_role_check') THEN ALTER TABLE … ADD CONSTRAINT … CHECK (role IN ('user','staff','supervisor','administrator','developer')) END $$`
4. Two indexes (`role` + a partial `is_demo WHERE is_demo = TRUE`).

`migrate_user_schema` is idempotent. Safe on a fresh DB and on a
Phase-6b DB.

### Coupling caught before commit

The first attempt shipped `migrate_user_schema` and the `Role` enum
extension as separate commits. The blog example crashed with `unknown
role: administrator` because it had run the schema migration but
hadn't been recompiled with the new enum yet. The DB was rolled back,
the two changes were merged into one atomic commit, and that's the
shape of `be6bec6`.

> User feedback: "Excellent work catching this before commit. Restoring
> the DB was the right move — that's the discipline we need."

### Tests

25-case `includes` ladder matrix in `auth/role.rs`. Every cross-pair
checked: `User ⊂ Staff ⊂ Supervisor ⊂ Administrator ⊂ Developer`,
identity, and the negative direction.

---

## `/sec1` — group delete

**Commit:** `928875c` — *group delete (closes Phase 6b/4 gap)*

### Why

Phase 6b shipped `/admin/groups/:id/edit` but no delete. An admin
could create a group, attach permissions, but then never delete it
without dropping the row in `psql`. This is a usability gap, not a
security one — but per the audit it sits in the auth/admin work
chronologically and the cascade behavior (M2M tables clearing) needed
care.

### What landed

```
GET  /admin/groups/:id/delete  →  show_group_delete  (confirm page)
POST /admin/groups/:id/delete  →  do_group_delete    (cascades + invalidates)
```

`show_group_delete` renders `admin/group_confirm_delete.html` with:
- group `name` + `description`
- a `user_count` (memberships about to break)
- a `perm_count` (permission attachments about to break)

`do_group_delete` first **captures every `user_id` that's a member of
the group** before issuing the `DELETE`. That list seeds the explicit
`invalidate_user_cache` loop after the cascade — without it, every
formerly-grouped user keeps their cached permission set for up to 60
seconds (the `PERM_CACHE_TTL`). This pattern recurs in `/sec3`.

### Test posture

Sandbox: 4 new tests covering the confirm-page rendering + the
cascade-list math (`user_count != 1 ? "memberships" : "membership"`
plural fork). PG-gated: 2 tests covering the actual cascade + cache
invalidation.

---

## `/sec2` — `is_active` before bypass

**Commit:** `f205056` — *check is_active before bypass in permission check (defense-in-depth)*

### The bug

`check_permission(identity, perm)` used to short-circuit on
`role.bypasses_group_checks()` first, returning `true` for any
Administrator or Developer. **Then** it checked `is_active`. An
Administrator marked `is_active = FALSE` could still pass the gate
because the bypass returned `true` before the active check ran.

### The fix

Reorder: `is_active` first, then bypass:

```rust
pub fn check_permission(identity: &Identity, perm: &str) -> Result<bool> {
    if !identity.is_active {
        return Ok(false);
    }
    if identity.role.bypasses_group_checks() {
        return Ok(true);
    }
    // ... DB lookup
}
```

This is defense-in-depth: `login_guard` already rejects sessions
attached to inactive users, so `check_permission` shouldn't see an
inactive identity. But "shouldn't" is not "won't" — caches, race
windows, deactivation while a session is mid-flight. The ordering
fix is cheap, durable insurance.

### Tests

The PG-gated test creates an Administrator, runs `check_permission`
through a fake gate, deactivates the user via SQL (simulating an
admin flipping the flag while the session is still alive), and
asserts the next call returns `false`. Pre-fix this test failed
deterministically.

---

## `/sec3` — cache invalidation on wholesale group removal

**Commit:** `4578c85` — *invalidate perm cache on wholesale group removal*

### The bug

`do_user_edit` resets group membership by issuing
`DELETE FROM rustio_user_groups WHERE user_id = $1` and then
`INSERT … (gid, $1)` for each ticked checkbox. The per-pair
`add_user_to_group` and `remove_user_from_group` helpers each call
`invalidate_user_cache` internally. The wholesale `DELETE` doesn't.

So a user demoted to **zero** groups (every checkbox unticked) kept
every permission for up to 60s — the perm cache was populated and
nothing invalidated it.

### The fix

A single explicit call after the wholesale `DELETE`:

```rust
sqlx::query("DELETE FROM rustio_user_groups WHERE user_id = $1")
    .bind(user_id).execute(...).await?;
auth::invalidate_user_cache(user_id);   // <-- this line
for gid in wanted { auth::add_user_to_group(...).await?; }
```

The `add_user_to_group` calls in the checkbox loop also invalidate,
but only when at least one box is ticked. The all-unchecked case
hits this path and only this path — without the explicit call, it's
the entire bug.

`do_group_delete` (from `/sec1`) carries the same pattern: capture
member ids before the cascade, then invalidate each.

---

## `/sec4` — sanitize Postgres errors on user creation

**Commit:** `9b0ff4a` — *sanitize create_user error to avoid Postgres detail leak*

### The bug

```
$ rustio user create --email alice@x.com --password ...   # second time
error: error returned from database: duplicate key value violates
       unique constraint "rustio_users_email_key"
```

Two things wrong: (a) the user sees the constraint name, the
`SQLSTATE`, the words "duplicate key value" — Postgres internals; (b)
the same shape will surface in the admin "Add user" form, which means
a HTML page rendering raw DB errors. Information leak, ugly UX.

### The fix

Wrap the `sqlx::Error` and look for the constraint name:

```rust
.map_err(|e| {
    log::warn!("create_user failed for {email}: {e}");   // server logs keep the detail
    if e.to_string().contains("rustio_users_email_key") {
        Error::BadRequest("An account with this email already exists.".into())
    } else {
        Error::BadRequest("Could not create user. Please check your input.".into())
    }
})?;
```

Server-side `log::warn!` keeps the full detail for ops; the client
gets a clean, actionable message.

### Tests

`duplicate_email_is_clean_error_message` (PG-gated) inserts twice
with the same email and asserts the returned message:
1. **Contains** "already exists"
2. **Does not contain** any of: `rustio_users_email_key`, `duplicate
   key value`, `constraint`, `SQLSTATE`, `23505`, `Postgres`, `pg::`

The "deny list" approach is more robust than asserting an exact
message — it catches new SQLx error format changes too.

---

## `/b` — `role_guard` + `perm_guard` + forbidden page

**Commit:** `7a000e4` — *role_guard + perm_guard + forbidden page*

### Surface added

Two new guards in `admin/routes.rs` that every admin handler will
adopt in `/e`:

```rust
pub enum Guard {
    Allow(Identity),
    Redirect(Response),
}

pub async fn role_guard(ctx: &AdminCtx, req: &Request, min: Role) -> Result<Guard>;
pub async fn perm_guard(ctx: &AdminCtx, req: &Request, perm: &str) -> Result<Guard>;
pub async fn perm_guard_verdict(...) -> Result<PermVerdict>;  // for tests
```

`Guard::Allow(Identity)` carries the authenticated, authorized
identity to the handler. `Guard::Redirect(Response)` carries a
non-Allow response — that response can be a 303 redirect to login
**or** a fully-rendered 403 HTML page. The variant name is misleading
(it's not always a redirect), but the shape stayed for backwards
compat.

### `admin/forbidden.html`

A real Django-style 403 page: breadcrumb, "You don't have access to
this page" headline, the missing permission codename in a `<code>`
block, a "Return to dashboard" link. Replaces the previous text/plain
"403 Forbidden" body.

The page renders inside the regular admin chrome (sidebar, header,
demo banner) so it's clearly part of the panel rather than a router
fallback.

### `perm_guard_verdict` — tests-only surface

`perm_guard` returns `Guard`. For sandbox tests we want to assert
exactly which gate failed (was it the session? the role floor? the
permission? the bypass?) without spinning up an HTTP layer. A 12-case
sandbox test matrix in `admin/routes.rs` covers all the combinations.

### Test count

291 → 304 (+13). All sandbox.

---

## `/c` — `SiteBranding.domain` + default groups + lazy permission attachment

**Commit:** `cd23b39` — *SiteBranding.domain + 6 default groups + lazy permission attachment*

### `SiteBranding.domain`

`SiteBranding` gains a fifth field: `domain: String`. Default value:
`"rustio.local"`. This is the email domain demo users are seeded
with — `staff@<domain>`, `supervisor@<domain>`, etc.

Why a separate field? Because for a real project (tolkhuset), the
demo bootstrap should generate `staff@tolkhuset.test` rather than
`staff@rustio.local`. Hardcoding `"rustio.local"` would force every
deployment to either disable demo mode or live with the wrong
domain.

### `bootstrap_default_groups`

Six groups, idempotent insertion via `ON CONFLICT (name) DO NOTHING`:

| Group | Description |
|---|---|
| Auditors | Read-only audit access |
| Content Editors | Full CRUD on content models |
| HR Managers | User management + reporting |
| Finance | Financial models + reports |
| Project Coordinators | Cross-team coordination |
| System Operators | System monitoring + ops |

These names + descriptions are deliberately generic so they apply to
any project. The corresponding *permissions* attached to each group
are project-specific — that's what `lazy_attach_permissions` handles.

### Lazy permission attachment — the key insight

The naive design: `bootstrap_default_groups` attaches permissions to
groups at boot. Problem: at boot, the `Admin` instance has been
declared but `seed_permissions` may not have run yet, so the
permission rows don't exist in `rustio_permissions`. You can't
attach `posts.add_post` to "Content Editors" if `posts.add_post`
isn't in the catalogue.

The lazy design: groups are created with **no permissions attached**.
Per-permission attachment runs whenever a permission is
**registered** (called from `seed_permissions`). At that moment, the
function looks up which default groups should hold the permission
(by codename pattern) and inserts the M2M row.

```rust
pub async fn lazy_attach_permissions(db: &Db) -> Result<()> {
    for spec in DEFAULT_GROUP_SPECS {
        let gid = find_group_id_by_name(db, spec.name).await?;
        for code in spec.permission_codes() {
            // INSERT … ON CONFLICT DO NOTHING — idempotent
        }
    }
    Ok(())
}
```

Result: bootstrap order is `init_tables → bootstrap_default_groups →
seed_permissions → lazy_attach_permissions → bootstrap_demo_users`.
Each step is idempotent.

### Tests

12 sandbox tests + 4 PG-gated. Sandbox covers the spec resolution
(`GroupSpec::All` flattens to every model's CRUD perms when an
`Admin` with at least one entry is supplied — the `admin_with_post_entry`
test helper supplies that entry via `AdminEntry::for_testing`).

---

## `/d` — five demo users + role-select UI + demo banner

**Commit:** `907d435` — *5 demo users + role-select UI + demo banner*

### What landed

When `RUSTIO_DEMO_MODE=1`, `bootstrap_demo_users` creates exactly
five users, one per role:

| Email | Role | Password | `is_demo` | `demo_label` |
|---|---|---|---|---|
| `user@<domain>` | User | `user` | TRUE | "User (no admin access)" |
| `staff@<domain>` | Staff | `staff` | TRUE | "Staff (per-model perms)" |
| `supervisor@<domain>` | Supervisor | `supervisor` | TRUE | "Supervisor (view + edit)" |
| `administrator@<domain>` | Administrator | `administrator` | TRUE | "Administrator (full coverage)" |
| `developer@<domain>` | Developer | `developer` | TRUE | "Developer (diagnostics)" |

Passwords are **literally the role slug** so you can paste-test from
memory. `is_demo = TRUE` and a populated `demo_label` flag the row.

### Demo banner

Every page rendered while logged into a demo session gets a
red-on-yellow strip below the header:

> ⚠ DEMO USER (Administrator (full coverage)) — these credentials are
> seeded for evaluation only. Disable `RUSTIO_DEMO_MODE` in production.

The banner reads `is_demo` + `demo_label` from `Identity` (which
loads from `rustio_users` on session lookup). `BaseContext` carries
`is_demo_session: bool` and `demo_label: Option<String>` to the
template. Real users (where `is_demo = FALSE`) get no banner —
the `{% if is_demo_session %}` block is skipped entirely.

### Role-select UI on `/admin/users/new` and `/admin/users/:id/edit`

Pre-/d, the role selector was three options (`user`, `staff`,
`admin`). Post-/d, five options matching the new ladder, with
descriptive labels:

```html
<option value="developer"     {% if role == "developer" %}selected{% endif %}>Developer (schema browser + execution logs + SQL console)</option>
<option value="administrator" {% if role == "administrator" %}selected{% endif %}>Administrator (full coverage; bypasses group checks)</option>
<option value="supervisor"    {% if role == "supervisor" %}selected{% endif %}>Supervisor (view + edit; no destructive ops)</option>
<option value="staff"         {% if role == "staff"      %}selected{% endif %}>Staff (admin access; per-model group permissions)</option>
<option value="user"          {% if role == "user"       %}selected{% endif %}>User (no admin access)</option>
```

Order is high-rank-first so the rare-but-elevated picks lead
visually.

### Test parallelism + `std::env::set_var`

The PG-gated tests for demo bootstrap toggle
`RUSTIO_DEMO_MODE`. `tokio::test`s run on a thread pool, so two tests
toggling the same env var stomp each other.

The fix is a process-wide tokio Mutex held across `.await`:

```rust
// auth/mod.rs, #[cfg(test)]
pub(crate) static TEST_ENV_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());
```

`tokio::sync::Mutex` not `std::sync::Mutex` — clippy's
`await_holding_lock` lint forbids the latter.

This pattern recurs anywhere a test mutates process state (env vars,
global registries). Documented in `feedback_pg_create_table_race.md`.

---

## `/e` — guards across every admin handler + developer stubs

**Commit:** `e57e61c` — *apply role/perm guards across all admin handlers + 3 developer stubs*

### The handler audit

Every existing admin route was rewired to use `role_guard` /
`perm_guard` from `/b`. The previous `admin_only_guard` and
`permission_guard` were deleted entirely.

Concrete changes:

| Route | Old guard | New guard |
|---|---|---|
| `/admin` | `admin_only_guard` | `role_guard(Staff)` |
| `/admin/users` | `admin_only_guard` | `role_guard(Administrator)` |
| `/admin/users/:id/edit` | `admin_only_guard` | `role_guard(Administrator)` |
| `/admin/groups/...` | `admin_only_guard` | `role_guard(Administrator)` |
| `/admin/:admin_name` | `permission_guard` | `perm_guard(perm_for(view))` |
| `/admin/:admin_name/new` | `permission_guard` | `perm_guard(perm_for(add))` |
| `/admin/:admin_name/:id/edit` | `permission_guard` | `perm_guard(perm_for(change))` |
| `/admin/:admin_name/:id/delete` | `permission_guard` | `perm_guard(perm_for(delete))` |

### `login_guard` reshaping (Phase 7a/0.5/e)

Pre-/e, `login_guard` did two jobs: session validity AND role floor.
Post-/e, `login_guard` does only the first; the role floor moves to
`role_guard`. This means a non-Staff user who reaches the panel via
URL hits a 403 page (rendered by `role_guard`), not a redirect to
login. The distinction matters: a logged-in user shouldn't be told
"please log in" — they're authenticated, they're just unauthorized.

### Three developer-only stubs

Three new routes for Developer-rank users:

```
GET /admin/__schema__       Schema browser (DB structure)
GET /admin/__logs__         Execution log viewer
GET /admin/__sql_console__  Ad-hoc SQL console
```

They render a `coming_soon.html` template with a "this is part of the
Developer surface — Phase 8 will fill in the body" placeholder.
Routes are real, gated by `role_guard(Developer)` — Administrator
sees a 403, Developer sees the coming-soon page.

### The trailing-slash bug

First registration: `/admin/__schema__/` (trailing slash). The
router's path-normalization strips trailing slashes from the request
path during matching. Result: 404 for Developer, because the route's
`segments` list ended with an empty string from the trailing slash
in the registration. Fix: register without trailing slashes
(`/admin/__schema__`); the router normalizes both forms via the
request side.

### Browser-verified

Staff hits `/admin/posts/1/delete` → `forbidden.html` (no permission).
Developer hits `/admin/__schema__` → `coming_soon.html` (the stub).
Both verified before commit.

---

## `/f` — last-developer guard + user delete + CLI escape hatch

**Commit:** `9aaa23d` — *last-developer guard (UI block + CLI escape) + user delete handler + template registry fix*

This is the largest of the twelve commits. It closes the role-management
loop with a hard UI block against deleting/demoting the last Developer
plus a CLI escape hatch.

### Part A — `would_orphan_developers` helper

```rust
pub async fn would_orphan_developers(
    db: &Db,
    user_id: i64,
    new_role: Option<Role>,
) -> Result<bool>
```

Returns `true` only when **all three** are true:

1. Exactly one active Developer exists in `rustio_users`
2. The target user (`user_id`) IS that Developer
3. The proposed change removes their Developer status (any role
   other than `Developer`, OR deletion modeled as `Some(Role::User)`)

Cheap-out: `Some(Role::Developer)` (no-op identity) returns false
without hitting the DB. Zero-developers DB returns false too — a
fresh DB pre-bootstrap is allowed.

### Part B — guard in `do_user_edit`

The role-edit POST handler calls the helper before applying the
`UPDATE`. On orphan, it re-renders `user_edit.html` with `errors:
vec![...]` and HTTP 400. The template's existing
`{% include "admin/includes/_field_errors.html" %}` block surfaces
the error inline.

The guard also catches deactivation: if `is_active` is being
unchecked on the sole developer, the helper sees a non-Developer
sentinel role (`Role::User`) and the same path fires. Without this,
an admin could uncheck `is_active` to bypass the role-only guard.

### Part C — `show_user_delete` + `do_user_delete`

Two new handlers:

```
GET  /admin/users/:id/delete  →  show_user_delete  (confirmation page)
POST /admin/users/:id/delete  →  do_user_delete    (perform delete)
```

`show_user_delete` renders `user_confirm_delete.html`. The page
shows the cascade summary (group memberships, active sessions,
direct permissions) and disables the "Yes, I'm sure" button when
either:

- `is_self` (admin trying to delete their own session-bound account)
- `is_last_developer` (`would_orphan_developers` returns true)

`do_user_delete` re-checks both guards server-side and returns
`Error::BadRequest` if either fires. Defense in depth: the disabled
button is HTML; a curl bypass doesn't see HTML.

### Part D — CLI escape hatch

```
rustio user role get --email <e>
rustio user role set --email <e> --role <r> [--yes]
```

`set` runs the same `would_orphan_developers` check. If the change
would orphan, it:

1. Without `--yes`: prompts on stderr `Type 'I UNDERSTAND' to
   continue, anything else to abort:`. The phrase is long and
   unique — paste-prevention against accidental scripted demotion.
2. With `--yes`: emits a stderr warning, skips the prompt.

The CLI is the operator-only break-glass for the UI's
strict-no-orphan rule. Pattern: promote a backup developer first,
then demote the original.

### Part E — UI hint banner

`user_edit.html` gets an amber `.warningnote` block when the target
is the sole active developer:

> Heads up: backup@example.com is the last active developer.
> Demoting their role or unchecking Active will be refused. To swap
> who holds the developer role, promote a backup first via
> `rustio user role set --email someone@example.com --role developer`.

Read before you save → fewer 400-with-error round trips.

### Tests

6 PG-gated tests for `would_orphan_developers` covering: sole-dev
demote, identity update, two devs, inactive devs don't count,
non-dev target, zero devs. Each test isolates the dev pool by
flipping `is_active` on unrelated devs for the test duration.

### The template registry gap (`f-fix1`)

Browser smoke after the commit: `GET /admin/users/122/delete` →
500, "template admin/user_confirm_delete.html not found". Same gap
shape as `coming_soon.html` in `/e`: file existed on disk, but the
`EMBEDDED_TEMPLATES` const in `templates.rs` didn't list it.
`Templates::new(None)` (the production single-binary path) returns
"not found" because the disk loader is a dev convenience.

Fixed in the same commit (combined-commit decision documented
elsewhere): registry line + 2 sandbox render tests for the template
landed alongside the source. Memory file
`feedback_template_registry.md` captures the lesson.

---

## `/f-fix2` — disabled-button CSS + acceptance suite

**Commit:** `dfac6b5` — *visually disable .deletelink-button when [disabled]*

### What was wrong

Browser smoke after `/f`: the "YES, I'M SURE" button on the
last-developer confirm page rendered with the HTML `disabled`
attribute (verified via curl), so a click was correctly refused by
the browser. **But it looked identical to an enabled button**:
red, full opacity, no cursor change. An operator looking at the
page concluded "still clickable, guard isn't working".

The four "real" defense-in-depth layers (template conditional,
context flag, show handler, submit handler) were all correct. The
fifth, undeclared layer — the visual style — was missing.

### The fix

```css
.deletelink-button:disabled,
.deletelink-button[disabled] {
    background: var(--default-button-bg);
    opacity: 0.55;
    cursor: not-allowed;
}
```

That's the entire fix.

### The acceptance suite

This is what `/f-fix2`'s commit actually delivers: a
**curl-based, no-human-needed acceptance suite** for all six /f
scenarios. Every test asserts both **client-side rendering**
(disabled in HTML, banner visible) AND **server-side enforcement**
(POST refused with 400, DB row unchanged). The full table:

| # | Scenario | Client | Server |
|---|---|---|---|
| 1 | CLI create developer | PASS | PASS |
| 2 | UI demote dev when 2+ devs | PASS | PASS |
| 3 | UI demote sole developer | PASS | PASS |
| 4 | CLI role set sole-dev with `I UNDERSTAND` | PASS | PASS |
| 5 | UI delete sole developer | PASS | PASS |
| 6 | UI self-delete | PASS | PASS |

Test 4 verified that piping `"I UNDERSTAND\n"` to stdin satisfies
the prompt — the CLI doesn't need a TTY for the confirmation. Tests
5 + 6 run a curl POST after the GET to prove the server-side guard,
not just the disabled HTML.

### Why a separate commit (the discussion)

`/f` had not yet been pushed when `/f-fix2` was written. The choice
was between (a) one combined `phase 7a/0.5/f` commit, (b) two
commits. The user picked (a) earlier (the combined `/f` covers the
template-registry fix internally), then (b) for the CSS + acceptance
suite, because the visual gap was a genuine downstream finding worth
preserving in the log.

The lesson — that defense-in-depth tests must cover the visual
layer — is captured in `feedback_template_registry.md` alongside the
template-registry triple.

---

## `/h` — user profile view + row-clickable navigation

**Commit:** `7a6ad32` — *user profile view page + row-clickable users list navigation*

### The shift

Before /h: clicking a user row went to `/admin/users/:id/edit`. The
edit form was the only landing surface.

After /h:

```
/admin/users
    └── click any cell → /admin/users/:id/   (read-only profile)
                              ├── ← Back to users
                              ├── Edit       (or disabled if guarded)
                              └── Delete     (or disabled if guarded)
```

The view is the navigation hub. Edit and Delete pages are dedicated
single-purpose surfaces; they don't have to render profile metadata.

### Part A — `show_user_view`

A new handler:

```rust
pub(crate) async fn show_user_view(
    ctx: &AuthAdminCtx,
    identity: Identity,
    user_id: i64,
    csrf: String,
) -> Result<Response>
```

Reuses `would_orphan_developers` (from `/f`) + `identity.user_id ==
target.user_id` to compute `is_self` and `is_last_developer`.
Derives `can_delete = !is_self && !is_last_developer`. The template
gets booleans, not role logic.

Three queries in the handler:

1. The user row (id, email, role, is_active, is_demo, demo_label,
   created_at, updated_at).
2. Group memberships JOIN `rustio_groups` for display names +
   descriptions.
3. **Direct** permission grants (NOT via groups) — the rare per-user
   override that admins should be able to spot.

Direct permissions are explicitly NOT joined through groups. The
template renders them under their own fieldset with copy explaining
why direct grants exist and that group membership is preferred.

### Part B — route registration order

```rust
// rustio-core/src/admin/routes.rs — registered AFTER /admin/users/new
// AND after /admin/users/:id/{edit,delete} so the wildcard doesn't
// shadow the literal "new" segment or the deeper paths.
let router = router.get("/admin/users/:id", move |req| { ... });
```

The router matches in insertion order, first match wins. `:id` is a
wildcard — it would happily match `"new"` if registered earlier,
turning `/admin/users/new` into a request to view user #NaN.

The smoke matrix specifically asserts `/admin/users/new` still
resolves correctly.

### Part C — `user_view.html` + registry

Template registered in `EMBEDDED_TEMPLATES` alongside its sibling
templates. 5 sandbox render tests landed in the same commit
(file + registry + render-test triple per the `feedback_template_registry.md`
lesson).

### Part D — row-clickable users list

`users_list.html` now wraps each `<td>` in an `<a class="row-link"
href="/admin/users/:id/">`. CSS gives the anchor `display: block` and
zeros the cell padding so clicks at the cell edge land on the link.
First-cell anchor is bolded (`td:first-child .row-link {
font-weight: 600 }`) so the email reads as the row anchor.

### Accessibility trade-off (documented, not blocking)

Wrapping every cell in an anchor produces 4 links per row. Screen
readers announce all 4. For an internal admin panel where the
audience is mostly sighted developers, this is acceptable. The
JS-free remediation path — single anchor in the email cell + JS
mousedown delegation on `<tr>` — is deferred until an audit flags
it.

### Tests

5 sandbox render tests for `user_view.html` (with-groups, empty,
is_self, is_last_developer, real-user-omits-demo) + 1 sandbox render
test for `users_list.html` row-clickable invariant (4 anchors per row,
none pointing at /edit). Total: 6 new sandbox tests, 319 → 325.

20-check curl smoke matrix (5 scenarios + routing safety).

---

## Test surface evolution

| Phase | Sandbox | Ignored | Failed |
|---:|---:|---:|---:|
| `7a/0` (baseline) | 291 | 21 | 0 |
| `7a/0.5/a` | 295 | 21 | 0 |
| `7a/0.5/sec1` | 299 | 23 | 0 |
| `7a/0.5/sec2` | 300 | 24 | 0 |
| `7a/0.5/sec3` | 301 | 25 | 0 |
| `7a/0.5/sec4` | 302 | 26 | 0 |
| `7a/0.5/b` | 314 | 26 | 0 |
| `7a/0.5/c` | 315 | 30 | 0 |
| `7a/0.5/d` | 316 | 35 | 0 |
| `7a/0.5/e` | 317 | 35 | 0 |
| `7a/0.5/f` | 319 | 41 | 0 |
| `7a/0.5/f-fix2` | 319 | 41 | 0 |
| `7a/0.5/h` (final) | **325** | **41** | **0** |

Total: +34 sandbox tests, +20 PG-gated (ignored) tests across the
twelve commits. Every commit kept the suite green; clippy
`-D warnings` ran cleanly at every step.

The 41 ignored tests are guarded by `RUSTIO_TEST_DB=1` + a running
postgres. They cover the parts of the auth + permissions surface
that can't meaningfully be tested without a real DB:
`would_orphan_developers` (6), permission cache invalidation (4),
demo bootstrap (5), group cascade (3), and the rest distributed
across the auth modules.

---

## Database schema changes

`/0.5` makes three concrete schema mutations to `rustio_users`,
all inside `migrate_user_schema` (idempotent, runs every boot):

1. `UPDATE rustio_users SET role = 'administrator' WHERE role = 'admin'`
   — rename Phase 6b's `'admin'` value to the new
   `'administrator'`. Runs **before** the `CHECK` constraint so it
   doesn't violate the new whitelist.

2. `ADD COLUMN IF NOT EXISTS is_demo BOOLEAN NOT NULL DEFAULT FALSE`
   — flags rows seeded by `bootstrap_demo_users`. Default `FALSE`
   so existing rows stay non-demo.

3. `ADD COLUMN IF NOT EXISTS demo_label TEXT` — human-readable
   description shown in the demo banner. Nullable.

4. `ALTER TABLE … ADD CONSTRAINT rustio_users_role_check CHECK (role
   IN ('user','staff','supervisor','administrator','developer'))`
   — guarded by `pg_constraint` lookup so re-runs are no-ops.

5. `CREATE INDEX IF NOT EXISTS rustio_users_role_idx ON
   rustio_users(role)` — speeds up `would_orphan_developers`.

6. `CREATE INDEX IF NOT EXISTS rustio_users_is_demo_idx ON
   rustio_users(is_demo) WHERE is_demo = TRUE` — partial index;
   small storage cost, fast `WHERE is_demo = TRUE` filtering for
   demo-bootstrap idempotency checks.

No new tables. The cascade FKs on `rustio_user_groups`,
`rustio_user_permissions`, and `rustio_sessions` (all `ON DELETE
CASCADE`) carry the user-delete cleanup with no application-side
DELETEs. The application is still responsible for cache
invalidation (the "M2M cascade does the DB job, application does
the cache job" pattern).

---

## Architectural decisions

### 1. Role ladder vs. role bag

Roles are linearly ranked, not a bag of orthogonal capabilities.
A user is **one** role. `Role::Developer.includes(Role::Staff)` is
true because Developer is "Staff and more", not because Developer
"has Staff capability".

Why: the permission system already covers fine-grained capability.
Adding role-as-capability would create two parallel grammars for the
same concept. The ladder is a simple "minimum rank" floor —
`role_guard(Role::Administrator)` says "Administrator or Developer
may proceed", nothing more.

This forces a clean separation: **role** answers "which surface can
this user reach"; **permission** answers "which row can this user
mutate".

### 2. Bypass at Administrator, not Developer

`bypasses_group_checks()` returns true for Administrator AND
Developer. Both bypass. Why both? Because:

- Administrator is the role for "I run this product". They should
  not have to be added to every group.
- Developer is Administrator + diagnostic surfaces (`/admin/__schema__`
  etc.). Demoting bypass to Administrator-only would create the
  weird state "Developer can browse the schema but can't view
  posts".

Bypass is a coarse policy choice. The fine-grained permission system
is still in play for Staff and Supervisor.

### 3. Lazy permission attachment over eager bootstrap

Default groups are created with no permissions. Permissions are
attached lazily — at the moment they are registered (via
`#[derive(RustioAdmin)]` + `seed_permissions`). This decouples
group existence from permission existence and makes the bootstrap
path tolerant of any registration order.

Tradeoff: a group's permission set isn't visible until at least one
admin entry has been registered. For a fresh DB this is fine; for
admin tools that introspect groups, it means `lazy_attach_permissions`
must be called before introspection.

### 4. Demo mode as an env flag, not a feature flag

`RUSTIO_DEMO_MODE=1` is read at boot in `bootstrap_demo_users` and
nowhere else. There's no `cfg!(demo_mode)` and no compile-time
switch. The same binary serves demo and prod; the env flag is the
sole runtime control.

Why: simpler to reason about. A test toggling the flag tests the
real code path. A demo deployment just sets the env var; no rebuild.

### 5. Guards return `Guard`, not `Identity`

```rust
pub enum Guard {
    Allow(Identity),
    Redirect(Response),  // any non-Allow response, including 403
}
```

The handler pattern is:

```rust
match role_guard(&ctx, &req, Role::Administrator).await? {
    Guard::Redirect(r) => Ok(r),       // pass-through
    Guard::Allow(ident) => handler_body(...).await,
}
```

This lets the guard render a proper 403 HTML page itself, not just
return `Err(Error::Forbidden)`. The error path goes through the
same render pipeline as the success path — same chrome, same
sidebar, same demo banner. A user who hits a 403 doesn't get a
"naked" router error page.

### 6. View-first navigation for users only

Posts and Groups stayed edit-first in /h. The view-first treatment
costs roughly:

- 1 new template (~85 lines)
- 1 new handler (~80 lines)
- 1 new route
- ~50 lines of CSS
- ~6 new tests

…for every model. Multiplying that by all admin entries would
double the admin's surface area without much benefit for pure CRUD
models. Users are the model where read-only profile metadata
(roles, group memberships, direct permissions, demo flag, timeline)
genuinely benefits from a dedicated surface.

Phase 7a/1's tolkhuset crate may extend view-first to its own
domain models if their read profile is similarly rich.

---

## Lessons learned (memory references)

The session captured three durable lessons in the auto-memory
system. Each lives in
`~/.claude/projects/-Users-mansour-Documents-rustio/memory/`:

### `feedback_phase_reports.md`
**Rule:** PHASE{N}.md must land in its own phase's final commit,
staged by name (not via `git add -A`). Past incident: Phase 5a's
`PHASE4.md` got swept into commit `7457bf3` by `git add -A`,
required follow-up `7973494` to reattach.

### `feedback_pg_create_table_race.md`
**Rule:** Parallel tests sharing a named table must gate init with
`tokio::sync::OnceCell`. Past incident: parallel `tokio::test`s
toggling `RUSTIO_DEMO_MODE` stomped each other's env state. Fixed
with the `TEST_ENV_LOCK: tokio::sync::Mutex<()>` pattern in
`auth/mod.rs`.

### `feedback_template_registry.md`
**Rule:** Adding a new admin template requires (a) the file, (b) a
line in `EMBEDDED_TEMPLATES`, AND (c) a sandbox render test —
treated as a single edit unit. Past incident: `coming_soon.html`
in `/e` (caught before commit) and `user_confirm_delete.html` in
`/f` (caught only by browser smoke, after the commit). Extended
post-`/f-fix2` with the related lesson: defense-in-depth must cover
the visual layer too — a `:disabled` button with no `:disabled` CSS
rule still LOOKS clickable to the operator.

These three rules are the closest the project has to a style guide.
Future Claude sessions reading the memory will pick them up.

---

## Open security items + future work

The Step 1 audit identified several items that are **explicitly out
of scope** for /0.5 and remain open:

1. **Per-row authorization.** The current model is "permission to
   change a Post" not "permission to change Post #42". For the
   tolkhuset domain (interpreters editing their own bookings), this
   distinction will matter. Phase 7a/1+ territory.

2. **Audit logging of destructive actions.** The `do_user_delete` /
   `do_group_delete` paths don't write to a `rustio_audit_log` (no
   such table yet). For real production this needs to land before
   the panel can be opened to non-trusted operators.

3. **2FA / WebAuthn.** Sessions are username + password + cookie. No
   second factor. Not blocking 1.0.

4. **Rate-limiting per-user.** The current rate-limiter middleware
   is per-IP. A logged-in user behind a shared IP gets the IP's
   bucket. Per-user buckets need login state plumbed into the rate
   limiter.

5. **CSRF token rotation on login.** A token issued pre-login is
   reused post-login. Standard hardening: rotate on session
   creation. Fits naturally into the `create_session` path.

6. **Direct permission grants UI.** The view page in /h *displays*
   direct permissions but there's no UI to grant or revoke them —
   that's CLI-only (`rustio perm grant-user`). For full coverage,
   the user view should grow an "Add direct permission" surface.

7. **Group-edit cascade preview.** `do_group_edit`'s permission
   rewrite (DELETE-then-INSERT) doesn't show the operator which
   permissions are being revoked. Same shape as `/sec3`'s
   wholesale-delete bug — likely needs the same explicit
   `invalidate_group_cache` call. Not yet audited.

8. **Browser test automation.** The curl-based smoke matrices in
   `/f-fix2` and `/h` cover the rendered HTML but not the
   JavaScript-driven UX. A Playwright suite could close the
   accessibility-trade-off question raised in `/h`. Future phase.

The audit notes for items 1–7 are tracked in the memory system and
will be consolidated into a future `OPEN_SECURITY.md` if they
remain open at the end of Phase 7a.

---

## Verification at end-of-phase

```text
$ cargo test --workspace --lib
test result: ok. 325 passed; 0 failed; 41 ignored; 0 measured

$ cargo clippy --workspace --all-targets
    Finished `dev` profile [unoptimized + debuginfo] target(s)

$ RUSTIO_TEST_DB=1 cargo test -p rustio-core --lib -- --ignored \
    auth::users::tests::orphan \
    auth::users::tests::no_orphan \
    auth::users::tests::inactive_devs \
    auth::users::tests::non_developer \
    auth::users::tests::zero_developers
test result: ok. 6 passed; 0 failed; 0 ignored
```

### `/f` acceptance suite (curl + psql, 6 scenarios)

| # | Scenario | Client | Server |
|---|---|---|---|
| 1 | CLI `user create --role developer` | PASS | PASS |
| 2 | UI demote dev when 2+ devs | PASS | PASS |
| 3 | UI demote sole developer | PASS | PASS |
| 4 | CLI role set sole-dev with `I UNDERSTAND` | PASS | PASS |
| 5 | UI delete sole developer | PASS | PASS |
| 6 | UI self-delete | PASS | PASS |

### `/h` smoke matrix (curl, 5 scenarios + routing safety)

| # | Scenario | Result |
|---|---|---|
| 1 | GET /admin/users → 4 anchors per row, no /edit leaks | PASS (3/3) |
| 2 | GET /admin/users/120/ (self) → Edit anchor + Delete disabled span | PASS (4/4) |
| 3 | GET /admin/users/122/ (sole dev) → Edit + Delete disabled span | PASS (4/4) |
| 4 | GET /admin/users/118/ (staff demo) → both Edit + Delete are anchors | PASS (5/5) |
| 5 | GET /admin/users/120/edit → "← Back to profile" link | PASS (2/2) |
| – | Routing: /admin/users/new still resolves | PASS (2/2) |

20/20 checks pass.

### Final DB state

```
 13 admin@example.com         administrator   active
117 user@rustio.local         user            active
118 staff@rustio.local        staff           active
119 supervisor@rustio.local   supervisor      active
120 administrator@rustio.local administrator  active
121 developer@rustio.local    developer       active
122 backup@example.com        developer       active
```

Two active developers (the demo `developer@rustio.local` and the
operator-created `backup@example.com`) so the next session can
exercise the orphan guard without setup.

---

## Phase 7a/1 readiness

Phase 7a/1 is the tolkhuset crate skeleton — a parallel binary in
the workspace alongside `examples/blog`, with its own domain
models, its own `SiteBranding` (Swedish-language), and its own demo
domain (`tolkhuset.test`).

`/0.5` is the foundation that makes tolkhuset's first commit a
single-file affair:

1. `tolkhuset/src/main.rs` calls `Admin::new().site_branding(SiteBranding {
   site_title: "Tolkhuset administration".into(),
   site_header: "Tolkhuset administration".into(),
   index_title: "Tolkhuset interpreter management".into(),
   footer_copyright: "© 2026 Tolkhuset AB. Powered by RustIO.".into(),
   domain: "tolkhuset.test".into(),
})`.
2. The first model (likely `Translator` or `Booking`) gets
   `#[derive(RustioAdmin)]` and the standard CRUD flow.
3. `RUSTIO_DEMO_MODE=1` produces five `*@tolkhuset.test` demo users
   covering the role ladder, attached to the six default groups.

No more boilerplate is required from the tolkhuset crate to inherit
the full /0.5 authorization machinery. That's what the twelve
commits buy.

---

*Phase 7a/0.5 closed. Working tree clean, blog stopped, two active
developers preserved in the DB. Ready for /1.*
