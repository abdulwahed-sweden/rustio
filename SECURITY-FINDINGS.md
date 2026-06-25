# Security findings

A running log of security observations found incidentally during feature
work, with their disposition. Items marked **deferred** are tracked for a
separate, dedicated task — not fixed in the PR that recorded them.

| ID | Summary | Status |
|----|---------|--------|
| SF-1 | Templated create/edit/delete forms render no CSRF token | **Resolved** (`fix(admin): render CSRF token in templated forms (SF-1)`) |

---

## SF-1 — Templated admin create/edit/delete forms render no CSRF token

**Status: RESOLVED** — fixed in commit
`fix(admin): render CSRF token in templated forms (SF-1)`. The original
finding is preserved below for the record.

**Resolution:** added the canonical token input
(`{% if csrf_token %}<input type="hidden" name="_csrf" value="{{ csrf_token }}">{% endif %}`)
to the four affected forms — `admin/form.html` (create + edit) and
`admin/list.html`'s three delete-form instances (table / list / cards
layout branches). Template-only: both render contexts already supplied
`csrf_token`, and `require_csrf` / `verify_token` were left unchanged (the
gate was never weakened — the forms simply now feed it a valid token).
`auth/login.html` correctly stays tokenless: `handle_login` does **not**
call `require_csrf` (it bootstraps the token; requiring it would be an
un-loginable chicken-and-egg). Covered by an end-to-end HTTP integration
test (`tests/admin_form_csrf.rs`): create/delete succeed with a valid
token and are rejected (403) with a missing or wrong one.

---

**Found:** during Phase 8 (persist per-model layout default), while locating
the canonical CSRF pattern to mirror.

**Affected templates** (do **not** render a `_csrf` hidden input):
- `rustio-core/assets/templates/admin/form.html` — the create/edit form.
- `rustio-core/assets/templates/admin/list.html` — the per-row **delete**
  form.

**Affected handlers** (all call `require_csrf`, which rejects an empty or
mismatched token via `auth::csrf::verify_token`):
- `admin_model_create_post`
- `admin_model_update_post`
- `admin_model_delete_post`

**Observed behavior:** because the forms post **no** `_csrf` field and
`app.js` performs no token injection, `require_csrf` sees an empty provided
token and returns `Error::Forbidden`. So these templated state-changing
actions appear to be **non-functional (403) rather than unprotected** — the
gate fails closed. Either way it is inconsistent: the forms must carry the
token (as `includes/header.html` logout, `admin/password_change.html`, and
`admin/suggestion_review.html` correctly do) for the actions to work, and
to be the intended defense.

**Working reference pattern** (carries the token, handler verifies):
`includes/header.html` logout form +
`{% if csrf_token %}<input type="hidden" name="_csrf" value="{{ csrf_token }}">{% endif %}`.

**Disposition:** **deferred — separate task.** Out of scope for Phase 8,
which only persists the list layout default. Phase 8's own new
"Set as default" form *does* mirror the working pattern (renders `_csrf`,
handler calls `require_csrf`), so it is correctly protected regardless of
this legacy gap. The broken legacy forms were left unchanged.
