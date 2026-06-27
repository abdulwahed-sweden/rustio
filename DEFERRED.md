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

**Shipped.** The toolbar now renders a filter control per `ViewSpec.filters`
field: a tri-state select for booleans, a dropdown of distinct values
(low-cardinality, displayed via i18n value labels, value = English token) for
enum-like columns, and a free-text box for high-cardinality columns; controls
auto-submit (app.js) with a no-JS Apply and a Clear link, preserving
search/sort/layout. Crucially, the **application** now honours `ViewSpec.filters`
too — `classify_filters` accepts a field declared by the view (not just
`AdminUiField.filterable`), so filters actually filter on the live list (a
verified gap: new-style models set no macro-`filterable`, so filters were dead).

**Follow-ups — also shipped.** FK (relation) filters now render a related-row
dropdown (distinct ids → resolved labels via `fk_lookup_batch`); and a
low-cardinality dropdown matches exactly (`=`) while only free-text boxes use
substring (`LIKE`). See the "Other signposted threads — all shipped" section
below.

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

**Non-status enum auto-discovery — shipped.** The editor now also auto-lists
non-status **`String`** fields (FK ids are integers → excluded by type; `Title`
and `Hidden` roles excluded) whose distinct-value count is **≤ 12** (an early-
stopping `DISTINCT LIMIT 13` per candidate); higher-cardinality (free-text /
identity) fields are skipped. So `service_type`-style enums are discovered
alongside status fields, while names/emails/free text are not. This closes the
L4-D thread end-to-end: storage → resolver → render → per-user switching →
editor (status + non-status discovery).

## Other signposted threads — all shipped

- **PII masking on the live list path — shipped.** `list_render` now masks
  shown sensitive cells (email / phone / personal id, via
  `intelligence::classify_field`) with `mask_pii`, **context-gated**: masking
  only activates when the project declares a `rustio.context.json` (no
  surprises; default rendering unchanged). Hidden fields are still omitted.
  A merged cell (9d) masks each sensitive source individually, so a merge
  never leaks a value its own column would have masked.
- **Per-model RBAC in the templated list — shipped.** List actions are now
  gated on the signed-in user's role resolved via `rbac::Role` →
  `permissions_for(table)` (SuperAdmin/Admin → full on app models, Editor →
  no delete, Viewer → view-only; system `rustio_` tables are stricter).
- **FK relation filters — shipped.** The filter bar renders a related-row
  dropdown for FK columns (distinct ids → resolved labels via
  `fk_lookup_batch`), matched exactly.
- **Phase-5 docs/asset cleanup — done.** `medflow` references removed from
  `docs/advanced/*`, `ai/executor.rs`, and `admin.css`; the stale
  `examples/medflow` + `examples/taskhub` directories deleted (only
  `examples/bookflow` remains).
- **9d — merge UI:** shipped earlier (see the 9d commit).
