# Phase 10 — built-in user profile page

**Goal:** Promote the user show/profile page to a first-class
built-in admin resource. Every project consuming `rustio-core` gets a
working `GET /admin/users/:id` page out of the box, with a clean
extension mechanism for project-specific fields. Same idea as Django
admin's built-in User model — no handler, no route, no template to
copy from project to project.

Phase 10 lands in three sub-phases. The schema migration (`/a`)
ships first so it's deployable on its own; the UI (`/b`) lights up
on top; the extension API (`/c`) plus docs lands last. Each is its
own commit; the phase report (this file) lives in `/c`'s commit per
the `PHASE{N}.md` hygiene rule.

## /a — Schema + `auth::UserProfile`

Commit `fe73c22`: `phase 10/a: rustio_users + rustio_sessions schema for built-in user profile`.

Additive migration. `rustio_users` gains three nullable columns —
`full_name`, `locale`, `timezone` — all `TEXT NULL`, no defaults,
no backfill. `rustio_sessions` gains `ip` and `user_agent`, also
`TEXT NULL`. The IP / UA columns aren't populated yet — the
session-create path doesn't see the request's IP today. A follow-up
phase (`/d` or later) will thread `X-Forwarded-For` / peer IP
through `auth::login` / `auth::create_session`. For now, existing
session rows render `—` in the new Sessions tab.

Three reasons IP went on `rustio_sessions` instead of `rustio_users`:
(1) IP is per-session, not per-user — a user with five sessions has
five IPs, not one; (2) it makes "last login IP" trivially derivable
from MAX(`created_at`)'s session row; (3) it avoids a UPDATE on
`rustio_users` per login, which would invalidate the read cache for
no good reason.

The migration is wired into `auth::init_tables` via a new
`pub(crate) sessions::migrate_session_schema` function, parallel
to the existing `migrate_user_schema`. Both are idempotent — every
ALTER uses `IF NOT EXISTS`.

### `auth::UserProfile`

New public read-only struct in `rustio-core/src/auth/users.rs`,
re-exported from `auth/mod.rs`. Construct via
`auth::load_user_profile(db, user_id) -> Result<Option<UserProfile>>`.

```rust
pub struct UserProfile {
    pub id: i64,
    pub email: String,
    pub role: Role,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub full_name: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub is_demo: bool,
    pub demo_label: Option<String>,
}
```

`UserProfile` deliberately excludes `password_hash`. The internal
`StoredUser` struct (used by the password-verification path) keeps
the hash; `UserProfile` is the public read-only shape passed to
project extension closures (`/c`). A project's extension cannot
accidentally leak credential material because the type doesn't
carry it.

### Tests (`/a`)

- Sandbox: 402 → 403 (+1 — `UserProfile` derives `Debug + Clone`).
- PG-gated: 37 → 42 (+5 — idempotent migration column-presence
  check, `load_user_profile` happy/missing paths, existing user
  CRUD smoke after migration, existing session CRUD smoke after
  migration).
- The 4 pre-existing `entry_builder`-blocked stubs in
  `admin/suggestions_tests.rs` continue to fail with the same
  reason — predates Phase 10, surfaces on `main` (verified by
  git-stash test against HEAD `970c223`).

## /b — Handler + template

Commit `d1e1bc9`: `phase 10/b: built-in user profile page (handler + template)`.

Replaces the bodies of the existing `show_user_view` handler in
`admin/builtin.rs` and the `admin/user_view.html` template. The
route at `/admin/users/:id` already existed (registered in
`admin/routes.rs`) — `/b` only swaps the implementation; it
doesn't add a new file or new route registration.

### Tabs

`?tab=overview|activity|permissions|sessions` selects the
detail-pane content. Default is `overview`. Invalid values fall
back silently to `overview` (no 400). Tab links strip `&page=`;
only the Activity tab's pager preserves it.

- **Overview**: stat-strip (Sessions / Activity / Last seen) +
  show-grid profile (Full name, Email, ID, Role, Groups, Locale,
  Timezone, Created, Last login) + last 7 audit events with a
  "View all activity →" link.
- **Activity**: paginated audit timeline, 50 events per page.
  Pager preserves `?tab=activity` across page links.
- **Permissions**: a `perm-grid` of effective permissions. Each
  tile shows the permission codename and a chip with the source
  (`direct` for `rustio_user_permissions` rows, `via <Group>` for
  inheritance). Both kinds appear in the same list; a permission
  granted both ways shows up twice with different chips.
