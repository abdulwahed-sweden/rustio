//! Phase 14, commit 5 — bridge from `ModelSchema` to admin metadata.
//!
//! This module is the framework's first real consumer of the
//! Phase 14 schema contract. Given a `&ModelSchema` produced by
//! `#[derive(RustioModel)]`, it emits the per-column admin
//! metadata required by the existing admin UI without a hand-
//! written `AdminModel` impl.
//!
//! # What stays untouched
//!
//! Existing manual admin paths (the `#[derive(RustioAdmin)]`
//! macro and projects that hand-build an `AdminModel`) are not
//! affected. This module is **additive** — it produces values
//! that consumers can plug into the existing `AdminEntry`
//! constructor; it never modifies, replaces, or shadows any
//! existing admin type.
//!
//! # Mapping rules (Phase 14, commit 5 spec)
//!
//! For each `ModelColumn`:
//!
//! | Contract field      | Bridge output                              |
//! |---------------------|--------------------------------------------|
//! | `name`              | `AdminField.name` (verbatim)               |
//! | `admin_label`       | `AdminField.label` (fallback = `name`)     |
//! | `admin_widget`      | `BridgedField.widget` (preserved through)  |
//! | `flags.searchable`  | `BridgedField.searchable`                  |
//! | `flags.filterable`  | `BridgedField.filterable`                  |
//! | `flags.sortable`    | `BridgedField.sortable`                    |
//! | `flags.readonly`    | `BridgedField.readonly` + `editable=!ro`   |
//! | `primary_key`       | `BridgedField.primary_key`                 |
//!
//! `AdminField` (the existing type) only models `editable`. The
//! remaining flag bits and the widget hint live on
//! `BridgedField` — a side-channel struct so consumers (search
//! indexer, list/sort UI, future renderer changes) can read them
//! without breaking `AdminField`'s shape.
//!
//! # Static lifetimes via `Box::leak`
//!
//! `AdminField` requires `&'static str` and an `&'static
//! [AdminField]` slice (the existing macro emits compile-time
//! constants). When bridging at runtime, we promote owned data
//! to static via `Box::leak`. This is a one-time setup cost
//! equivalent to a `static`: schemas are registered at process
//! startup and live for the program's lifetime, so leaked memory
//! is never reclaimed but never grows either.
//!
//! # No DB, no reflection, no new deps
//!
//! Pure CPU. No async, no database access, no `unsafe`, no new
//! `Cargo.toml` entries.

use crate::admin::types::{AdminField, FieldType};
use crate::contract::{ModelColumn, ModelSchema, RustType};

// ---------------------------------------------------------------------------
// FieldType mapping
// ---------------------------------------------------------------------------

/// Map a contract column's `(RustType, nullable)` pair to the
/// admin's `FieldType` vocabulary.
///
/// Variants the admin layer doesn't model natively (`F64`,
/// `Decimal`, `JsonValue`, `Uuid`) fall through to
/// `String` / `OptionalString` — admin renders them as text
/// inputs, which preserves their values without inventing
/// widgets that don't exist yet. Future commits may extend
/// `FieldType` with dedicated variants; until then text input
/// is the safe minimum.
pub fn field_type_for(col: &ModelColumn) -> FieldType {
    // Match exhaustively — `RustType` is `#[non_exhaustive]` only
    // cross-crate, but inside `rustio-core` it's exhaustive. Keeping
    // the match tight means adding a future variant fails compilation
    // here until the bridge gets an explicit mapping; a wildcard
    // would silently fall back to text input and mask the gap.
    use RustType::*;
    match col.rust_type {
        // The admin's `FieldType` has no `OptionalI32` variant; a
        // nullable `i32` column collapses to `OptionalI64` since
        // both render as the same numeric input. Non-nullable
        // `i32` keeps its dedicated variant.
        I32 if col.nullable => FieldType::OptionalI64,
        I32 => FieldType::I32,
        I64 if col.nullable => FieldType::OptionalI64,
        I64 => FieldType::I64,
        // `Bool` has no nullable variant in the admin layer; a
        // tri-state checkbox isn't part of the existing UI, so
        // nullable bools render as the same checkbox (NULL is
        // treated as `false` at form-submission time).
        Bool => FieldType::Bool,
        String if col.nullable => FieldType::OptionalString,
        String => FieldType::String,
        DateTimeUtc if col.nullable => FieldType::OptionalDateTime,
        DateTimeUtc => FieldType::DateTime,
        // Variants the admin layer doesn't model natively — `F64`,
        // `Decimal`, `JsonValue`, `Uuid` — collapse to text inputs
        // (`String` / `OptionalString`). Documented behaviour;
        // a future commit may extend `FieldType` with dedicated
        // variants and tighten these.
        F64 | Decimal | JsonValue | Uuid if col.nullable => FieldType::OptionalString,
        F64 | Decimal | JsonValue | Uuid => FieldType::String,
    }
}

// ---------------------------------------------------------------------------
// Label resolution
// ---------------------------------------------------------------------------

/// Resolved admin label per the spec's fallback rule:
/// `admin_label` when set, otherwise the column's name verbatim.
///
/// Both branches return `&'static str` so the result drops
/// directly into `AdminField.label` without a leak — explicit
/// labels are already `&'static` (compile-time constants from
/// the macro), and `col.name` is `&'static` by `ModelColumn`
/// definition.
pub fn label_for(col: &ModelColumn) -> &'static str {
    col.admin_label.unwrap_or(col.name)
}

