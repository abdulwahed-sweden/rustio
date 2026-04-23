//! Database-driven [`AdminModelConfig`] generation.
//!
//! Reads a SQLite table's structure via `PRAGMA table_info(...)` and
//! converts each column into an [`AdminUiField`]. The result is the
//! same kind of [`AdminModelConfig`] the manual builder produces, so
//! every downstream consumer (registry, routes, persistence,
//! rendering) stays unaware of where the metadata came from.
//!
//! Lifetime note: the registry stores `&'static str` for slug, table,
//! field names, etc. Names returned by `PRAGMA` are owned `String`s,
//! so `generate_from_table` leaks them at registration time
//! (`String::leak`). This is a one-shot at startup — the leaks are
//! bounded by the number of registered models, never per-request.
//!
//! # Constraints
//! - **No CREATE TABLE generation.** `ensure_table_sql` is left empty;
//!   the caller is expected to have provisioned the table already.
//! - **Identifier quoting.** Table names are routed through
//!   [`quote_ident`] before being interpolated into the PRAGMA
//!   statement. SQLite pragmas do not accept bind parameters, so
//!   identifier-quoting is the safety boundary here.

use sqlx::Row;

use crate::admin::admin_form_bridge::{AdminUiField, AdminUiModel};
use crate::admin::admin_generator::{from_config, AdminModelConfig};
use crate::error::Error;
use crate::orm::Db;

/// A single column as reported by `PRAGMA table_info`. Just the four
/// fields the generator actually needs — `dflt_value` and `cid` are
/// dropped to keep the surface area minimal.
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub is_primary_key: bool,
}

/// Quote a SQL identifier the same way `persistence.rs` does. Kept
/// local to avoid a cross-module visibility change for one helper.
fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Read column metadata for `table` via `PRAGMA table_info(...)`.
///
/// Returns [`Error::NotFound`] when the table is unknown or has no
/// columns (PRAGMA returns an empty result set in both cases — there
/// is no separate "missing table" signal).
pub async fn get_table_columns(db: &Db, table: &str) -> Result<Vec<ColumnInfo>, Error> {
    let sql = format!("PRAGMA table_info({})", quote_ident(table));
    let rows = sqlx::query(&sql)
        .fetch_all(db.pool())
        .await
        .map_err(Error::from)?;

    if rows.is_empty() {
        return Err(Error::NotFound);
    }

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get("name").map_err(Error::from)?;
        let data_type: String = row.try_get("type").map_err(Error::from)?;
        let not_null: i64 = row.try_get("notnull").map_err(Error::from)?;
        let pk: i64 = row.try_get("pk").map_err(Error::from)?;
        out.push(ColumnInfo {
            name,
            data_type,
            not_null: not_null != 0,
            is_primary_key: pk != 0,
        });
    }
    Ok(out)
}

/// Decide the [`AdminUiField`] shape for one column. Pure function
/// on metadata — no DB lookup. Heuristics:
///
/// - `is_*` / `*_flag` columns become booleans regardless of the
///   raw SQLite type (SQLite has no native bool — INTEGER 0/1 is
///   the convention).
/// - Names containing `email` get [`AdminDataType::Email`].
/// - Long-text hints (`description`, `content`) promote TEXT to
///   `textarea`.
/// - Unknown SQLite types fall back to text rather than erroring.
///
/// Defaults: every emitted field is `filterable`, `sortable`,
/// `visible_in_table`. The caller can post-process if a column
/// should be excluded.
pub fn column_to_field(col: &ColumnInfo) -> AdminUiField {
    use crate::admin::admin_form_bridge::AdminDataType;

    // `name` and `label` need `&'static str`. Leak once at
    // registration time — see module docs.
    let name_static: &'static str = Box::leak(col.name.clone().into_boxed_str());
    // Trivial label = identifier; renderers / future i18n can refine.
    let label_static: &'static str = name_static;

    let lname = col.name.to_lowercase();
    let upper_type = col.data_type.to_uppercase();

    let is_boolean_name = lname.starts_with("is_") || lname.ends_with("_flag");
    let is_email = lname.contains("email");
    let is_long_text = lname.contains("description") || lname.contains("content");

    // 1. Name-based overrides win over raw SQL type — boolean and
    //    email semantics matter more than storage class.
    let dt = if is_boolean_name {
        AdminDataType::Boolean
    } else if is_email {
        AdminDataType::Email
    } else if upper_type.contains("INT") {
        AdminDataType::Integer
    } else if upper_type.contains("REAL")
        || upper_type.contains("FLOA")
        || upper_type.contains("DOUB")
        || upper_type.contains("NUMERIC")
        || upper_type.contains("DECIMAL")
    {
        AdminDataType::Float
    } else if upper_type.contains("DATE") || upper_type.contains("TIME") {
        AdminDataType::DateTime
    } else if upper_type.contains("CHAR")
        || upper_type.contains("TEXT")
        || upper_type.contains("CLOB")
    {
        if is_long_text {
            AdminDataType::Text
        } else {
            AdminDataType::String
        }
    } else {
        // Fallback — treat unknown types as plain text.
        AdminDataType::String
    };

    let mut field = match dt {
        AdminDataType::Boolean => AdminUiField::boolean(name_static, label_static),
        AdminDataType::Email => AdminUiField::email(name_static, label_static),
        AdminDataType::Integer => AdminUiField::integer(name_static, label_static),
        AdminDataType::Float => AdminUiField::float(name_static, label_static),
        AdminDataType::DateTime => AdminUiField::datetime(name_static, label_static),
        AdminDataType::Text => AdminUiField::textarea(name_static, label_static),
        AdminDataType::String => AdminUiField::text(name_static, label_static),
    };

    field = field
        .required(col.not_null)
        .filterable(true)
        .sortable(true)
        .visible_in_table(true);
    field
}

