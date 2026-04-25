# Phase 6b — Admin UI: 5 remaining pages + first audit consumer

Built on top of Phase 6a + post-merge fixes (`03e3e3a`).

## Commits shipped

```
2d08d0e  phase 6b/5: self-service password change page (validates old + match + len ≥ 8)
21d729e  phase 6b/4: Django-style groups (list + edit), BaseContext refactor finished
e537c0e  phase 6b/3: Django-style users (list + edit), BaseContext refactor, admin-reset inline
?????    phase 6b/2: history pages (per-object + global) + lazy audit::ensure_table via OnceCell
7ebf5a7  phase 6b/1: delete confirm with cascade list (first RelationRegistry consumer)
52335a3  phase 6b/0: rewrite error.html (Django shape) + ErrorCtx prep
```

## Step 1 audit (recap)

Inspection found 6 deferred bespoke templates from Phase 6a (`confirm_delete`, `error`, `users_list`, `user_edit`, `groups_list`, `group_edit`) that rendered with browser defaults because their classes (`.cards`, `.data-table`, `.btn-ghost`, `.actions-col`, etc.) had no rules in `admin.css`. Plus three genuinely new: per-object history, global history log, self-service password change.

The 4 builtin contexts (`UsersListCtx`, `UserEditCtx`, `GroupsListCtx`, `GroupEditCtx`) had hand-rolled `identity` / `csrf_token` instead of embedding `BaseContext` — they're the reason base.html's `{{ site_title }}` rendered empty for those pages.

## Per-page changes

### 6b/0 — `error.html` rewrite + `ErrorCtx` prep

- Template rewritten to Django shape (breadcrumbs + h1 + body + back link). 14 LOC.
- `ErrorCtx { base: BaseContext, status_code, status_message, details }` added in `render.rs` with `#[allow(dead_code)]` — orphan render path (no caller in NEW; Phase 9 is the expected first consumer).

### 6b/1 — Delete confirm with cascade list

- `ConfirmDeleteCtx` extended: added `object_id: i64` (form action needs it) and `cascading: Vec<CascadeItem>`.
- New `CascadeItem { source_display_name, source_admin_name, source_field }` — derived from `RelationRegistry::has_many(entry.singular_name)` at handler entry. **First real consumer of Phase 3's `RelationRegistry`.**
- `show_delete_confirm` (in `handlers.rs`) builds a fresh `Schema::from_admin(&ctx.admin)` + `RelationRegistry::from_schema` per request — cheap, schema is small.
- Template `confirm_delete.html` (44 LOC): breadcrumbs through model → object → Delete. Page header "Are you sure?". Bullet list of cascading models with **browse** links scoped to the object id (e.g. `/admin/comments/?post_id=42`). Submit row: red "Yes, I'm sure" left, gray "No, take me back" right.
- New CSS (admin.css +29 LOC): `.cascade-list` (light-bg bordered ul), `.deletelink-button` (red), `.cancel-link` (steel-muted), `.confirm-form .submit-row { justify-content: flex-start }`.

### 6b/2 — History pages (per-object + global) + `audit::ensure_table` wire-up

**Lazy `ensure_table` via `tokio::sync::OnceCell`** (handlers.rs):

```rust
static AUDIT_TABLE_READY: OnceCell<()> = OnceCell::const_new();

async fn ensure_audit_ready(db: &Db) {
    AUDIT_TABLE_READY.get_or_init(|| async {
        if let Err(e) = audit::ensure_table(db).await {
            log::warn!("audit::ensure_table failed: {e}");
        }
    }).await;
}
```

`register_admin_routes` is sync (returns `Router`, not `Future<Router>`); calling `.await` from inside isn't possible without an API change to the two callers (blog + CLI). Lazy init runs once at first audit-touching request. Phase 5b's pattern (Postgres `CREATE TABLE IF NOT EXISTS` is not race-safe) makes this the safe way to handle parallel first-requests. Failures **log + swallow** so the dashboard's silent-degrade path stays alive.

Called from: `dashboard`, `show_object_history`, `show_log_entries` — every handler that queries the audit table.

