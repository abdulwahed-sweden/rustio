# Phase 1 — Admin UX stabilization

> Filename note: the `PHASE1.md` slot in this directory belongs to a
> historical phase (the OLD-codebase port). This Phase 1 is a separate
> UX-stabilization track that runs on top of Phase 7a/2 and Phase 0,
> hence the `-ux` suffix.

## 1. Overview

Phase 1 took an admin that was *technically* working (Golden Flow
green at the close of Phase 0) and made it *production-usable* for a
fresh operator. Three sub-phases — `1/a`, `1/b`, `1/c` — each shipped
as one focused commit. The aim was clarity and obviousness, not
features: framework-managed fields stop appearing in forms, validation
errors read like sentences, required fields look required, and an
empty list page says something different from a filtered-empty list
page. No architecture moved. No API changed.

## 2. Sub-phases breakdown

### Phase 1/a — Auto timestamps

* **Problem.** The reference consumer's `Post` had a `created_at:
  DateTime<Utc>` field. The auto-generated form rendered it as a
  required `<input type="datetime-local">`. A first-time user filling
  in title / body / author and clicking Save got
  `created_at is not a valid date`. Worse, the column already had
  `DEFAULT NOW()` in the migration, so the requirement was redundant
  with the database's own default.
* **Solution.** The macro already had a `FieldKind::DateTimeAuto` arm
  that marked fields non-editable (`form_ctx` filters those out) and
  defaulted them to `Utc::now()` inside `from_form`. The promotion
  was unreachable — `classify_type` returned `DateTime` for every
  `DateTime<Utc>` and nothing flipped it. Phase 1/a wired the missing
  trigger by name: a single helper that promotes `DateTime →
  DateTimeAuto` when the field is named `created_at` or `updated_at`.
* **Result.** The field disappears from both new and edit forms. The
  insert path still sends a non-null value — sourced from the server
  clock instead of the user's input. Identical row shape, identical
  column nullability, identical `INSERT_COLUMNS`.
* **Why no DB migration was needed.** The example consumer's column
  already had `DEFAULT NOW()`. The macro fills the value before the
  insert anyway, so even consumers without a DB-level default still
  get a non-null write. The change is form-side only.

Commit: `f7500b1 phase 1/a: auto-promote created_at / updated_at to DateTimeAuto`.

### Phase 1/b — Form UX polish

Six surgical changes, all under the existing visual system. No new
CSS framework, no new design tokens.

* **Humanised labels.** `FormField.label` now sources from
  `intelligence::field_ui_metadata().label`, so `created_at` reads as
  `Created At`, `published` as `Published`, `body` as `Body`. The
  intelligence layer already owned a Title-case humaniser; the form
  context just hadn't been wired to it.
* **Required markers.** New `FormField.required: bool` computed from
  `FieldType::nullable()` with a Bool exception (checkboxes always
  submit a value). The form template renders a small rust-coloured
  `*` after the label, plus an `sr-only "(required)"` span for
  screen readers.
* **Human-readable error messages.** The macro precomputes the
  humanised label per field at expansion and inlines it into the
  three error format strings. `body is required` becomes
  `Body is required.` Period-terminated, sentence-cased,
  user-readable.
* **Cancel button.** `form.html`'s submit-row gains a
  btn-ghost `Cancel` link to `/admin/<admin_name>/`. Save /
  Save-and-continue / Save-and-add-another preserved exactly as they
  were per the user's instruction.
* **Input consistency.** `.form-row` now styles `number`, `url`,
  `tel`, `date`, `time`, `datetime-local` alongside the previous
  `text/email/password/search` set. Numeric and datetime inputs no
  longer fall back to browser defaults that broke alignment.
* **Textarea improvements.** `min-h-32 leading-relaxed` on
  textareas, so a one-line draft feels deliberate instead of squat.
  `rows="5"` stays as the visible upper bound.

Commit: `cd32c95 phase 1/b: form UX polish (humanised labels, required marker, cancel)`.

### Phase 1/c — Empty states

* **True empty vs filtered empty.** Before, `list.html` rendered the
  same string — `No <model> match the current filter.` — for both
  "this is a fresh DB" and "I searched and nothing matched". The two
  states want different copy and different actions.
* **CTA behaviour.** When the table is empty AND the search box is
  empty AND no filter group has a `current` selection, the page shows
  the friendly heading `No <model> yet.` with a primary CTA
  `Create your first <singular>`. When the user is *narrowing*, the
  page shows `No results match your search.` with no CTA — nudging
  "create" while someone is filtering would be the wrong instinct.
* **UX improvement.** A fresh operator visiting `/admin/posts/` on a
  new deploy now has an obvious next click instead of having to hunt
  for the `+ Add` button at the top of the toolbar. Same field, same
  context, just better information design.

The branch logic uses `filters|selectattr("current")|list|length` —
minijinja-2's idiom for "any filter group has a `current` value" —
and reads only existing `ListCtx` fields. No new context plumbing.

Commit: `b6fb827 phase 1/c: split true-empty vs filtered-empty list states`.

## 3. Before vs after

| Surface | Before | After |
|---|---|---|
| New-post form | `created_at` rendered as required `<input type="datetime-local">`; submit fails on empty | `created_at` not rendered; macro fills it with `Utc::now()` |
| Form labels | Raw column names: `title`, `body`, `created_at`, `published` | Humanised: `Title`, `Body`, `Created At`, `Published` |
| Required indication | None — required and optional fields visually identical | Small rust-coloured `*` after each required label, `sr-only "(required)"` for screen readers |
| Validation error on empty body | `body is required` | `Body is required.` |
| Submit row buttons | `Save / Save and continue editing / Save and add another` (no escape) | `Save / Save and continue editing / Save and add another / Cancel` |
| Empty list, fresh DB | `No posts match the current filter.` (confusing — there is no filter) | `No posts yet. Get started by creating your first post.` + primary CTA |
| Empty list, search miss | `No posts match the current filter.` | `No results match your search.` |

## 4. Technical impact

**Files touched** (10 across the three sub-phases, all leaf files):

* `rustio-macros/src/lib.rs` — auto-timestamp promotion + humanised
  error labels.
* `rustio-core/src/lib.rs` — `extern crate self as rustio_core` under
  `cfg(test)` so the derive macro can be exercised from inside its
  home crate.
* `rustio-core/src/admin/mod.rs` — wire the new `macro_tests` module.
* `rustio-core/src/admin/macro_tests.rs` — new (Phase 1/a tests).
* `rustio-core/src/admin/render.rs` — `FormField.label: String`,
  `FormField.required: bool`, three new render tests.
* `rustio-core/assets/templates/admin/form.html` — required marker,
  Cancel button.
* `rustio-core/assets/templates/admin/list.html` — empty-state
  branches.
* `rustio-core/assets/css/input.css` — input-type coverage,
  textarea min-height, `.required` style.
* `rustio-core/assets/static/css/admin.css` — regenerated by
  `make css`.

**No schema changes.** The example consumer's `posts` table is
unchanged; no migration was authored or required.

**No public API changes.** Every change is either inside the macro's
generated code, inside `pub(crate)` admin types, or inside template
files. The `rustio_core` re-exports at the crate root are identical.

**No breaking changes for downstream consumers.** A consumer with a
`created_at: DateTime<Utc>` field gets the new behaviour automatically
(field hidden, value defaulted) — the insert path still receives a
value of the same type. A consumer that previously *relied* on the
form requiring a `created_at` value would notice the change, but no
in-tree consumer did and the new behaviour is strictly more correct.

## 5. Validation

| Surface | Pre-Phase-1 (Phase 0) | Post-Phase-1 |
|---|---|---|
| `cargo test --workspace --lib` | 329 / 0 fail / 41 ignored | **335 / 0 fail / 41 ignored** (+6 focused tests across a/b/c) |
| `cargo clippy --workspace --all-targets` | clean | clean |
| `make css-check` | clean | clean |
| Golden Flow CRUD on `/admin/posts/` | green | green |
| PG-gated suite | 37 pass / 4 pre-existing `entry_builder`-blocked failures | unchanged (no new regressions) |

New tests:

* `admin::macro_tests::auto_timestamp_fields_are_not_editable`
* `admin::macro_tests::from_form_accepts_submission_without_auto_timestamps`
* `admin::render::tests::form_renders_required_marker_humanised_label_and_cancel`
* `admin::render::tests::list_true_empty_renders_friendly_cta`
* `admin::render::tests::list_filtered_empty_omits_cta`
* `admin::render::tests::list_filter_only_empty_omits_cta`

The Golden Flow was re-verified live at the end of every sub-phase
against the running blog example — login, list, new (no `created_at`
input), create, edit, update, confirm-delete, delete; rows landed in
`posts` with non-null timestamps; humanised labels rendered; required
asterisks rendered exactly on the non-nullable, non-Bool fields;
filtered-empty branch served the new copy without the CTA.

## 6. Final result

The admin is now **production-usable for a first-time operator**:
forms hide framework-managed fields, mark what's required, and speak
in sentences when something goes wrong; empty list pages distinguish
"nothing here yet" from "your search didn't match" and offer the
right next click for each. The framework's deploy contract, public
API, and database schema are unchanged.