/// Strip the demo prefix and pluralisation off a table name for the
/// URL slug. `admin_new_demo_users` → `users`. Tables that don't
/// carry the demo prefix pass through unchanged.
fn slug_from_table(table: &str) -> String {
    table
        .strip_prefix("admin_new_demo_")
        .unwrap_or(table)
        .to_string()
}

/// `users` → `User`, `orders` → `Order`. ASCII-only title-case +
/// trailing-`s` strip; good enough for the demo vocabulary, and
/// non-ASCII slugs simply get title-cased without pluralisation
/// surgery.
fn model_name_from_slug(slug: &str) -> String {
    let mut base = slug.to_string();
    if base.len() > 1 && base.ends_with('s') {
        base.pop();
    }
    let mut chars = base.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

/// Build an [`AdminModelConfig`] entirely from a live table's
/// schema. Equivalent to hand-writing the same config — the result
/// plugs into `register_generated` exactly like a manual one.
pub async fn generate_from_table(db: &Db, table: &str) -> Result<AdminModelConfig, Error> {
    let columns = get_table_columns(db, table).await?;
    if columns.is_empty() {
        return Err(Error::NotFound);
    }

    let pk_name = columns
        .iter()
        .find(|c| c.is_primary_key)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "id".to_string());

    // 1. Skip the PK from the editable field list — the form engine
    //    treats it as a hidden routing field, not a user input.
    let fields: Vec<AdminUiField> = columns
        .iter()
        .filter(|c| !c.is_primary_key)
        .map(column_to_field)
        .collect();

    // 2. Searchable: free-text columns only. Booleans are handled by
    //    the filter UI; integer ids are not useful for `LIKE`.
    let searchable_fields: Vec<&'static str> = columns
        .iter()
        .filter(|c| !c.is_primary_key)
        .filter(|c| {
            let lname = c.name.to_lowercase();
            let upper_type = c.data_type.to_uppercase();
            let is_boolean_name = lname.starts_with("is_") || lname.ends_with("_flag");
            !is_boolean_name
                && (upper_type.contains("CHAR")
                    || upper_type.contains("TEXT")
                    || upper_type.contains("CLOB"))
        })
        .map(|c| {
            let s: &'static str = Box::leak(c.name.clone().into_boxed_str());
            s
        })
        .collect();

    // 3. Status field: first `is_*` boolean. `_flag` columns are
    //    booleans too but the bulk Activate/Deactivate semantics
    //    align better with `is_*` naming, so the spec restricts
    //    detection to that prefix.
    let primary_status_field: Option<&'static str> = columns
        .iter()
        .find(|c| c.name.to_lowercase().starts_with("is_"))
        .map(|c| {
            let s: &'static str = Box::leak(c.name.clone().into_boxed_str());
            s
        });

    let slug_owned = slug_from_table(table);
    let slug_static: &'static str = Box::leak(slug_owned.clone().into_boxed_str());
    let model_name_static: &'static str =
        Box::leak(model_name_from_slug(&slug_owned).into_boxed_str());
    let table_static: &'static str = Box::leak(table.to_string().into_boxed_str());
    let pk_static: &'static str = Box::leak(pk_name.into_boxed_str());

    let mut cfg = AdminModelConfig::new(slug_static, model_name_static)
        .table(table_static)
        .primary_key(pk_static)
        .fields(fields)
        .searchable(searchable_fields);
    if let Some(s) = primary_status_field {
        cfg = cfg.status_field(s);
    }
    // ensure_sql intentionally left unset — schema-driven generation
    // assumes the table already exists.
    Ok(cfg)
}

/// Convenience pairing: introspect + box. Lets a caller stash the
/// result behind `Box<dyn AdminUiModel>` without naming the
/// generator type.
pub async fn generate_model_from_table(
    db: &Db,
    table: &str,
) -> Result<Box<dyn AdminUiModel>, Error> {
    let cfg = generate_from_table(db, table).await?;
    Ok(from_config(cfg))
}