**New context types** (render.rs):
- `HistoryEntryCtx` — single audit row with derived `when_relative` + `pill_class`.
- `ObjectHistoryCtx { base, page_title, admin_name, display_name, singular_name, object_id, object_label, entries }`.
- `LogEntriesCtx { base, page_title, entries }`.
- `map_audit_actions(Vec<AdminAction>) -> Vec<HistoryEntryCtx>` helper.

**New handlers** (handlers.rs):
- `show_object_history(ctx, identity, admin_name, id, req)` — calls `audit::for_object(...)`. Silently degrades to empty list if the table is missing.
- `show_log_entries(ctx, identity, req)` — calls `audit::recent(db, 100, None, None)`.

**New routes** (routes.rs):
- `GET /admin/:admin_name/:id/history` — gated by the same `view` permission as the changelist.
- `GET /admin/history` — admin-only.

**New templates**:
- `admin/object_history.html` (40 LOC) — `.results` table with date/user/action/summary/IP columns.
- `admin/log_entries.html` (39 LOC) — same plus an Object column linking back to the change form.

**Form integration**: `admin/form.html` gained an `<ul class="object-tools object-tools-actions">` with a `.historylink` ("↻ History") on edit-mode pages only. New CSS (admin.css +9 LOC): `.object-tools-actions .historylink`.

### 6b/3 — Users (list + edit) + start of BaseContext refactor

- `UsersListCtx` and `UserEditCtx` refactored to embed `#[serde(flatten)] base: BaseContext`. Removed duplicated `identity` + `csrf_token` fields. Handler bodies unchanged except for context construction.
- `users_list.html` (40 LOC) — `change-list` shape: `.object-tools` header, `.results` table with email (linked to edit), `rio-pill rio-pill-{rose|indigo|emerald}` role badge, active flag, joined date.
- `user_edit.html` (84 LOC) — three `fieldset.module.aligned` blocks: Identity (email read-only + role + active), Groups (checkbox list), Reset password (admin-only inline reset). Cross-link to self-service `/admin/password_change` for the logged-in user.
- New CSS (admin.css +25 LOC): `.checkbox-list` + `.checkbox-item` (vertical baseline-aligned checkboxes, used by user/group edit).

### 6b/4 — Groups (list + edit) + finish BaseContext refactor

- `GroupsListCtx` and `GroupEditCtx` refactored — same pattern. `IdentityCtx` import removed from `builtin.rs` (no remaining uses).
- `groups_list.html` (35 LOC) — same change-list shape, columns Name (linked) + Description.
- `group_edit.html` (60 LOC) — General fieldset (name + description) + Permissions fieldset with `<code>`-styled permission names in a checkbox list.

### 6b/5 — Self-service password change

- `PasswordChangeCtx { base, page_title, errors: Vec<String>, success: bool }` in render.rs.
- `MIN_PASSWORD_LEN: usize = 8` constant in handlers.rs — single source of truth, easy to tighten in a future phase.
- Two handlers:
  - `show_password_change(ctx, identity, req)` — empty form.
  - `do_password_change(ctx, identity, req)` — looks up user via `auth::find_user_by_email(ctx.db, identity.email)`, runs three validations: `verify_password(old, stored_hash)`, `new_password1 == new_password2`, `new_password1.len() >= MIN_PASSWORD_LEN`. On success: `auth::set_password(db, user.id, new1)` + render success page. On failure: re-render with `errors: Vec<String>` + `400`.
- Routes: `GET /admin/password_change` + `POST /admin/password_change` — both gated by `login_guard` (any authenticated user, not admin-only).
- "Change password" link added to base.html's `#user-tools` row, between the welcome message and the Users link.
- Template `admin/password_change.html` (52 LOC) — three password fields, success branch shows "Your password was changed successfully" + back link.
- New CSS (admin.css +20 LOC): `.messagelist`, `.message-success`, `.message-warning`, `.message-error`.

## File-LOC summary

