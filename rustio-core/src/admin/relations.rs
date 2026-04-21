//! Relation Intelligence Layer — runtime registry.
//!
//! The admin renders, filters, and guards deletes on foreign keys by
//! consulting a `RelationRegistry` that is built pure-functionally
//! from a parsed [`Schema`]. The registry itself is data; it does no
//! I/O and holds no connections. Every rendering or query path passes
//! a `&RelationRegistry` explicitly — there is no global singleton and
//! no background refresh.
//!
//! ## Scope — what this module owns
//!
//! - Two lookup tables computed once per schema:
//!   (a) `belongs_to[(model, field)] → ResolvedRelation` — the forward
//!   direction declared via `#[rustio(belongs_to = "Target")]`.
//!   (b) `has_many[model] → Vec<InverseRelation>` — every incoming
//!   edge into `model` from any other model, including the
//!   field on the source that carries the FK.
//! - A single validation pass flagging declarations that reference
//!   models that don't exist in the current schema.
//! - Constants consumed by the admin UI:
//!   - [`RELATION_FILTER_DROPDOWN_CAP`] — above this row count, a
//!     filter falls back to a numeric input rather than a `<select>`.
//!
//! ## Scope — what this module does NOT own
//!
//! - SQL execution. Query builders live in `admin.rs` alongside the
//!   list / detail / delete handlers that own the `Db` handle.
//! - Rendering. The admin decides how a resolved relation looks on
//!   the page; the registry only tells it which target to look up.
//! - User-facing error messages. The delete-guard's 409 page, the
//!   filter's "too many options" hint, and the unresolved `#<id>`
//!   fallback are all rendered by `admin.rs` with copy owned there.
//!
//! ## v1 performance notes
//!
//! Current FK-label resolution on a list page issues **one extra
//! SELECT per FK column** with the IDs batched into an `IN (…)`
//! clause: for a table with `K` foreign keys and `N` rows the cost
//! is `1 + K` queries, independent of `N`. This is the v1 shape on
//! purpose — correctness and stability first.
//!
//! ### Future JOIN optimisation point
//!
//! A later pass will rewrite the list query to `LEFT JOIN` every
//! target carrying a `display_field`, projecting `display_field` as
//! an aliased column alongside the FK id. That collapses `1 + K`
//! round-trips into a single query and moves the join cost from the
//! application layer into the database engine where it belongs. Not
//! implemented yet because the admin's list query builder in
//! `admin.rs` does not currently support projection aliases, and
//! changing that shape touches more of the codebase than is warranted
//! for the initial relation layer.
//!
//! ## Future evolution — intentionally named extension points
//!
//! The v1 module is deliberately narrow. These are the places the
//! next iteration is expected to grow into, and the lines where they
//! plug in are annotated in code.
//!
//! - **Inverse panels: preview rows.** Phase 4 shows counts only. The
//!   next step is a small preview table per inverse (top N by
//!   `created_at DESC`) with its own link to a filtered list page.
//!   Extension point: a new [`ResolvedRelation::preview_query`]
//!   helper + a `render_related_preview` call in `admin.rs`.
//!
//! - **Inverse panels: per-panel navigation.** Today each panel links
//!   to `/admin/<inverse_table>?<fk>=<id>`. Future versions may open
//!   the inverse as a nested section on the same page. Extension
//!   point: the admin's detail-page renderer, not the registry.
//!
//! - **Relation-aware search (Phase 7, design only).** The admin's
//!   `?q=<text>` today searches in-table string columns only. A
//!   relation-aware version would: for every `belongs_to` on the
//!   model carrying a non-`None` `display_field`, issue
//!   `SELECT id FROM <target> WHERE <display_field> LIKE '%{q}%' LIMIT 200`,
//!   collect the ids into a `Vec<i64>`, and add `OR <field> IN (<ids>)`
//!   to the outer list query. A cap of 200 rows per target keeps the
//!   `IN` list bounded. When every target hits the cap, the UI warns
//!   ("too many matches for `q` inside `patient.full_name`; refine
//!   the search"). No code is written in this pass.
//!
//! - **Many-to-many / join tables.** Not modelled. A future
//!   `RelationKind::ManyToMany { via_table, via_left, via_right }`
//!   variant would live here alongside `BelongsTo` / `HasMany`, with
//!   the registry learning to walk the join table. Every public
//!   method on `RelationRegistry` today handles unknown variants
//!   with `_ => ...` wildcards so the addition is non-breaking.