// ---------------------------------------------------------------------------
// BridgedField
// ---------------------------------------------------------------------------

/// One column in its bridge form: the existing `AdminField`
/// (consumed verbatim by the admin UI) plus the column-level
/// flags `AdminField` doesn't model.
///
/// Consumers:
/// - The admin renderer plucks `.field` out for `AdminEntry`.
/// - A search-index sync layer (commit 6) reads `.searchable`.
/// - Future filter/sort UI reads `.filterable` / `.sortable`.
/// - The `.primary_key` bit identifies the row-id column for
///   any code that needs it without re-scanning the schema.
#[derive(Debug, Clone)]
pub struct BridgedField {
    /// The existing-shape admin field. Plug this directly into
    /// `AdminEntry.fields` (after `Box::leak`-ing the slice).
    pub field: AdminField,
    /// `true` when the source column has `primary_key = true`.
    /// Mirrors `ModelColumn.primary_key`.
    pub primary_key: bool,
    /// `flags.searchable` from the source column.
    pub searchable: bool,
    /// `flags.filterable` from the source column.
    pub filterable: bool,
    /// `flags.sortable` from the source column.
    pub sortable: bool,
    /// `flags.readonly` from the source column. Also drives
    /// `field.editable = !readonly`.
    pub readonly: bool,
    /// `admin_widget` from the source column, preserved
    /// verbatim. `AdminField` doesn't carry a widget override
    /// today; the existing renderer derives the widget from
    /// `FieldType.widget()`. Holding the override here lets
    /// future renderer code consult it without altering
    /// `AdminField`'s shape.
    pub widget: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Public bridge API
// ---------------------------------------------------------------------------

/// Bridge every column in declaration order. Order is
/// preserved 1:1 with `schema.columns` — the admin UI lists
/// columns in the order the model declared them, and skipping
/// or reordering would silently change rendered forms.
pub fn bridged_fields_from_schema(schema: &ModelSchema) -> Vec<BridgedField> {
    schema
        .columns
        .iter()
        .map(|col| BridgedField {
            field: AdminField {
                name: col.name,
                label: label_for(col),
                field_type: field_type_for(col),
                editable: !col.flags.readonly,
                relation: None,
                choices: None,
            },
            primary_key: col.primary_key,
            searchable: col.flags.searchable,
            filterable: col.flags.filterable,
            sortable: col.flags.sortable,
            readonly: col.flags.readonly,
            widget: col.admin_widget,
        })
        .collect()
}

/// Static-leaked `&'static [AdminField]` for direct use as
/// `AdminEntry.fields`. Equivalent to a `static` array — the
/// memory is allocated once and lives the program's lifetime.
pub fn admin_fields_from_schema(schema: &ModelSchema) -> &'static [AdminField] {
    let fields: Vec<AdminField> = bridged_fields_from_schema(schema)
        .into_iter()
        .map(|b| b.field)
        .collect();
    Box::leak(fields.into_boxed_slice())
}

/// The schema's primary-key column, located by the
/// `primary_key = true` flag. Returns `None` when no column
/// is flagged (a malformed schema; the validator in commit 3
/// surfaces this as `WrongPrimaryKey`).
pub fn primary_key_column(schema: &ModelSchema) -> Option<&ModelColumn> {
    schema.columns.iter().find(|c| c.primary_key)
}

// ---------------------------------------------------------------------------
// SchemaOps — Phase 14, commit 8
// ---------------------------------------------------------------------------
//
// `SchemaOps` is a generic `AdminOps` implementation that drives
// CRUD using only a `ModelSchema` — no `AdminModel` impl, no
// `Model` impl, no per-type code. The admin runtime registers
// schema-driven entries via `Admin::from_schema::<T>()`, and the
// resulting `AdminEntry` ferries CRUD through this type.
//
// SQL is built dynamically from the schema's column list. Type
// dispatch is via `ModelColumn::rust_type` — every supported
// `RustType` variant maps to one read path
// (`format_pg_value_for_column`) and one write path
// (`bind_form_value`). Variants the framework doesn't yet model
// natively for the admin path return a clear validation error
// rather than silently coercing to text.
//
// Constraint envelope:
// - No new dependencies (sqlx, chrono, uuid, serde_json are
//   already pulled in via rustio-core's Cargo.toml).
// - Read-only against schema metadata; write paths only modify
//   rows in the model's own table.
// - The SQL strings are built from `ModelSchema`'s `&'static
//   str` column / table names — there is no path for a request
//   to inject arbitrary identifiers (the column name list is
//   defined at compile time by `#[derive(RustioModel)]`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::Row as SqlxRow;

use crate::admin::types::{AdminEntry, AdminOps, EditRow, ListRow};
use crate::contract::HasSchema;
use crate::error::{Error, Result};
use crate::http::FormData;
use crate::orm::Db;

/// Static-leaked `ModelSchema`. Required because `AdminEntry`
/// stores `&'static str` for table / admin_name / etc., and the
/// schema needs to outlive every async future spawned from
/// `SchemaOps`. Schemas are registered at startup and live for
/// the program's lifetime, so the leak is a one-time setup cost
/// equivalent to a `static`.
fn leak_schema(schema: ModelSchema) -> &'static ModelSchema {
    Box::leak(Box::new(schema))
}