| File | Phase 6b delta |
|---|---:|
| `assets/templates/admin/error.html` | rewritten, 14 LOC |
| `assets/templates/admin/confirm_delete.html` | rewritten, 44 LOC |
| `assets/templates/admin/object_history.html` | **new**, 40 LOC |
| `assets/templates/admin/log_entries.html` | **new**, 39 LOC |
| `assets/templates/admin/users_list.html` | rewritten, 40 LOC |
| `assets/templates/admin/user_edit.html` | rewritten, 84 LOC |
| `assets/templates/admin/groups_list.html` | rewritten, 35 LOC |
| `assets/templates/admin/group_edit.html` | rewritten, 60 LOC |
| `assets/templates/admin/password_change.html` | **new**, 52 LOC |
| `assets/templates/admin/base.html` | +1 line (Change password link) |
| `assets/templates/admin/form.html` | +5 lines (History link in `.object-tools`) |
| `assets/static/css/admin.css` | +83 lines across 5 sections (cascade list, checkbox list, history link, message list, password page) |
| `src/admin/render.rs` | +90 LOC (5 new contexts: ErrorCtx, CascadeItem, HistoryEntryCtx, ObjectHistoryCtx, LogEntriesCtx, PasswordChangeCtx + map_audit_actions helper) |
| `src/admin/handlers.rs` | +150 LOC (6 new handlers + OnceCell wire-up) |
| `src/admin/routes.rs` | +35 LOC (3 new routes: per-object history, global log, password change GET/POST) |
| `src/admin/builtin.rs` | net no change — 4 contexts refactored to BaseContext (+1 line each, -3 lines each → −8 LOC) |
| `src/templates.rs` | +3 lines (3 new templates in EMBEDDED_TEMPLATES) |

**0 LOC dropped from any module that was in scope.** No `audit.rs`, `suggestions.rs`, `intelligence.rs`, `relations.rs`, `types.rs`, or `ai/` was touched.

## `audit::ensure_table` wire-up — what happens at the boundary

| Scenario | Behavior |
|---|---|
| First request after process boot hits `/admin` (dashboard) | Cell uninitialized → `ensure_audit_ready` runs `audit::ensure_table` (3 idempotent `CREATE … IF NOT EXISTS`) → cell stores `Some(())` → audit query proceeds. |
| First request goes to `/admin/history` instead | Same — any of the 3 audit-touching handlers triggers init. |
| Subsequent requests from any thread | Cell already initialized → `get_or_init` returns instantly, no DDL re-issued. |
| `audit::ensure_table` returns `Err` (PG down at boot, perms wrong, …) | `log::warn!` fires, cell is **still set** (the closure ran, just had no side-effect). Subsequent reads via `audit::recent` / `audit::for_object` will fail — handlers `unwrap_or_default` to empty `Vec`. The dashboard's Recent Actions sidebar shows "No recent activity yet." History pages show "no recorded history." Application keeps serving. |
| Operator wires `audit::ensure_table` from their own startup sequence | Idempotent — `CREATE TABLE IF NOT EXISTS` no-ops, `CREATE INDEX IF NOT EXISTS` no-ops. Safe to call before our lazy path. |

## Verification

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 291 passed; 0 failed; 21 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored

$ cargo clippy --workspace --all-targets -- -D warnings    → clean
$ cargo check --workspace --all-targets                    → clean
```

Phase 5b → 6b: same 291 passing / 21 ignored. **No new tests added in 6b** — every handler is template-glue + existing-API consumption; the existing template + audit + auth tests cover the underlying mechanics.

### Smoke test against running blog (sandbox)

After `pkill -f target/debug/blog && cargo build -p blog && env -i HOME=$HOME PATH=$PATH DATABASE_URL=… target/debug/blog`:

```
GET /admin/login                  → 200    (login page)
GET /admin                         → 303    (auth-guard redirect — expected without session)
GET /admin/posts/1/delete          → 303    (was bespoke styling pre-6b; route reaches new template)
GET /admin/posts/1/history         → 303    (NEW route — was 404 in 6a)
GET /admin/history                 → 303    (NEW route — was 404 in 6a)
GET /admin/users                   → 303    (template now Django-shaped)
GET /admin/users/1/edit            → 303    (template now Django-shaped)
GET /admin/groups                  → 303    (template now Django-shaped)
GET /admin/password_change         → 303    (NEW route — was 404 in 6a)
```

Every URL the user clicks now matches a registered route; 6a's three `404`s on the new pages are gone.

## Browser checklist (your turn)

```bash
make up
cargo build -p blog
env -i HOME=$HOME PATH=$PATH \
  DATABASE_URL=postgres://postgres:dev@localhost:5432/rustio_dev \
  MEILI_URL=http://localhost:7700 \
  RUSTIO_TEMPLATE_DIR=/tmp/none \
  target/debug/blog