use std::collections::HashMap;

use crate::schema::{RelationKind, Schema};

/// Soft cap on the number of rows a relation filter will expose as a
/// `<select>` dropdown. Above this threshold the admin renders a
/// numeric-id input instead, with a muted hint explaining why.
///
/// 500 was chosen to fit comfortably in one HTTP round-trip, not
/// because of any browser limit on `<option>` count (browsers handle
/// thousands fine). The real constraint is cognitive: an operator
/// cannot usefully scan through more than a few hundred options.
pub const RELATION_FILTER_DROPDOWN_CAP: usize = 500;

/// One forward (`BelongsTo`) relation resolved against the schema.
///
/// Carries enough information for the admin to:
///   1. render the FK column (`target_model`, `target_table`,
///      `target_display_field`);
///   2. link to the target's detail page (`target_admin_name`);
///   3. issue the batched prefetch query for list pages.
///
/// Everything is owned `String` rather than `&'static str` because
/// the registry is built from a parsed [`Schema`] whose `String`
/// fields outlive the registry. Copying costs once at schema-reload
/// time and buys simple lifetimes everywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRelation {
    /// Model name holding the FK column, e.g. `"Appointment"`.
    pub source_model: String,
    /// Field on the source carrying the id, e.g. `"patient_id"`.
    pub source_field: String,
    /// Target model name, e.g. `"Patient"`.
    pub target_model: String,
    /// Target model's SQL table, e.g. `"patients"`.
    pub target_table: String,
    /// Target model's admin slug, e.g. `"patients"` — what appears
    /// in `/admin/<slug>/<id>` URLs.
    pub target_admin_name: String,
    /// Column on the target whose value is rendered as the human
    /// label. `None` means the admin renders `#<id>` and does NOT
    /// infer a column — display-field guessing is explicitly off.
    pub target_display_field: Option<String>,
    /// Direction marker. Always `BelongsTo` for forward relations.
    pub kind: RelationKind,
}

/// One reverse (`HasMany`) relation — an incoming edge pointing at a
/// given target model. Produced by inverting every stored
/// `BelongsTo` at registry-build time. Consumed by the inverse-panel
/// renderer and the delete guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InverseRelation {
    /// Source model holding the FK, e.g. `"Appointment"`.
    pub source_model: String,
    /// Source model's SQL table, e.g. `"appointments"`.
    pub source_table: String,
    /// Source model's admin slug for filter links.
    pub source_admin_name: String,
    /// Source model's display name — used as the panel heading.
    /// Always the plural form ("Appointments").
    pub source_display_name: String,
    /// Field on the source pointing at `target_model.id`.
    pub source_field: String,
    /// Target model name — supplied for symmetry with
    /// [`ResolvedRelation`] even though callers already know it.
    pub target_model: String,
}

