//! Deterministic ViewSpec renderer.
//!
//! Given a [`ViewSpec`] (what to show, and how) plus the actual data
//! rows, this module produces a [`RenderedView`] — a **structured,
//! serializable** description of what to display. It deliberately stops
//! short of HTML: like [`crate::schema`] and the parent [`crate::viewspec`]
//! module, core stays *presentation-data*, and the web layer owns markup.
//! A later UI phase turns a [`RenderedView`] into pixels.
//!
//! ## Guarantees
//!
//! - **Pure & deterministic.** `(ViewSpec, rows) → RenderedView` with no
//!   I/O, no AI, no randomness, and no clock-dependent formatting. Rows
//!   are iterated through a [`BTreeMap`], so field iteration order never
//!   leaks nondeterminism. Render the same inputs twice → identical output.
//! - **The ViewSpec is the only authority.** The renderer never inspects
//!   raw schema order and never dumps unknown columns: a value reaches the
//!   output only if some [`FieldSpec`] in the ViewSpec names it.
//! - **`Hidden` is never emitted.** A [`FieldRole::Hidden`] spec is dropped
//!   before any value is read — the one data-safety rule this phase
//!   enforces. (Masking of *shown* fields is `intelligence`'s job at a
//!   later integration point; this phase does not mask.)
//! - **Never-empty rows.** See [`select_specs`] for the fallback that keeps
//!   a row from rendering blank when a layout's role set surfaces nothing.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Timelike};
use serde::{Deserialize, Serialize};

use super::{FieldRole, FieldSpec, ViewLayout, ViewSpec};

/// Separator placed between merged sub-values and between date/time parts
/// of a formatted timestamp. A space-flanked middle dot.
const SEP: &str = " · ";

// ---------------------------------------------------------------------------
// Input row shape (intentionally decoupled from the ORM)
// ---------------------------------------------------------------------------

/// One data row as field-name → display value. Values are pre-stringified
/// by the caller; the renderer never queries the database. A [`BTreeMap`]
/// is used so iteration is deterministic.
pub type Row = BTreeMap<String, RowValue>;

/// A single cell's raw value, before display formatting. Neutral on
/// purpose — the renderer formats these into strings, so callers don't
/// have to pre-format and the formatting rules live in one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RowValue {
    /// A text value, emitted as-is (except for the `Timestamp` role, which
    /// may reformat a parseable datetime).
    Text(String),
    /// An integer, emitted via `to_string`.
    Int(i64),
    /// A boolean, emitted as `"Yes"` / `"No"`.
    Bool(bool),
    /// A missing / null value, emitted as the empty string.
    Null,
}

// ---------------------------------------------------------------------------
// Output shape
// ---------------------------------------------------------------------------

/// The fully-rendered, layout-agnostic-at-the-cell-level view. Serializable
/// so the web layer can consume it directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderedView {
    /// The model this view renders (copied from [`ViewSpec::model`]).
    pub model: String,
    /// The layout the rows were rendered for.
    pub layout: ViewLayout,
    /// One entry per input row, in input order.
    pub rows: Vec<RenderedRow>,
}

/// One rendered row: an ordered list of cells. Cells follow
/// [`ViewSpec::fields`] order, filtered to the layout's role set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderedRow {
    /// The cells to display for this row.
    pub cells: Vec<RenderedCell>,
}

/// One rendered cell. Carries the role (so the web layer can choose
/// markup), a human label, the formatted value, and the source field
/// name(s) it was built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderedCell {
    /// The role this cell plays, from its [`FieldSpec`].
    pub role: FieldRole,
    /// Human label derived from the (anchor) source name — snake_case
    /// turned into `"Title Case"`.
    pub label: String,
    /// The formatted display value. For a merged cell, the merged values
    /// joined per the documented rule (see [`render_cell`]).
    pub value: String,
    /// The field name(s) this cell came from: one, or many if merged.
    pub sources: Vec<String>,
}

// ---------------------------------------------------------------------------
// Layout → role selection (the single auditable source of truth)
// ---------------------------------------------------------------------------

const TABLE_ROLES: &[FieldRole] = &[
    FieldRole::Title,
    FieldRole::Subtitle,
    FieldRole::Badge,
    FieldRole::Timestamp,
    FieldRole::Meta,
];
const LIST_ROLES: &[FieldRole] = &[
    FieldRole::Title,
    FieldRole::Subtitle,
    FieldRole::Badge,
    FieldRole::Timestamp,
];
const CARDS_ROLES: &[FieldRole] = &[
    FieldRole::Title,
    FieldRole::Subtitle,
    FieldRole::Badge,
    FieldRole::Timestamp,
    FieldRole::Meta,
];
const COMPACT_ROLES: &[FieldRole] = &[FieldRole::Title, FieldRole::Badge];