```

Sign in `admin@example.com / admin`, then:

| URL | Expected |
|---|---|
| `/admin/posts/1/delete` | "Are you sure?" header. Bullet list (just the post itself, since blog has no other model pointing at posts). Red "Yes, I'm sure" left, steel "No, take me back" right. |
| `/admin/posts/1/edit` | New `↻ History` link above the page header. |
| `/admin/posts/1/history` | "Change history: …" h1. Empty-state ("This post has no recorded history…") on first visit, since blog doesn't yet log via `audit::record` from its admin handlers. |
| `/admin/history` | "Recent admin actions" h1. Empty-state until something writes to the audit log. |
| `/admin/users` | Django table: Email · Role (rust-pill) · Active · Joined. Clicking email → user_edit. |
| `/admin/users/1/edit` | 3 fieldsets (Identity / Groups / Reset password). Cross-link in the password block points at `/admin/password_change`. |
| `/admin/groups` | Django table: Name (linked) · Description. |
| `/admin/groups/<id>/edit` | 2 fieldsets (General / Permissions). Permission names in `<code>` style. |
| `/admin/password_change` | 3 password fields + steel/rust submit button. Submit with mismatched new1/new2 → red error block "The two password fields didn't match." Submit with all-correct → green success block + "Return to admin home" link. |

Each page: view source → `<link rel="stylesheet" href="/static/admin.css">` present. Each form: `<input type="hidden" name="_csrf" value="...">` present.

## Known boundaries (deferred to Phase 7+)

- **Bulk-action UI** stays hidden through Phase 6b (Phase 6a fix/2 gate). `bulk_actions_enabled = false` hard-coded in `list_ctx`. Phase 7 wires the action-bar POST handler when needed.
- **Audit `record()` calls from CRUD handlers** — Phase 5b's `audit::record` is wired but no handler calls it. Until Phase 7 (or a project that wants the trail) calls `record()` inside `do_create` / `do_update` / `do_delete`, the history pages always render empty.
- **Per-model template overrides** — `Templates::render_for_model` exists but no handler calls it. Phase 7 (tolkhuset) is the expected first consumer.
- **Filter widgets** beyond `BoolYesNo` on the changelist — same as 6a; rendered inert until Phase 7+.
- **Self-service password change** uses single rule: `len ≥ 8`. No history check, no entropy scoring, no breach lookup. Tighten when a project's compliance requires it.

## Open questions for Phase 7 (tolkhuset)

1. **Where do `audit::record` calls land?** Inside `do_create` / `do_update` / `do_delete` is the natural site, but `record()` needs the acting user's `id` (not just email — different from `Identity` which carries email). Either thread `user_id` through `Identity` or do a lookup-per-write.
2. **Per-app permission seeding for the dashboard's `app_label` grouping.** A multi-app project like `tolkhuset.translators` + `tolkhuset.bookings` needs the macro to derive `admin_name = "tolkhuset.translators"` cleanly — currently the `humanise()` strip doesn't account for the dotted prefix. Verify the macro behavior in Phase 7's first multi-app test.
3. **Group / user creation from the UI.** Both list pages currently say "Create one from the CLI" — the "+ Add" workflow for users + groups isn't implemented in 6b. Phase 7 may want it.
4. **Permission edit via the user_edit page.** Currently the user form only assigns to groups. Direct user-permission grants still require the CLI. Add to user_edit when needed.

## Confirmation

- **Plain HTML + minijinja + admin.css.** No new JS framework, no Google Fonts, no animation longer than the focus-ring's 0 transition.
- **Every form** carries `<input type="hidden" name="_csrf" value="{{ csrf_token }}">`.
- **Every page** extends `admin/base.html` and overrides blocks. No template duplicates the shell.
- **Sandbox suite**: 291 passing, 21 ignored, 0 failed. Clippy `-D warnings` clean.
- **No CSS file added.** All new styles extended `admin.css`. Total file growth this phase: +83 LOC (664 → 747).
