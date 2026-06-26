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