/// The set of roles a layout surfaces. This is a **membership filter**,
/// not an ordering — cells are always emitted in [`ViewSpec::fields`]
/// order; a spec is emitted only if its role is in this set. [`Hidden`] is
/// in no layout's set, ever.
///
/// - **Table** — every non-`Hidden` role, each as its own cell/column.
/// - **List** — `Title` + `Subtitle` + `Badge` + `Timestamp`; `Meta`
///   omitted (shown only in detail/expanded contexts).
/// - **Cards** — every non-`Hidden` role (same membership as Table; the
///   two differ only in the web layer's markup, not here).
/// - **Compact** — `Title` + `Badge` only.
///
/// [`Hidden`]: FieldRole::Hidden
pub fn roles_for_layout(layout: ViewLayout) -> &'static [FieldRole] {
    match layout {
        ViewLayout::Table => TABLE_ROLES,
        ViewLayout::List => LIST_ROLES,
        ViewLayout::Cards => CARDS_ROLES,
        ViewLayout::Compact => COMPACT_ROLES,
    }
}

/// Pick, in [`ViewSpec::fields`] order, the specs a `layout` will render.
///
/// A spec is selected when it is **not** [`FieldRole::Hidden`] *and* its
/// role is in [`roles_for_layout`]. (`Hidden` is doubly excluded — it is
/// never in any layout's role set, and we filter it explicitly so the
/// data-safety rule reads at the call site.)
///
/// ## Never-empty-row guarantee
///
/// If the layout's role set would surface **no** cell — e.g. `Compact` on
/// a ViewSpec with neither a `Title` nor a `Badge`, or a hand-written spec
/// whose only visible field is `Meta` — this falls back to emitting the
/// **first non-`Hidden`** [`FieldSpec`] in declared order as a single
/// cell, so a row is never blank. The fallback can only fail to produce a
/// cell when *every* field is `Hidden`; hiding everything is a deliberate
/// choice and correctly renders nothing.
fn select_specs(spec: &ViewSpec, layout: ViewLayout) -> Vec<&FieldSpec> {
    let allowed = roles_for_layout(layout);
    let selected: Vec<&FieldSpec> = spec
        .fields
        .iter()
        .filter(|f| f.role != FieldRole::Hidden && allowed.contains(&f.role))
        .collect();
    if !selected.is_empty() {
        return selected;
    }
    // Never-empty-row fallback: first non-Hidden field, if any.
    spec.fields
        .iter()
        .find(|f| f.role != FieldRole::Hidden)
        .into_iter()
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl RenderedView {
    /// Render `rows` using the ViewSpec's own default [`ViewSpec::layout`].
    pub fn render(spec: &ViewSpec, rows: &[Row]) -> Self {
        Self::render_with_layout(spec, spec.layout, rows)
    }

    /// Render `rows` for an explicit `layout`, overriding the ViewSpec's
    /// default. The same `(spec, rows)` always yield the same output.
    pub fn render_with_layout(spec: &ViewSpec, layout: ViewLayout, rows: &[Row]) -> Self {
        let specs = select_specs(spec, layout);
        let rendered_rows = rows
            .iter()
            .map(|row| RenderedRow {
                cells: specs.iter().map(|fs| render_cell(fs, row)).collect(),
            })
            .collect();
        Self {
            model: spec.model.clone(),
            layout,
            rows: rendered_rows,
        }
    }
}

/// Build one cell from a [`FieldSpec`] and a row.
///
/// - **Plain cell** — `value` is the field's formatted value (role-aware:
///   a `Timestamp` role reformats a parseable datetime, see
///   [`format_value`]); `sources` is the single source name.
/// - **Merged cell** (`merge` set) — each source in `merge` is formatted
///   with the *basic* formatter (no per-sub timestamp special-casing),
///   empty results are dropped, and the rest are joined with `" · "`. The
///   anchor source is conventionally first in `merge`, so it leads. The
///   `label` is the anchor (the [`FieldSpec::source`]) humanised, and
///   `sources` lists every merged name in order.
fn render_cell(fs: &FieldSpec, row: &Row) -> RenderedCell {
    let (value, sources) = match &fs.merge {
        Some(merge) => {
            let value = merge
                .iter()
                .map(|name| format_basic(row.get(name)))
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(SEP);
            (value, merge.clone())
        }
        None => (
            format_value(fs.role, row.get(&fs.source)),
            vec![fs.source.clone()],
        ),
    };
    RenderedCell {
        role: fs.role,
        label: humanise(&fs.source),
        value,
        sources,
    }
}

/// Format a value with role awareness. Only [`FieldRole::Timestamp`]
/// differs from the basic formatter: a `Text` value that parses as an
/// RFC3339 datetime is reformatted; anything else passes through
/// [`format_basic`].
fn format_value(role: FieldRole, value: Option<&RowValue>) -> String {
    if role == FieldRole::Timestamp {
        if let Some(RowValue::Text(s)) = value {
            return format_timestamp(s);
        }
    }
    format_basic(value)
}

/// The basic, role-independent value formatter:
/// `Text` as-is, `Int` via `to_string`, `Bool` → `"Yes"`/`"No"`,
/// `Null` / missing → empty string.
fn format_basic(value: Option<&RowValue>) -> String {
    match value {
        Some(RowValue::Text(s)) => s.clone(),
        Some(RowValue::Int(n)) => n.to_string(),
        Some(RowValue::Bool(true)) => "Yes".to_string(),
        Some(RowValue::Bool(false)) => "No".to_string(),
        Some(RowValue::Null) | None => String::new(),
    }
}

/// Format an RFC3339 datetime string deterministically as
/// `"Mon D, YYYY · HH:MM"` (24-hour), e.g.
/// `2026-06-25T14:30:00Z` → `Jun 25, 2026 · 14:30`. Built from parsed
/// date parts so it is platform-independent (no `strftime` padding
/// quirks), locale-free, and free of any relative ("today") logic. If the
/// input does not parse, it is returned unchanged.
fn format_timestamp(s: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    match DateTime::parse_from_rfc3339(s) {
        Ok(dt) => {
            let month = MONTHS[(dt.month() as usize) - 1];
            format!(
                "{month} {day}, {year}{SEP}{hour:02}:{minute:02}",
                day = dt.day(),
                year = dt.year(),
                hour = dt.hour(),
                minute = dt.minute(),
            )
        }
        Err(_) => s.to_string(),
    }
}

/// snake_case → `"Title Case"`. Mirrors the local helper in
/// `admin::intelligence`; kept private here so the renderer doesn't reach
/// across module boundaries for a one-liner.
fn humanise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' {
            out.push(' ');
            next_upper = true;
        } else if next_upper {
            out.push(ch.to_ascii_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{SchemaField, SchemaModel};

    fn sf(name: &str, ty: &str) -> SchemaField {
        SchemaField {
            name: name.to_string(),
            ty: ty.to_string(),
            nullable: false,
            editable: true,
            relation: None,
        }
    }

    /// "Customer"-shaped schema model used to derive a default ViewSpec.
    fn customer_model() -> SchemaModel {
        SchemaModel {
            name: "Customer".to_string(),
            table: "customers".to_string(),
            admin_name: "customers".to_string(),
            display_name: "Customers".to_string(),
            singular_name: "Customer".to_string(),
            fields: vec![
                sf("id", "i64"),
                sf("name", "String"),
                sf("email", "String"),
                sf("status", "String"),
                sf("created_at", "DateTime"),
                sf("password_hash", "String"),
                sf("notes", "String"),
            ],
            relations: Vec::new(),
            core: false,
        }
    }

    fn row(pairs: &[(&str, RowValue)]) -> Row {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn customer_rows() -> Vec<Row> {
        vec![
            row(&[
                ("id", RowValue::Int(1)),
                ("name", RowValue::Text("Alice".into())),
                ("email", RowValue::Text("alice@example.com".into())),
                ("status", RowValue::Text("active".into())),
                ("created_at", RowValue::Text("2026-06-25T14:30:00Z".into())),
                ("password_hash", RowValue::Text("secret123".into())),
                ("notes", RowValue::Text("VIP".into())),
            ]),
            row(&[
                ("id", RowValue::Int(2)),
                ("name", RowValue::Text("Bob".into())),
                ("email", RowValue::Text("bob@example.com".into())),
                ("status", RowValue::Text("suspended".into())),
                ("created_at", RowValue::Text("2025-01-02T09:05:00Z".into())),
                ("password_hash", RowValue::Text("hunter2".into())),
                ("notes", RowValue::Text("".into())),
            ]),
        ]
    }

    /// Source names of the first rendered row's cells, in order.
    fn first_row_sources(rv: &RenderedView) -> Vec<Vec<String>> {
        rv.rows[0].cells.iter().map(|c| c.sources.clone()).collect()
    }

    fn first_row_roles(rv: &RenderedView) -> Vec<FieldRole> {
        rv.rows[0].cells.iter().map(|c| c.role).collect()
    }

    #[test]
    fn each_layout_surfaces_exactly_its_roles() {
        let spec = ViewSpec::from_schema_model(&customer_model());
        let rows = customer_rows();

        // Table: every visible field (Hidden id + password_hash dropped).
        let table = RenderedView::render_with_layout(&spec, ViewLayout::Table, &rows);
        assert_eq!(
            first_row_sources(&table),
            vec![
                vec!["name".to_string()],
                vec!["email".to_string()],
                vec!["status".to_string()],
                vec!["created_at".to_string()],
                vec!["notes".to_string()],
            ]
        );
        assert_eq!(
            first_row_roles(&table),
            vec![
                FieldRole::Title,
                FieldRole::Subtitle,
                FieldRole::Badge,
                FieldRole::Timestamp,
                FieldRole::Meta,
            ]
        );

        // List: Meta (notes) omitted.
        let list = RenderedView::render_with_layout(&spec, ViewLayout::List, &rows);
        assert_eq!(
            first_row_roles(&list),
            vec![
                FieldRole::Title,
                FieldRole::Subtitle,
                FieldRole::Badge,
                FieldRole::Timestamp,
            ]
        );

        // Cards: same membership as Table.
        let cards = RenderedView::render_with_layout(&spec, ViewLayout::Cards, &rows);
        assert_eq!(first_row_roles(&cards), first_row_roles(&table));

        // Compact: Title + Badge only.
        let compact = RenderedView::render_with_layout(&spec, ViewLayout::Compact, &rows);
        assert_eq!(
            first_row_sources(&compact),
            vec![vec!["name".to_string()], vec!["status".to_string()]]
        );
        assert_eq!(
            first_row_roles(&compact),
            vec![FieldRole::Title, FieldRole::Badge]
        );

        // Every layout rendered both rows.
        for rv in [&table, &list, &cards, &compact] {
            assert_eq!(rv.rows.len(), 2);
            assert_eq!(rv.model, "Customer");
        }
    }

    #[test]
    fn hidden_value_never_reaches_output_in_any_layout() {
        // `password_hash` is Hidden in the derived spec; its value must not
        // appear anywhere in the serialized RenderedView, for any layout.
        let spec = ViewSpec::from_schema_model(&customer_model());
        let rows = customer_rows();
        for layout in [
            ViewLayout::Table,
            ViewLayout::List,
            ViewLayout::Cards,
            ViewLayout::Compact,
        ] {
            let rv = RenderedView::render_with_layout(&spec, layout, &rows);
            let json = serde_json::to_string(&rv).unwrap();
            assert!(
                !json.contains("secret123"),
                "hidden value leaked into {layout:?}: {json}"
            );
            assert!(
                !json.contains("hunter2"),
                "hidden value leaked into {layout:?}: {json}"
            );
        }
    }

    #[test]
    fn merged_cell_joins_sources_and_values() {
        // A Title cell merging name + email renders one cell carrying both
        // sources and the documented " · "-joined value.
        let spec = ViewSpec {
            version: super::super::VIEWSPEC_VERSION,
            model: "Customer".to_string(),
            layout: ViewLayout::Table,
            fields: vec![FieldSpec {
                source: "name".to_string(),
                role: FieldRole::Title,
                merge: Some(vec!["name".to_string(), "email".to_string()]),
                filterable: false,
            }],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
        };
        let rows = vec![row(&[
            ("name", RowValue::Text("Alice".into())),
            ("email", RowValue::Text("alice@example.com".into())),
        ])];
        let rv = RenderedView::render_with_layout(&spec, ViewLayout::Table, &rows);
        assert_eq!(rv.rows[0].cells.len(), 1);
        let cell = &rv.rows[0].cells[0];
        assert_eq!(cell.sources, vec!["name".to_string(), "email".to_string()]);
        assert_eq!(cell.value, "Alice · alice@example.com");
        assert_eq!(cell.label, "Name");
    }

    #[test]
    fn merged_cell_drops_empty_values() {
        // An empty/missing merged source produces no dangling separator.
        let spec = ViewSpec {
            version: super::super::VIEWSPEC_VERSION,
            model: "Customer".to_string(),
            layout: ViewLayout::Table,
            fields: vec![FieldSpec {
                source: "name".to_string(),
                role: FieldRole::Title,
                merge: Some(vec!["name".to_string(), "email".to_string()]),
                filterable: false,
            }],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
        };
        let rows = vec![row(&[
            ("name", RowValue::Text("Alice".into())),
            ("email", RowValue::Null),
        ])];
        let rv = RenderedView::render_with_layout(&spec, ViewLayout::Table, &rows);
        assert_eq!(rv.rows[0].cells[0].value, "Alice");
    }

    #[test]
    fn timestamp_formats_to_exact_expected_string() {
        let spec = ViewSpec::from_schema_model(&customer_model());
        let rows = customer_rows();
        let rv = RenderedView::render_with_layout(&spec, ViewLayout::Table, &rows);
        let ts = rv.rows[0]
            .cells
            .iter()
            .find(|c| c.sources == vec!["created_at".to_string()])
            .unwrap();
        assert_eq!(ts.value, "Jun 25, 2026 · 14:30");

        // Second row, different instant — still exact, still 24h.
        let ts2 = rv.rows[1]
            .cells
            .iter()
            .find(|c| c.sources == vec!["created_at".to_string()])
            .unwrap();
        assert_eq!(ts2.value, "Jan 2, 2025 · 09:05");
    }

    #[test]
    fn unparseable_timestamp_passes_through() {
        assert_eq!(format_timestamp("not a date"), "not a date");
    }

    #[test]
    fn bool_and_null_format_as_documented() {
        assert_eq!(format_basic(Some(&RowValue::Bool(true))), "Yes");
        assert_eq!(format_basic(Some(&RowValue::Bool(false))), "No");
        assert_eq!(format_basic(Some(&RowValue::Null)), "");
        assert_eq!(format_basic(None), "");
        assert_eq!(format_basic(Some(&RowValue::Int(42))), "42");
    }

    #[test]
    fn never_empty_row_falls_back_to_first_visible_field() {
        // A spec whose only visible field is Meta, rendered in Compact
        // (which surfaces neither Title nor Badge), still yields one
        // non-empty cell per row: the first non-Hidden field.
        let spec = ViewSpec {
            version: super::super::VIEWSPEC_VERSION,
            model: "Note".to_string(),
            layout: ViewLayout::Compact,
            fields: vec![
                FieldSpec {
                    source: "id".to_string(),
                    role: FieldRole::Hidden,
                    merge: None,
                    filterable: false,
                },
                FieldSpec {
                    source: "notes".to_string(),
                    role: FieldRole::Meta,
                    merge: None,
                    filterable: false,
                },
            ],
            filters: vec![],
            default_language: "en".to_string(),
            labels: std::collections::BTreeMap::new(),
        };
        let rows = vec![row(&[
            ("id", RowValue::Int(7)),
            ("notes", RowValue::Text("hello".into())),
        ])];
        let rv = RenderedView::render_with_layout(&spec, ViewLayout::Compact, &rows);
        assert_eq!(rv.rows[0].cells.len(), 1);
        assert_eq!(rv.rows[0].cells[0].sources, vec!["notes".to_string()]);
        assert_eq!(rv.rows[0].cells[0].value, "hello");
        // And the fallback never reaches the Hidden field.
        let json = serde_json::to_string(&rv).unwrap();
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn rendering_is_deterministic() {
        let spec = ViewSpec::from_schema_model(&customer_model());
        let rows = customer_rows();
        let a = RenderedView::render_with_layout(&spec, ViewLayout::Cards, &rows);
        let b = RenderedView::render_with_layout(&spec, ViewLayout::Cards, &rows);
        assert_eq!(a, b);
    }

    #[test]
    fn render_uses_specs_default_layout() {
        // `render` (no explicit layout) honours ViewSpec::layout, which
        // `from_schema_model` sets to List.
        let spec = ViewSpec::from_schema_model(&customer_model());
        let rows = customer_rows();
        let rv = RenderedView::render(&spec, &rows);
        assert_eq!(rv.layout, ViewLayout::List);
        assert_eq!(
            first_row_roles(&rv),
            vec![
                FieldRole::Title,
                FieldRole::Subtitle,
                FieldRole::Badge,
                FieldRole::Timestamp,
            ]
        );
    }
}