/// `AdminOps` driven entirely by a `ModelSchema`.
///
/// Holds a static-leaked schema so each async `AdminOps` method
/// can borrow the column list across `await` points without
/// lifetime issues — `'a` references the captured `&'a self`,
/// but the underlying schema reference is `'static`.
pub(crate) struct SchemaOps {
    schema: &'static ModelSchema,
}

impl SchemaOps {
    fn new(schema: &'static ModelSchema) -> Self {
        Self { schema }
    }

    fn pk_col(&self) -> &'static crate::contract::ModelColumn {
        // The schema's `primary_key` field names the PK column;
        // primary_key_column finds the entry flagged
        // `primary_key = true`. We trust both agree (the
        // validator surfaces drift) and prefer the latter.
        primary_key_column(self.schema).unwrap_or_else(|| {
            // Defensive: a schema without any flagged PK column
            // is a contract bug. Returning the first column
            // makes the code defensible without panicking; the
            // validator's `WrongPrimaryKey` issue catches it
            // separately.
            &self.schema.columns[0]
        })
    }

    /// Columns the create/update path writes. Excludes the
    /// primary key (assumed BIGSERIAL — auto-assigned by PG)
    /// and any column flagged `readonly` (e.g. `created_at
    /// DEFAULT NOW()`). Returns the column references in
    /// declaration order.
    fn writable_columns(&self) -> Vec<&'static crate::contract::ModelColumn> {
        self.schema
            .columns
            .iter()
            .filter(|c| !c.primary_key && !c.flags.readonly)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Per-RustType read formatting — turns a sqlx row column into a
// String suitable for `ListRow.cells` / `EditRow.values`.
// ---------------------------------------------------------------------------