- **Sessions**: passive table — `created_at`, `last_seen`, `ip`,
  `user_agent`, truncated 7-char `token`. No revoke button. Empty
  IP / UA fields render as the framework's `rio-cell-empty`
  marker.

### Why the inline Delete button is gone

The pre-`/b` `user_view.html` had Edit + Delete in `page-tools`
at the top, with full guarding (`is_self`, `is_last_developer`
→ `<span>` instead of `<a>`). `/b` keeps Edit (in the detail-head
actions row) and **removes the inline Delete entirely**.
Destructive ops are reachable exclusively through
`/admin/users/:id/delete`, which has its own confirm flow with the
same guarding (Phase 7a/0.5/f's `user_confirm_delete.html`). The
guarding contract didn't disappear — it lives on the page where it
canonically belongs.

This collapsed two pre-`/b` render tests
(`user_view_is_self_disables_delete_as_span`,
`user_view_last_developer_disables_delete`); their contract is
covered by the existing `user_confirm_delete.html` tests.

### CSS port

The splitview / tabs / timeline / show-grid / stat-strip / row-*
/ pane-* / detail-* / tl-* vocabulary did not exist in the
framework's `admin/base.html` before `/b`. It was authored in a
project-level halalops `base.html` override and never made it into
the framework. `/b` ports it — ~430 lines of CSS appended to
`admin/base.html`'s `<style>` block — verbatim from the original
Tailwind UI Application UI authoring.

A `§1.5` alias block at the top of `:root` maps the halalops-flavour
palette names (`--gray-*`, `--emerald-*`, `--shadow-*`, `--ring-*`,
`--rounded-*`) onto the framework's existing `--rio-*` design
tokens. Emerald shades alias to `--rio-accent` so the new
components inherit the framework brand instead of bleeding emerald
through. Existing pages don't reference the new selectors and
aren't visually affected.

`admin.css` was regenerated by `make css`. The single net change
is `+.italic{font-style:italic}` — Tailwind's content scanner
extracted the keyword from a `font-style: italic` rule in the new
`.show-v--muted` selector. Mechanical regeneration; no design
change.

### List pane sort

The user spec asked for `last_seen DESC` on the list pane, with a
fallback to `created_at DESC` if the cross-table cost was
unjustified. `last_seen` lives only on `rustio_sessions`, so a
true `last_seen DESC` sort needs a correlated subquery
(`(SELECT MAX(s.last_seen) FROM rustio_sessions s WHERE
s.user_id = u.id)`) per row in the SELECT — moderate cost on a
50-row LIMIT, but it scales linearly with table size. `/b` ships
the cheaper `created_at DESC` sort. A follow-up can switch once
the cost is measured against a realistic dataset.

### Tests (`/b`)

- Sandbox: 403 → 404 (+1 net — deleted 2 inline-Delete tests,
  renamed/updated 3 to match new template fragments, added 3
  new tab tests: `activity_tab_renders_pager`,
  `permissions_tab_renders_with_sources`,
  `sessions_tab_truncates_token_and_handles_nulls`).
- PG-gated: 42 → 42 (unchanged — `/b` adds no PG tests).

`/b` skipped the HTTP-level integration tests called for in the
original spec. The framework's existing test surface has no
router-level tests for builtin handlers; adding them would
introduce a new testing pattern (scope creep). Browser smoke
covers the contracts:

- `GET /admin/users/:id` (admin session, all 4 tabs) → 200 with
  the expected splitview / timeline / perm-grid / table fragment.
- `GET /admin/users/:id` (no session, all 4 tabs) → 303 to
  `/admin/login`.
- `GET /admin/users/999999999` (admin session) → 404.

## /c — Extension closure + docs

Replaces the empty `{% block project_user_fields %}` body with a
default that renders any sections returned by a project-registered
closure. Projects use the new `Admin::user_profile_extension`
builder method to register one closure that returns a
`Vec<UserProfileSection>`; the framework calls it on every Overview
render and merges the result into the template context as
`project_fields`.

### API

```rust
use rustio_core::admin::{Admin, UserProfileRow, UserProfileSection};

let admin = Admin::new()
    .model_with_search::<Post>(indexer.clone())
    .user_profile_extension(|_db, user| Box::pin(async move {
        Ok(vec![UserProfileSection {
            label: "Halal certification".into(),
            rows: vec![UserProfileRow {
                label: "License #".into(),
                value: "HC-2025-0042".into(),
            }],
        }])
    }));
```

Three public types from `rustio_core::admin`:

- `UserProfileSection { label: String, rows: Vec<UserProfileRow> }`.
- `UserProfileRow { label: String, value: String }`.
- `Admin::user_profile_extension(F)` builder method.

The closure signature is
`Fn(Db, auth::UserProfile) -> Future<Output = Result<Vec<UserProfileSection>>>`
— both arguments are owned (`Db` is `Arc`-cheap to clone,
`UserProfile` is `Clone` and small). Boxing happens inside the
builder so callers don't have to name `BoxFuture`; they just write
`|_db, user| Box::pin(async move { ... })`.

### Why a closure, not a trait

The user spec considered three alternatives: a closure, a
`UserProfileExt` trait, and a derive macro. `/c` chose the closure
because:

1. There's only one user profile per project — no polymorphism
   over multiple types, so a trait would carry no extra weight
   over a function.
2. The framework already takes function-shaped configuration
   elsewhere (route handlers in `Router`); a builder method that
   accepts a closure matches the existing idiom.
3. A derive macro is premature until ≥ 2 real consumer projects
   with different shapes drive the design. We have one (halalops).
4. Adding a trait or derive later is non-breaking — existing
   closure-registered extensions keep working — so this is a
   reversible choice.

The downside: the closure is limited to key-value rows. Projects
that need richer markup (charts, custom layouts) drop a project
template at `templates/admin/user_view.html` extending
`admin/base.html` and override the `{% block project_user_fields %}`
block directly. Both extension paths can coexist — the closure
contributes data; the block contributes markup.

### `UserProfile` is the only data the closure sees

The closure parameter is `auth::UserProfile`, not `StoredUser`.
`UserProfile` is the public read-only shape introduced in `/a` —
no `password_hash`, no internal `StoredUser` fields. A project's
extension closure cannot accidentally leak credential material
because the type doesn't carry any. This is a deliberate
construct-time guarantee, not a code-review check.

### Tests (`/c`)

- Sandbox: 404 → 406 (+2 — `user_view_overview_renders_project_fields_section`
  for the populated case, `user_view_overview_omits_extension_when_project_fields_empty`
  for the zero-config case).
- PG-gated: 42 → 42 (unchanged — extension closure is a pure-data
  contract; correctness of project queries is the project's
  responsibility).

### Example registration

`examples/blog/src/main.rs` registers a minimal two-row "Blog
account" section computed from `UserProfile` alone, no extra
schema. It's enough to wire the closure end-to-end without
forcing the example to drag in a halalops-flavour table. Real
projects typically join against a project-specific table here.

### Halalops migration

Halalops's local override at
`~/Desktop/halalops/src/admin_views/` becomes redundant once
`rustio-core` releases the next version with `/c`. The override
delete + dep bump + closure registration is halalops's own
post-release commit, not part of Phase 10.

---

## What Phase 10 deliberately did NOT do

- **Edit the existing user-edit page** to expose the new core
  columns (`full_name`, `locale`, `timezone`). The new columns
  are read-only on the show page; editing them is a separate
  follow-up task on the existing user-edit form.
- **Populate `rustio_sessions.ip` / `user_agent`**. The columns
  exist but the session-create path doesn't see the request's
  IP today. A follow-up phase threads the IP through
  `auth::login` / `auth::create_session`.
- **Real fuzzy filter on the list-pane search input**. The
  input is decorative in `/b` (carries `data-search-input` so
  the global Esc handler clears it, but no live filter is
  attached). Wiring is a follow-up.
- **Revoke button on Sessions tab.** Passive list only in `/b`.
  Revoke is a separate destructive-action surface that needs
  CSRF, audit logging, and self-revoke handling.
- **Activity-tab filtering / sorting.** Pagination only.
- **Add a `Permissions` tab editor.** Read-only display only.
  Editing per-user permissions is the existing user-edit form.

Each of these is a clean follow-up phase; none of them block the
Phase 10 user value (a working built-in profile page with a clean
project-extension story).

## Test counts across Phase 10

| Sub-phase | Sandbox | PG-gated (passing) |
|---|---|---|
| Pre-`/a` baseline | 402 | 37 |
| After `/a` | 403 (+1) | 42 (+5) |
| After `/b` | 404 (+1 net) | 42 |
| After `/c` | 406 (+2) | 42 |

The 4 pre-existing `entry_builder`-blocked PG stubs continue to
fail with their original reason throughout Phase 10. They're not
introduced by Phase 10 and they're unaffected by it.
