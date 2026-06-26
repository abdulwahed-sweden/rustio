# Deferred work

Signposted follow-ups discovered during feature work, tracked so they
aren't lost. Each names where it was found and why it was deferred.

---

## DW-1 — Render `ViewSpec.filters` as a list filter bar

**Found:** Phase 9c (composition editor — filter toggles).

**State today:** the composition editor persists which fields are
filterable into the saved view — `ViewSpec.filters` + `FieldSpec.filterable`
(validated). But the **live list page does not render those filters as a
filter UI**. `admin/list.html`'s only filter mention is a comment ("Filter
chips and bulk actions land when the Rust side surfaces them …"); in
`layout::list_render`, the `filters: &HashMap` parameter is the *active
filter values from the query string* (applied to the query in
`fetch_users_table_state`), and `ViewSpec.filters` is never read by the
live list path.

**Deferred work:** wire `ViewSpec.filters` into the list page — render a
filter control per filterable field (the right widget per
type/relation, e.g. a dropdown for `status`/FK columns), feeding the
existing `filters` query map so the controls actually filter rows. This is
a rendering feature (parallel to how Phase 6 wired *column* selection but
left *filter* rendering unwired), meaningfully larger than the 9c "filter
toggles," so it is its own phase — **not** built in 9c.

**Disposition:** deferred — its own phase. 9c is complete as the editor
capability (toggle + persist + validate); the list-side filter rendering
is the next related thread.

---

## i18n track — design notes for L4 (decided, not yet built)

The i18n track so far: **L1** (ViewSpec per-language labels + `label`/`label_for`
resolvers, shipped), **L2** (admin list headers render through labels for the
view's `default_language`, shipped), **L3** (edit labels in the composition
editor — next). **L4** is the per-user language preference; these notes capture
decisions already made so they aren't lost.

### L4-A — Active language = a saved per-user preference

A per-user language preference, saved the same way as the Phase-8 layout
default (CSRF + atomic write). At render time it **replaces `default_language`
as the active language**, with precedence:

> user preference → view/project `default_language` → `"en"`.

L2 today passes `&spec.default_language` into `label_for`/`label`; L4 swaps that
single argument for the resolved active language. The resolver and the iron rule
are unchanged — only *which* language code is requested changes.

### L4-B — Language switcher: endonym display names, not ISO codes

The switcher must show each language's **endonym display name** — `"English"`,
`"Svenska"`, `"العربية"` — **never** the ISO code. Storage and all code keys stay
ISO 639-1 (`en` / `sv` / `ar`). This needs a small **language registry** mapping
ISO code → endonym display name (the code is the key; the endonym is presentation
— the same English-key / translated-shell split as the iron rule).

### L4-C — Switcher is ONE reusable component, multiple placements

The switcher is a **single reusable component** (one logic path, no duplicated
blocks) rendered in multiple locations:

- **topbar** — mandatory; the globally-expected spot near the user/logout menu;
- **bottom of the sidebar** — also included;
- **footer** — optional, lower priority for an admin panel.

### L4-D — Enum / stored-value display labels

**Shipped (storage + render):** `ViewSpec.value_labels`
(`source → value → lang → label`) + `value_label_for` resolver + validation,
and the admin list renders status pills and plain string cells through it for
the active language (the pill colour + sorting + stored values stay English —
the iron rule). Parallel to L1 (labels) + L2 (render).

**Editor editing — shipped.** A "Value labels" section in the composition
editor: for each status-shaped field the editor auto-discovers its stored
values (`SELECT DISTINCT`, capped at 50) and offers a label input per value in
the editing language; any already-labelled value (for any field) also appears,
so hand-authored labels stay editable. Persisted via `apply_value_label_edits`
through the same `build_edited_spec → validate → save_view_spec` path, with the
strict exact-editing-language prefill, composes with field labels in one Save,
and prunes on merge-away. Value keys stay the English token (read-only).

**Remaining follow-up (smaller):** auto-discovery for **non-status** enum-like
string fields (e.g. `service_type`) — today they're hand-authorable and remain
editable once authored, but the editor won't proactively list their values
(only status-shaped fields are auto-discovered). A cardinality-based detection
(`DISTINCT count ≤ N`) for arbitrary string fields would close the gap.

## Other signposted threads (not yet phased)

- **PII masking on the live list path** — `layout::list_render` omits
  Hidden fields but applies no masking to shown cells (noted in the
  Phase 6/7/8 `list_render` comments).
- **Per-model RBAC in the templated list** — list permissions are gated on
  `signed_in` only; the `admin/rbac.rs` subsystem isn't wired into this
  path (noted in the Phase 6 `list_render` comment).
- **Phase-5 docs/asset cleanup** — `docs/advanced/*healthcare*` and a few
  `admin.css` / `ai/executor.rs` strings still reference the deleted
  `medflow` example.
- **9d — merge UI** in the composition editor (the remaining sub-phase).