fn format_pg_value_for_column(
    row: &sqlx::postgres::PgRow,
    col: &crate::contract::ModelColumn,
) -> String {
    // Centralised null handling: any column read that errors out
    // OR returns NULL maps to the empty string. The render layer
    // displays empty strings as "—" already.
    use crate::contract::RustType::*;
    match (col.rust_type, col.nullable) {
        (I32, false) => row.try_get::<i32, _>(col.name).map(|v| v.to_string()).unwrap_or_default(),
        (I32, true) => row
            .try_get::<Option<i32>, _>(col.name)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        (I64, false) => row.try_get::<i64, _>(col.name).map(|v| v.to_string()).unwrap_or_default(),
        (I64, true) => row
            .try_get::<Option<i64>, _>(col.name)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        (Bool, false) => row.try_get::<bool, _>(col.name).map(|b| b.to_string()).unwrap_or_default(),
        (Bool, true) => row
            .try_get::<Option<bool>, _>(col.name)
            .ok()
            .flatten()
            .map(|b| b.to_string())
            .unwrap_or_default(),
        (String, false) => row.try_get::<std::string::String, _>(col.name).unwrap_or_default(),
        (String, true) => row
            .try_get::<Option<std::string::String>, _>(col.name)
            .ok()
            .flatten()
            .unwrap_or_default(),
        (DateTimeUtc, false) => row
            .try_get::<DateTime<Utc>, _>(col.name)
            .map(|d| d.to_rfc3339())
            .unwrap_or_default(),
        (DateTimeUtc, true) => row
            .try_get::<Option<DateTime<Utc>>, _>(col.name)
            .ok()
            .flatten()
            .map(|d| d.to_rfc3339())
            .unwrap_or_default(),
        (F64, false) => row.try_get::<f64, _>(col.name).map(|v| v.to_string()).unwrap_or_default(),
        (F64, true) => row
            .try_get::<Option<f64>, _>(col.name)
            .ok()
            .flatten()
            .map(|v| v.to_string())
            .unwrap_or_default(),
        (Uuid, false) => row
            .try_get::<uuid::Uuid, _>(col.name)
            .map(|u| u.to_string())
            .unwrap_or_default(),
        (Uuid, true) => row
            .try_get::<Option<uuid::Uuid>, _>(col.name)
            .ok()
            .flatten()
            .map(|u| u.to_string())
            .unwrap_or_default(),
        // Decimal and JsonValue: render the raw text the DB
        // returns rather than parsing into a typed value the
        // admin layer can't carry without an extra dep. PG
        // exposes both as text-coercible — `::text` cast in the
        // query would be cleaner, but reading as String works
        // for the common shapes.
        (Decimal, _) | (JsonValue, _) => row
            .try_get::<std::string::String, _>(col.name)
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Per-RustType write parsing — turns a form value string into a
// SQL bind argument; emits a clear validation error on parse
// failure rather than panicking.
// ---------------------------------------------------------------------------

/// Bind one form value onto a `sqlx::query` builder, dispatched
/// by `RustType` + nullability. Returns the updated builder on
/// success, or a string error suitable for the `Err(Vec<String>)`
/// validation channel of `AdminOps::create` / `update`.
///
/// Empty form input on a nullable column binds `NULL`. Empty
/// form input on a non-nullable column binds the empty string
/// (for `String`) or returns a "required" error (for typed
/// columns).
fn bind_form_value<'a>(
    q: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    col: &crate::contract::ModelColumn,
    raw: Option<&str>,
) -> std::result::Result<sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>, std::string::String> {
    use crate::contract::RustType::*;
    let raw = raw.unwrap_or("").trim();

    // Empty input + nullable column = NULL. Empty input +
    // String column = empty string (the DB constraint catches
    // NOT NULL TEXT fields when they should have content).
    if raw.is_empty() && col.nullable {
        return Ok(match col.rust_type {
            I32 => q.bind(None::<i32>),
            I64 => q.bind(None::<i64>),
            F64 => q.bind(None::<f64>),
            Bool => q.bind(None::<bool>),
            String => q.bind(None::<std::string::String>),
            DateTimeUtc => q.bind(None::<DateTime<Utc>>),
            Uuid => q.bind(None::<uuid::Uuid>),
            // Decimal / JsonValue null-binding goes through the
            // text path; PG accepts NULL casts at the protocol
            // level for any column.
            Decimal | JsonValue => q.bind(None::<std::string::String>),
        });
    }

    let parsed: std::result::Result<sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>, std::string::String> = match col.rust_type {
        I32 => raw
            .parse::<i32>()
            .map(|v| q.bind(v))
            .map_err(|e| format!("`{}`: {}", col.name, e)),
        I64 => raw
            .parse::<i64>()
            .map(|v| q.bind(v))
            .map_err(|e| format!("`{}`: {}", col.name, e)),
        F64 => raw
            .parse::<f64>()
            .map(|v| q.bind(v))
            .map_err(|e| format!("`{}`: {}", col.name, e)),
        Bool => Ok({
            // HTML form checkboxes send "on" / "true" / "1" when
            // checked, nothing when unchecked. The form layer
            // normalises absent fields to None — by the time we
            // see a string here it's almost always "on" for
            // truthy. Unknown tokens default to false rather
            // than rejecting outright; the column's `NOT NULL
            // DEFAULT FALSE` semantics match.
            let truthy = matches!(
                raw.to_ascii_lowercase().as_str(),
                "on" | "true" | "1" | "yes"
            );
            q.bind(truthy)
        }),
        String => Ok(q.bind(raw.to_string())),
        DateTimeUtc => DateTime::parse_from_rfc3339(raw)
            .map(|dt| q.bind(dt.with_timezone(&Utc)))
            .map_err(|e| format!("`{}`: expected RFC3339 timestamp ({})", col.name, e)),
        Uuid => uuid::Uuid::parse_str(raw)
            .map(|u| q.bind(u))
            .map_err(|e| format!("`{}`: {}", col.name, e)),
        Decimal | JsonValue => Ok(q.bind(raw.to_string())),
    };

    parsed
}

// ---------------------------------------------------------------------------
// AdminOps — the read + write surface
// ---------------------------------------------------------------------------

type CreateFut<'a> = Pin<Box<dyn Future<Output = Result<std::result::Result<i64, Vec<std::string::String>>>> + Send + 'a>>;
type UpdateFut<'a> = Pin<Box<dyn Future<Output = Result<std::result::Result<(), Vec<std::string::String>>>> + Send + 'a>>;

impl AdminOps for SchemaOps {
    fn list<'a>(
        &'a self,
        db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>> {
        Box::pin(async move {
            let pk = self.pk_col();
            let cols = self
                .schema
                .columns
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT {cols} FROM {} ORDER BY {} DESC LIMIT 200",
                self.schema.table, pk.name
            );
            let rows = sqlx::query(&sql)
                .fetch_all(db.pool())
                .await
                .map_err(|e| Error::Internal(format!("schema-list({}): {e}", self.schema.table)))?;

            let out = rows
                .into_iter()
                .map(|row| {
                    // ID column comes back as i64 (BIGSERIAL); fall
                    // back to 0 if the column is shaped differently.
                    let id = row.try_get::<i64, _>(pk.name).unwrap_or(0);
                    let cells = self
                        .schema
                        .columns
                        .iter()
                        .map(|c| format_pg_value_for_column(&row, c))
                        .collect();
                    ListRow { id, cells }
                })
                .collect();
            Ok(out)
        })
    }

    fn find_row<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>> {
        Box::pin(async move {
            let pk = self.pk_col();
            let cols = self
                .schema
                .columns
                .iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT {cols} FROM {} WHERE {} = $1",
                self.schema.table, pk.name
            );
            let maybe_row = sqlx::query(&sql)
                .bind(id)
                .fetch_optional(db.pool())
                .await
                .map_err(|e| Error::Internal(format!("schema-find({}): {e}", self.schema.table)))?;
            Ok(maybe_row.map(|row| {
                let values = self
                    .schema
                    .columns
                    .iter()
                    .map(|c| (c.name.to_string(), format_pg_value_for_column(&row, c)))
                    .collect();
                EditRow { id, values }
            }))
        })
    }

    fn create<'a>(&'a self, db: &'a Db, form: &'a FormData) -> CreateFut<'a> {
        Box::pin(async move {
            let pk = self.pk_col();
            let writables = self.writable_columns();
            let col_names: Vec<&str> = writables.iter().map(|c| c.name).collect();
            let placeholders: Vec<std::string::String> =
                (1..=writables.len()).map(|i| format!("${i}")).collect();
            let sql = format!(
                "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
                self.schema.table,
                col_names.join(", "),
                placeholders.join(", "),
                pk.name
            );

            let mut q = sqlx::query(&sql);
            let mut errors: Vec<std::string::String> = Vec::new();
            for col in &writables {
                match bind_form_value(q, col, form.get(col.name)) {
                    Ok(next) => q = next,
                    Err(msg) => {
                        errors.push(msg);
                        // Bind a placeholder so subsequent
                        // bindings stay aligned with placeholders;
                        // the query won't run if errors is
                        // non-empty.
                        q = sqlx::query(&sql); // reset; we won't execute
                        break;
                    }
                }
            }
            if !errors.is_empty() {
                return Ok(Err(errors));
            }

            let row = q
                .fetch_one(db.pool())
                .await
                .map_err(|e| Error::Internal(format!("schema-create({}): {e}", self.schema.table)))?;
            let id: i64 = row
                .try_get(pk.name)
                .map_err(|e| Error::Internal(format!("returning {}: {e}", pk.name)))?;
            db.invalidate(self.schema.table);
            Ok(Ok(id))
        })
    }

    fn update<'a>(&'a self, db: &'a Db, id: i64, form: &'a FormData) -> UpdateFut<'a> {
        Box::pin(async move {
            let pk = self.pk_col();
            let writables = self.writable_columns();
            let sets: Vec<std::string::String> = writables
                .iter()
                .enumerate()
                .map(|(i, c)| format!("{} = ${}", c.name, i + 1))
                .collect();
            let sql = format!(
                "UPDATE {} SET {} WHERE {} = ${}",
                self.schema.table,
                sets.join(", "),
                pk.name,
                writables.len() + 1
            );

            let mut q = sqlx::query(&sql);
            let mut errors: Vec<std::string::String> = Vec::new();
            for col in &writables {
                match bind_form_value(q, col, form.get(col.name)) {
                    Ok(next) => q = next,
                    Err(msg) => {
                        errors.push(msg);
                        q = sqlx::query(&sql);
                        break;
                    }
                }
            }
            if !errors.is_empty() {
                return Ok(Err(errors));
            }
            q = q.bind(id);
            q.execute(db.pool())
                .await
                .map_err(|e| Error::Internal(format!("schema-update({}): {e}", self.schema.table)))?;
            db.invalidate(self.schema.table);
            Ok(Ok(()))
        })
    }

    fn delete<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let pk = self.pk_col();
            let sql = format!(
                "DELETE FROM {} WHERE {} = $1",
                self.schema.table, pk.name
            );
            sqlx::query(&sql)
                .bind(id)
                .execute(db.pool())
                .await
                .map_err(|e| Error::Internal(format!("schema-delete({}): {e}", self.schema.table)))?;
            db.invalidate(self.schema.table);
            Ok(())
        })
    }

    fn object_label<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<std::string::String>>> + Send + 'a>> {
        Box::pin(async move {
            // Pick the first non-PK String column as the label
            // source; fall back to "{table} #{id}" when there's
            // none. Mirrors the heuristic the AdminModel-driven
            // path uses for object_label.
            let label_col = self.schema.columns.iter().find(|c| {
                !c.primary_key
                    && matches!(c.rust_type, crate::contract::RustType::String)
            });
            let pk = self.pk_col();
            match label_col {
                Some(col) => {
                    let sql = format!(
                        "SELECT {} FROM {} WHERE {} = $1",
                        col.name, self.schema.table, pk.name
                    );
                    let row = sqlx::query(&sql)
                        .bind(id)
                        .fetch_optional(db.pool())
                        .await
                        .map_err(|e| {
                            Error::Internal(format!(
                                "schema-object-label({}): {e}",
                                self.schema.table
                            ))
                        })?;
                    Ok(row.and_then(|r| {
                        let v = if col.nullable {
                            r.try_get::<Option<std::string::String>, _>(col.name)
                                .ok()
                                .flatten()
                        } else {
                            r.try_get::<std::string::String, _>(col.name).ok()
                        };
                        v.filter(|s| !s.is_empty())
                    }))
                }
                None => Ok(Some(format!("{} #{}", self.schema.table, id))),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// AdminEntry construction from a ModelSchema
// ---------------------------------------------------------------------------

/// Build a fully-configured `AdminEntry` from a `ModelSchema`,
/// without requiring an `AdminModel` impl. The resulting entry's
/// CRUD goes through `SchemaOps`; the field metadata comes from
/// `admin_fields_from_schema`.
///
/// `admin_name`, `display_name`, and `singular_name` are derived
/// from `schema.table`:
///
/// - `admin_name` = `schema.table` verbatim (route prefix)
/// - `display_name` = humanised + Title Case (`"projects"` →
///   `"Projects"`)
/// - `singular_name` = humanised + naive singular (strip a
///   trailing `s`; `"projects"` → `"Project"`)
///
/// Naive singularisation is fine for the common-case English
/// plural; project models with irregular plurals can extend the
/// macro layer with a `#[rustio(singular = "...")]` attribute in
/// a future commit.
pub fn admin_entry_from_schema(schema: ModelSchema) -> AdminEntry {
    let static_schema = leak_schema(schema);
    let admin_name: &'static str = static_schema.table;
    let display_name: &'static str =
        Box::leak(humanise_table(static_schema.table).into_boxed_str());
    let singular_name: &'static str =
        Box::leak(singularise(static_schema.table).into_boxed_str());

    AdminEntry {
        admin_name,
        display_name,
        singular_name,
        table: static_schema.table,
        fields: admin_fields_from_schema(static_schema),
        core: false,
        ops: Arc::new(SchemaOps::new(static_schema)),
        search_hook: None,
    }
}

/// Same as `admin_entry_from_schema` but takes the model type
/// rather than a schema value. Convenience wrapper around
/// `T::SCHEMA`.
pub fn admin_entry_from_type<T: HasSchema>() -> AdminEntry {
    admin_entry_from_schema(T::SCHEMA)
}

/// `"projects"` → `"Projects"`. ASCII Title Case of the first
/// character; rest unchanged. Underscores → spaces.
fn humanise_table(name: &str) -> std::string::String {
    let mut out = std::string::String::with_capacity(name.len());
    let mut next_upper = true;
    for ch in name.chars() {
        if ch == '_' {
            out.push(' ');
            next_upper = true;
        } else if next_upper {
            out.extend(ch.to_uppercase());
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// `"projects"` → `"Project"`. Strips a trailing `s` after
/// humanising. English-naive — irregular plurals (people,
/// children, indices) round-trip wrongly. A future
/// `#[rustio(singular = "...")]` attribute is the planned
/// override.
fn singularise(name: &str) -> std::string::String {
    let h = humanise_table(name);
    if let Some(stripped) = h.strip_suffix('s') {
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }
    h
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::SchemaFlags;

    // ----- Test fixture -----------------------------------------------------

    /// A schema covering every mapping rule the bridge must
    /// honour: primary key, label override, widget override,
    /// every flag bit, every commonly-used `RustType`, both
    /// nullable and non-nullable. One static fixture so the
    /// individual tests don't drift from each other.
    fn fixture_schema() -> ModelSchema {
        static COLS: &[ModelColumn] = &[
            // Primary key, readonly (auto-managed).
            ModelColumn {
                name: "id",
                sql_decl: "BIGSERIAL PRIMARY KEY",
                rust_type: RustType::I64,
                nullable: false,
                primary_key: true,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: false,
                    sortable: true,
                    readonly: true,
                },
                admin_label: None,
                admin_widget: None,
            },
            // Searchable + filterable + explicit label.
            ModelColumn {
                name: "title",
                sql_decl: "TEXT NOT NULL",
                rust_type: RustType::String,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: true,
                    filterable: true,
                    sortable: false,
                    readonly: false,
                },
                admin_label: Some("Headline"),
                admin_widget: None,
            },
            // Nullable string + widget override.
            ModelColumn {
                name: "body",
                sql_decl: "TEXT",
                rust_type: RustType::String,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: true,
                    filterable: false,
                    sortable: false,
                    readonly: false,
                },
                admin_label: None,
                admin_widget: Some("textarea"),
            },
            // Nullable timestamp + sortable.
            ModelColumn {
                name: "published_at",
                sql_decl: "TIMESTAMPTZ",
                rust_type: RustType::DateTimeUtc,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: true,
                    sortable: true,
                    readonly: false,
                },
                admin_label: None,
                admin_widget: None,
            },
            // Bool, no flags set.
            ModelColumn {
                name: "is_pinned",
                sql_decl: "BOOLEAN NOT NULL",
                rust_type: RustType::Bool,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            },
        ];
        ModelSchema {
            table: "posts",
            columns: COLS,
            primary_key: "id",
            search_index: Some("posts"),
        }
    }

    // ----- Required by spec -------------------------------------------------

    /// Spec gate: "fields generated from schema". One
    /// `BridgedField` per column, none dropped, none added.
    #[test]
    fn fields_generated_from_schema_one_per_column() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        assert_eq!(
            bridged.len(),
            schema.columns.len(),
            "every ModelColumn must produce exactly one BridgedField"
        );
    }

    /// Spec gate: "ordering preserved". Bridge output order
    /// matches the schema's declaration order column-for-column.
    /// Reordering would silently re-arrange admin forms.
    #[test]
    fn ordering_preserved_matches_schema_columns() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        let bridged_names: Vec<&str> = bridged.iter().map(|b| b.field.name).collect();
        let schema_names: Vec<&str> = schema.columns.iter().map(|c| c.name).collect();
        assert_eq!(
            bridged_names, schema_names,
            "BridgedField order must mirror ModelSchema.columns order"
        );
    }

    /// Spec gate: "flags correctly mapped". Every flag bit
    /// flows through to the matching `BridgedField` field.
    #[test]
    fn flags_correctly_mapped_per_column() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // `id`: sortable + readonly only.
        let id = &bridged[0];
        assert!(!id.searchable);
        assert!(!id.filterable);
        assert!(id.sortable);
        assert!(id.readonly);
        assert!(!id.field.editable, "readonly => editable=false");

        // `title`: searchable + filterable.
        let title = &bridged[1];
        assert!(title.searchable);
        assert!(title.filterable);
        assert!(!title.sortable);
        assert!(!title.readonly);
        assert!(title.field.editable);

        // `body`: searchable only.
        let body = &bridged[2];
        assert!(body.searchable);
        assert!(!body.filterable);
        assert!(!body.sortable);
        assert!(!body.readonly);

        // `published_at`: filterable + sortable.
        let pa = &bridged[3];
        assert!(!pa.searchable);
        assert!(pa.filterable);
        assert!(pa.sortable);

        // `is_pinned`: all flags off.
        let pin = &bridged[4];
        assert!(!pin.searchable);
        assert!(!pin.filterable);
        assert!(!pin.sortable);
        assert!(!pin.readonly);
    }

    /// Spec gate: "label fallback works". When `admin_label`
    /// is set, the bridge uses it verbatim; when `None`, the
    /// fallback is the column's name string (per spec table:
    /// "fallback = name").
    #[test]
    fn label_fallback_uses_column_name_when_no_override() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // Override case: `title` had `admin_label = Some("Headline")`.
        assert_eq!(bridged[1].field.label, "Headline");

        // Fallback case: `id`, `body`, `published_at`, `is_pinned`
        // all had `admin_label = None`.
        assert_eq!(bridged[0].field.label, "id");
        assert_eq!(bridged[2].field.label, "body");
        assert_eq!(bridged[3].field.label, "published_at");
        assert_eq!(bridged[4].field.label, "is_pinned");
    }

    /// Spec gate: "widget override works". `admin_widget`
    /// from the source column is preserved on `BridgedField.widget`
    /// verbatim. `None` stays `None`.
    #[test]
    fn widget_override_preserved_through_bridge() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        assert_eq!(bridged[2].widget, Some("textarea"), "body's textarea override must survive");
        assert!(bridged[0].widget.is_none(), "id had no widget override");
        assert!(bridged[1].widget.is_none(), "title had no widget override");
        assert!(bridged[3].widget.is_none(), "published_at had no widget override");
        assert!(bridged[4].widget.is_none(), "is_pinned had no widget override");
    }

    /// Spec gate: "primary key detected". The bridge surfaces
    /// the same column flagged in the contract; helper picks
    /// it out by reference.
    #[test]
    fn primary_key_detected_from_schema() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // Exactly one column flagged primary_key.
        let pk_count = bridged.iter().filter(|b| b.primary_key).count();
        assert_eq!(pk_count, 1, "fixture has exactly one primary-key column");
        assert!(bridged[0].primary_key, "the `id` column is the PK");
        assert!(!bridged[1].primary_key);

        // Helper resolves the same column.
        let pk = primary_key_column(&schema).expect("PK exists in fixture");
        assert_eq!(pk.name, "id");
    }

    /// `primary_key_column` returns `None` when no column is
    /// flagged. The validator in commit 3 surfaces this as a
    /// `WrongPrimaryKey` issue; the bridge just reports honestly.
    #[test]
    fn primary_key_column_returns_none_when_unflagged() {
        static COLS: &[ModelColumn] = &[ModelColumn {
            name: "value",
            sql_decl: "TEXT NOT NULL",
            rust_type: RustType::String,
            nullable: false,
            primary_key: false,
            flags: SchemaFlags::empty(),
            admin_label: None,
            admin_widget: None,
        }];
        let schema = ModelSchema {
            table: "scratch",
            columns: COLS,
            primary_key: "value",
            search_index: None,
        };
        assert!(primary_key_column(&schema).is_none());
    }

    // ----- FieldType mapping -----------------------------------------------

    /// Every `RustType` variant the admin natively models has
    /// the documented `(nullable, non-nullable)` pair.
    #[test]
    fn field_type_mapping_covers_native_variants() {
        // Helper: build a stub column for type-mapping checks.
        fn col(rust_type: RustType, nullable: bool) -> ModelColumn {
            ModelColumn {
                name: "f",
                sql_decl: "",
                rust_type,
                nullable,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            }
        }

        assert_eq!(field_type_for(&col(RustType::I32, false)), FieldType::I32);
        // No OptionalI32 variant — nullable I32 collapses into OptionalI64.
        assert_eq!(field_type_for(&col(RustType::I32, true)), FieldType::OptionalI64);

        assert_eq!(field_type_for(&col(RustType::I64, false)), FieldType::I64);
        assert_eq!(field_type_for(&col(RustType::I64, true)), FieldType::OptionalI64);

        assert_eq!(field_type_for(&col(RustType::Bool, false)), FieldType::Bool);
        assert_eq!(field_type_for(&col(RustType::Bool, true)), FieldType::Bool);

        assert_eq!(field_type_for(&col(RustType::String, false)), FieldType::String);
        assert_eq!(field_type_for(&col(RustType::String, true)), FieldType::OptionalString);

        assert_eq!(field_type_for(&col(RustType::DateTimeUtc, false)), FieldType::DateTime);
        assert_eq!(field_type_for(&col(RustType::DateTimeUtc, true)), FieldType::OptionalDateTime);
    }

    /// Variants the admin doesn't natively model (`F64`,
    /// `Decimal`, `JsonValue`, `Uuid`) collapse to text inputs.
    /// Documented behaviour; a future commit may extend
    /// `FieldType` and tighten these.
    #[test]
    fn field_type_mapping_falls_back_to_string_for_unmodelled_variants() {
        fn col(rust_type: RustType, nullable: bool) -> ModelColumn {
            ModelColumn {
                name: "f",
                sql_decl: "",
                rust_type,
                nullable,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            }
        }

        for rt in [RustType::F64, RustType::Decimal, RustType::JsonValue, RustType::Uuid] {
            assert_eq!(field_type_for(&col(rt, false)), FieldType::String, "{:?} -> String", rt);
            assert_eq!(field_type_for(&col(rt, true)), FieldType::OptionalString, "{:?} -> OptionalString", rt);
        }
    }

    // ----- AdminField slice helper -----------------------------------------

    /// `admin_fields_from_schema` returns a slice the same
    /// length and order as `bridged_fields_from_schema`, with
    /// each `AdminField` matching the bridge output verbatim.
    #[test]
    fn admin_fields_slice_matches_bridge_output() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        let slice = admin_fields_from_schema(&schema);

        assert_eq!(slice.len(), bridged.len());
        for (i, f) in slice.iter().enumerate() {
            assert_eq!(f.name, bridged[i].field.name, "name @{}", i);
            assert_eq!(f.label, bridged[i].field.label, "label @{}", i);
            assert_eq!(f.field_type, bridged[i].field.field_type, "field_type @{}", i);
            assert_eq!(f.editable, bridged[i].field.editable, "editable @{}", i);
        }
    }

    /// The slice satisfies the `&'static [AdminField]` shape
    /// `AdminEntry.fields` requires — it can be used in places
    /// where a `'static` lifetime is mandatory. Compile-time
    /// gate: this won't compile if the helper returns a non-
    /// static reference.
    #[test]
    fn admin_fields_slice_is_static_lifetime() {
        fn assert_static(_x: &'static [AdminField]) {}
        let schema = fixture_schema();
        let slice = admin_fields_from_schema(&schema);
        assert_static(slice);
    }

    /// A column with `flags.readonly = true` produces
    /// `AdminField.editable = false`. The inverse
    /// (readonly = false → editable = true) is also covered.
    #[test]
    fn editable_is_inverse_of_readonly() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        for b in &bridged {
            assert_eq!(
                b.field.editable, !b.readonly,
                "editable must always equal !readonly for `{}`",
                b.field.name
            );
        }
    }

    /// Empty schema → empty bridge output. A schema with zero
    /// columns is malformed but the bridge shouldn't panic.
    #[test]
    fn empty_schema_produces_empty_bridge_output() {
        static COLS: &[ModelColumn] = &[];
        let schema = ModelSchema {
            table: "empty",
            columns: COLS,
            primary_key: "id",
            search_index: None,
        };
        assert_eq!(bridged_fields_from_schema(&schema).len(), 0);
        assert_eq!(admin_fields_from_schema(&schema).len(), 0);
        assert!(primary_key_column(&schema).is_none());
    }

    // ----- Phase 14, commit 8 — name derivation for AdminEntry -----------

    /// Plain plural `"projects"` humanises + singularises to
    /// `"Projects"` / `"Project"`.
    #[test]
    fn humanise_table_capitalises_first_letter() {
        assert_eq!(super::humanise_table("projects"), "Projects");
        assert_eq!(super::humanise_table("clients"), "Clients");
        assert_eq!(super::humanise_table("invoices"), "Invoices");
    }

    /// Underscore tables humanise as Title Case (every
    /// underscore-separated word capitalised).
    #[test]
    fn humanise_table_translates_underscores_to_spaces() {
        assert_eq!(super::humanise_table("audit_logs"), "Audit Logs");
        assert_eq!(super::humanise_table("user_profiles"), "User Profiles");
    }

    /// Naive singular: strips a trailing `s` after humanising.
    #[test]
    fn singularise_strips_trailing_s() {
        assert_eq!(super::singularise("projects"), "Project");
        assert_eq!(super::singularise("clients"), "Client");
        assert_eq!(super::singularise("invoices"), "Invoice");
        // Single-word table without trailing `s` round-trips.
        assert_eq!(super::singularise("status"), "Statu"); // documents naive behaviour
    }

    /// `admin_entry_from_schema` builds an entry with derived
    /// names and the bridge's field list. Integration test that
    /// exercises every commit-5 + commit-8 admin surface
    /// without a DB.
    #[test]
    fn admin_entry_from_schema_packages_metadata_correctly() {
        let schema = fixture_schema();
        let entry = super::admin_entry_from_schema(schema);

        assert_eq!(entry.admin_name, "posts");
        assert_eq!(entry.display_name, "Posts");
        assert_eq!(entry.singular_name, "Post");
        assert_eq!(entry.table, "posts");
        assert!(!entry.core, "schema-derived entries are never `core`");

        // Field list matches the bridge output column-for-column.
        let names: Vec<&str> = entry.fields.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            vec!["id", "title", "body", "published_at", "is_pinned"]
        );

        // No search hook attached — search wiring is a separate
        // step (Indexer::from_schema in commit 8).
        assert!(entry.search_hook.is_none());
    }

    /// `Admin::from_schemas` registers one entry per supplied
    /// schema and preserves the input order (the existing
    /// `core` user entry is pre-seeded; new entries appear after).
    #[test]
    fn admin_from_schemas_registers_each_schema_in_order() {
        use crate::admin::types::Admin;

        let schemas = vec![
            ModelSchema {
                table: "alpha",
                columns: fixture_schema().columns,
                primary_key: "id",
                search_index: None,
            },
            ModelSchema {
                table: "beta",
                columns: fixture_schema().columns,
                primary_key: "id",
                search_index: None,
            },
        ];

        let admin = Admin::new().from_schemas(&schemas);
        let entry_tables: Vec<&str> =
            admin.entries().iter().map(|e| e.table).collect();

        // Core user entry at index 0; the two schema entries
        // follow in declaration order.
        assert!(entry_tables.contains(&"alpha"));
        assert!(entry_tables.contains(&"beta"));
        let alpha_pos = entry_tables.iter().position(|t| *t == "alpha").unwrap();
        let beta_pos = entry_tables.iter().position(|t| *t == "beta").unwrap();
        assert!(alpha_pos < beta_pos, "from_schemas preserves slice order");
    }
}