/// Why a [`RelationRegistry`] declaration was rejected. Each variant
/// names the enclosing relation so CLI tooling (`rustio schema`,
/// `ai validate`) can point a user straight at the bad declaration.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// `#[rustio(belongs_to = "X")]` on `<model>.<field>` but `X`
    /// doesn't exist in the schema.
    UnknownTarget {
        model: String,
        field: String,
        target: String,
    },
    /// `display = "col"` but `col` isn't a field on the target model.
    /// The macro's compile-time check catches this at build time; the
    /// runtime check exists as a backstop for hand-edited
    /// `rustio.schema.json` files (AI-layer users).
    UnknownDisplayField {
        model: String,
        field: String,
        target: String,
        display: String,
    },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTarget {
                model,
                field,
                target,
            } => write!(
                f,
                "`{model}.{field}` declares `belongs_to = \"{target}\"`, \
                 but no model named `{target}` exists in the schema"
            ),
            Self::UnknownDisplayField {
                model,
                field,
                target,
                display,
            } => write!(
                f,
                "`{model}.{field}` declares `display = \"{display}\"` against `{target}`, \
                 but `{target}` has no field named `{display}`"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

/// Relation lookup tables for one snapshot of the schema.
///
/// Build once per schema reload and consume by `&`-reference from
/// every admin handler that needs it. Pure data — no interior
/// mutability, no cached queries, no hidden state.
///
/// ```text
/// belongs_to[("Appointment", "patient_id")] → ResolvedRelation {
///     target_model: "Patient",
///     target_table: "patients",
///     target_display_field: Some("full_name"),
///     …
/// }
///
/// has_many["Patient"] → [
///     InverseRelation { source_model: "Appointment", source_field: "patient_id", … },
///     InverseRelation { source_model: "Invoice",     source_field: "patient_id", … },
/// ]
/// ```
#[derive(Debug, Clone, Default)]
pub struct RelationRegistry {
    belongs_to: HashMap<(String, String), ResolvedRelation>,
    has_many: HashMap<String, Vec<InverseRelation>>,
    /// Forward relations indexed by source model. Used by the list
    /// handler when it needs to enumerate every FK on a given model
    /// (to render filters, to pre-fetch labels, etc).
    belongs_to_of: HashMap<String, Vec<ResolvedRelation>>,
}

impl RelationRegistry {
    /// Empty registry. Every lookup returns `None`. Useful as a
    /// safe default when the schema file is missing or fails to
    /// parse — the admin falls back to raw-id rendering.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build the registry from a [`Schema`]. Silent on unknown
    /// targets / display fields — call [`validate`](Self::validate)
    /// after if you want those surfaced as errors. Building is
    /// intentionally lenient so a single typo in
    /// `rustio.schema.json` doesn't blank out the entire registry.
    pub fn from_schema(schema: &Schema) -> Self {
        let mut belongs_to: HashMap<(String, String), ResolvedRelation> = HashMap::new();
        let mut has_many: HashMap<String, Vec<InverseRelation>> = HashMap::new();
        let mut belongs_to_of: HashMap<String, Vec<ResolvedRelation>> = HashMap::new();

        // Index models by name for O(1) target lookup.
        let index: HashMap<&str, &crate::schema::SchemaModel> =
            schema.models.iter().map(|m| (m.name.as_str(), m)).collect();

        for source in &schema.models {
            for field in &source.fields {
                let Some(rel) = &field.relation else {
                    continue;
                };
                let Some(target) = index.get(rel.model.as_str()) else {
                    // Dangling relation: recorded via `validate` below,
                    // not surfaced here.
                    continue;
                };
                // Silently drop a relation whose display field is
                // declared but doesn't exist on the target. The admin
                // falls back to `#<id>`; `validate` surfaces this as a
                // warning/error for tooling that cares.
                let display_field = match &rel.display_field {
                    None => None,
                    Some(col) => {
                        if target.fields.iter().any(|f| &f.name == col) {
                            Some(col.clone())
                        } else {
                            None
                        }
                    }
                };

                let resolved = ResolvedRelation {
                    source_model: source.name.clone(),
                    source_field: field.name.clone(),
                    target_model: target.name.clone(),
                    target_table: target.table.clone(),
                    target_admin_name: target.admin_name.clone(),
                    target_display_field: display_field,
                    kind: rel.kind,
                };

                belongs_to.insert((source.name.clone(), field.name.clone()), resolved.clone());

                belongs_to_of
                    .entry(source.name.clone())
                    .or_default()
                    .push(resolved.clone());

                if matches!(rel.kind, RelationKind::BelongsTo) {
                    has_many
                        .entry(target.name.clone())
                        .or_default()
                        .push(InverseRelation {
                            source_model: source.name.clone(),
                            source_table: source.table.clone(),
                            source_admin_name: source.admin_name.clone(),
                            source_display_name: source.display_name.clone(),
                            source_field: field.name.clone(),
                            target_model: target.name.clone(),
                        });
                }
            }
        }

        // Deterministic order: sort inverse lists by source model name
        // so panel rendering is stable across runs.
        for list in has_many.values_mut() {
            list.sort_by(|a, b| a.source_model.cmp(&b.source_model));
        }
        for list in belongs_to_of.values_mut() {
            list.sort_by(|a, b| a.source_field.cmp(&b.source_field));
        }

        Self {
            belongs_to,
            has_many,
            belongs_to_of,
        }
    }

    /// The `ResolvedRelation` for `(model, field)`, if any.
    pub fn belongs_to(&self, model: &str, field: &str) -> Option<&ResolvedRelation> {
        self.belongs_to.get(&(model.to_string(), field.to_string()))
    }

    /// Every forward relation owned by a source model. Used by the
    /// list handler to enumerate FK columns for prefetch + filters.
    /// Returns an empty slice when the model declares none.
    pub fn belongs_to_of(&self, model: &str) -> &[ResolvedRelation] {
        self.belongs_to_of
            .get(model)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Every incoming edge into `model`. Used by the inverse-panel
    /// renderer and the delete guard.
    pub fn has_many(&self, model: &str) -> &[InverseRelation] {
        self.has_many
            .get(model)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// `true` if the registry knows no relations at all. Cheap check
    /// used by list / detail handlers to skip the prefetch machinery
    /// entirely when the project has no annotations.
    pub fn is_empty(&self) -> bool {
        self.belongs_to.is_empty()
    }

    /// Walk every stored relation and report declarations that
    /// reference models or columns not present in the schema. The
    /// macro's compile-time checks already catch these for declared
    /// fields; this pass also covers hand-edited `rustio.schema.json`
    /// files (the AI pipeline writes those).
    pub fn validate(&self, schema: &Schema) -> Vec<RegistryError> {
        let mut errors: Vec<RegistryError> = Vec::new();
        let models: HashMap<&str, &crate::schema::SchemaModel> =
            schema.models.iter().map(|m| (m.name.as_str(), m)).collect();

        for source in &schema.models {
            for field in &source.fields {
                let Some(rel) = &field.relation else {
                    continue;
                };
                let Some(target) = models.get(rel.model.as_str()) else {
                    errors.push(RegistryError::UnknownTarget {
                        model: source.name.clone(),
                        field: field.name.clone(),
                        target: rel.model.clone(),
                    });
                    continue;
                };
                if let Some(display) = &rel.display_field {
                    if !target.fields.iter().any(|f| &f.name == display) {
                        errors.push(RegistryError::UnknownDisplayField {
                            model: source.name.clone(),
                            field: field.name.clone(),
                            target: rel.model.clone(),
                            display: display.clone(),
                        });
                    }
                }
            }
        }

        errors
    }

    /// A forward iterator over every ResolvedRelation in the
    /// registry, in deterministic order. Not called on hot paths;
    /// convenient for tests and CLI introspection commands.
    pub fn iter_belongs_to(&self) -> impl Iterator<Item = &ResolvedRelation> {
        let mut entries: Vec<&ResolvedRelation> = self.belongs_to.values().collect();
        entries.sort_by(|a, b| {
            a.source_model
                .cmp(&b.source_model)
                .then_with(|| a.source_field.cmp(&b.source_field))
        });
        entries.into_iter()
    }
}
