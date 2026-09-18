# Acceptance — UX Refresh 0.13

Screenshot-verifiable criteria. Each line is checkable by looking at a rendered
page; none requires reading source.

Verify every screen at **1920 · 1440 · 900 · 390**.

---

## Global — every screen

- [ ] Masthead is 56px at every width above 480.
- [ ] Desktop rail is 240px.
- [ ] Every gap and pad is on the 4 / 8 / 12 / 16 / 24 / 32 scale.
- [ ] **No functional product text is below 13px, at any width.** 12px appears
      only on tertiary technical metadata and the sort chevron.
- [ ] **No text is smaller at 390 than it is at 1920.**
- [ ] Nothing operational is pale: `#596779` appears only on tertiary technical
      metadata, never on a field label, column head, control or value.
- [ ] Exactly one strong blue action per page, and it is that page's primary
      action.
- [ ] Every interactive element has a visible focus ring: 3px solid `#2F7BD6`,
      2px offset, drawn on the surface behind the control.
- [ ] No horizontal document overflow. Wide content scrolls inside its own
      container.
- [ ] No approved screen shows bulk-selection checkboxes, a bulk-action bar, or
      a rows-per-page control.

---

## Overview

- [ ] No stat card is visually emphasised over another.
- [ ] Every row of the model grid is full; no orphan card on the last row at
      1440 or 1920.
- [ ] Record counts are not underlined.
- [ ] Main uses the wide measure — no 1120px island inside a 1920px viewport.
- [ ] Hero, model grid and activity panel read as three separated blocks.
- [ ] At 390 the page scrolls vertically only.

---

## Records — Table

- [ ] The page reads top-to-bottom as **identity → find → present → data →
      pagination**, and the five stages are visually separable at a glance.
- [ ] Add stands alone in the header. No filter, layout control or view-editor
      action sits beside it.
- [ ] Search and filters read as one operational group.
- [ ] The result count is attached to the filtering group, not to Add.
- [ ] Layout switch and view tools read as a separate group on their own row,
      on a quieter ground, divided by a full-width rule.
- [ ] The first column anchors the record's identity — a booking number, not a
      database id.
- [ ] Column order is Booking · Resource · Status · Accepted at · Actions.
- [ ] Table header is 40px; rows are at least 48px; cells have 16px horizontal
      padding.
- [ ] Cells are 15px/500 `#171B22`; in-cell detail is 14px/500 `#3F4A59`.
- [ ] The sorted column is visibly the sorted column, with a 12px chevron.
- [ ] Inactive column heads are readable, not washed out.
- [ ] Row actions are right-aligned and visually subordinate to record content.
- [ ] Both row actions carry text labels; neither is icon-only.
- [ ] Pagination sits below the data on the table surface: range left;
      Previous, page numbers, Next right.
- [ ] Disabled pagination arrows remain visible.
- [ ] Filters that are active are marked in border, fill **and** weight.
- [ ] “Clear filters” is absent when nothing is filtering.
- [ ] There is no sort-by dropdown in the toolbar.
- [ ] At 390 the page body does not scroll sideways — only the table's own
      scroller moves.

---

## Records — Cards

- [ ] The operational rows above the records are identical to the Table board.
- [ ] A card's headline is unambiguously its most prominent element.
- [ ] No card renders its title as a label-over-value pair.
- [ ] The status badge sits top-right on the headline row at every width.
- [ ] Cards in the same row are equal height and their footers align.
- [ ] Card minimum width 320px; exactly one card per row at 390.
- [ ] Actions occupy the same position in every card.
- [ ] Headline 16px/700 `#171B22`; subtitle 15px/500 `#3F4A59`.

---

## Record Form

- [ ] The form retains a focused readable measure at 1920 — fields do not
      stretch across the monitor.
- [ ] Field groups are visually distinct; each carries a 13px mono heading.
- [ ] Readonly fields are distinguishable from editable ones without reading
      them.
- [ ] Exactly one blue button, and it is Save.
- [ ] Labels 14px/600; inputs 38px; label-to-control gap 4px everywhere.
- [ ] An invalid field shows border, fill **and** message together.
- [ ] Save and Cancel are left-aligned as a pair under the last field.
- [ ] At 390 nothing overflows and the action bar remains reachable.

---

## View Editor

- [ ] The effect of a role change is visible without leaving the page.
- [ ] Field rows do not read as a spreadsheet: human label and backend name are
      on separate lines.
- [ ] Hidden fields are visibly inert but still reorderable.
- [ ] Value-label tables are collapsed by default.
- [ ] Exactly one blue button, and it is Save view.
- [ ] The preview is labelled as a preview and states that nothing is saved
      until Save view.
- [ ] At 900 and below the preview appears above the composer.
- [ ] ▲▼ reorder controls are present and keyboard-operable.

---

## Sign In

- [ ] Card is 420px maximum and centred on both axes.
- [ ] Brand block and form share one left edge.
- [ ] Exactly one blue button.
- [ ] A failed attempt preserves the email and moves focus to the password.
- [ ] The card does not change size between the default and error states —
      only the alert is added.

---

## Foundations

- [ ] Every token in `TOKENS.md` renders as documented.
- [ ] The type specimen shows the revised scale, not the shipped one.
- [ ] Button faces are two ordinary plus one destructive semantic, and one
      small size.

---

## Behavioural regressions to watch

These are **not** design criteria — they are the behaviours a visual change
could break by accident. Confirm each still holds after implementation.

- [ ] `/admin/<model>/new` renders the create form; `/admin/<model>/create`
      still renders too.
- [ ] Clicking the rendered **Add** link reaches a 200 form.
- [ ] Clicking the first rendered **Edit** link reaches a 200 form.
- [ ] An unknown record on edit is 404, on both GET and POST.
- [ ] An unknown model is 404.
- [ ] A user without create permission gets 403, not 404.
- [ ] Every mutating form still carries `_csrf`.
- [ ] Create and edit still redirect (303) to the list and persist the change.
- [ ] Sorting a column produces the same URL the existing sort control
      produces.
- [ ] Saving a view writes the same ViewSpec it wrote before.
