//! Persistence helpers for `/admin-new` — basic CREATE + UPDATE.
//!
//! Deliberately small. No ORM integration, no schema discovery, no
//! migration framework. The caller hands in a `(table, column → value)`
//! map and these helpers build a parameterised `INSERT` / `UPDATE`
//! against the existing [`Db`] pool. Column names are sorted so the
//! emitted SQL is deterministic across runs.

use std::collections::HashMap;

use sqlx::{Column, Row};

use crate::admin::form::FormConfig;
use crate::error::Error;
use crate::orm::Db;

/// A model that knows how to map its admin form onto SQL columns.
///
/// Implementors expose:
/// - the **target table**,
/// - the **primary-key column**,
/// - and two converters that turn a [`FormConfig`] into the
///   `column → value` map [`insert_record`] / [`update_record`] need.
///
/// Splitting `to_insert_map` and `to_update_map` lets implementors
/// decide which columns participate in each path (e.g. omit
/// immutable fields on update). For models where both maps are
/// identical, callers just delegate.
pub trait PersistableModel {
    fn table_name() -> &'static str;
    fn primary_key() -> &'static str;

    fn to_insert_map(form: &FormConfig) -> HashMap<String, String>;
    fn to_update_map(form: &FormConfig) -> HashMap<String, String>;
}

/// Idempotent migration for the `/admin-new` demo table.
///
/// Lives here (rather than in `migrations.rs`) because the demo is
/// not part of the framework's core schema — it's a sandbox for the
/// admin-new submit pipeline, created lazily on first POST. Cheap to
/// invoke repeatedly: SQLite's `CREATE TABLE IF NOT EXISTS` is a
/// no-op when the table already exists.
pub async fn ensure_demo_table(db: &Db) -> Result<(), Error> {
    db.execute(
        "CREATE TABLE IF NOT EXISTS admin_new_demo_users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            email TEXT NOT NULL,
            is_active TEXT NOT NULL DEFAULT 'false',
            doctor_id TEXT,
            salary_amount TEXT
        )",
    )
    .await
}

/// Run an `INSERT INTO <table> (<cols>) VALUES (<placeholders>)`
/// against `db`. Returns the newly-allocated row id (`last_insert_rowid()`).
///
/// All values are bound positionally — caller-supplied strings are
/// never spliced into the SQL text, so column values can carry any
/// content without escaping concerns.
pub async fn insert_record(
    db: &Db,
    table: &str,
    data: &HashMap<String, String>,
) -> Result<i64, Error> {
    if data.is_empty() {
        return Err(Error::Internal("insert_record: no columns supplied".into()));
    }
    let mut cols: Vec<&String> = data.keys().collect();
    cols.sort();

    let cols_sql = cols
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = vec!["?"; cols.len()].join(", ");

    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_ident(table),
        cols_sql,
        placeholders,
    );

    let mut q = sqlx::query(&sql);
    for col in &cols {
        q = q.bind(data.get(*col).map(String::as_str).unwrap_or(""));
    }
    let result = q.execute(db.pool()).await.map_err(Error::from)?;
    Ok(result.last_insert_rowid())
}

/// Run an `UPDATE <table> SET <col = ?>... WHERE id = ?` against
/// `db`. The primary-key column is fixed to `id` for now (matches
/// the demo table); broaden when a model needs a custom PK column.
pub async fn update_record(
    db: &Db,
    table: &str,
    id: &str,
    data: &HashMap<String, String>,
) -> Result<(), Error> {
    if data.is_empty() {
        return Err(Error::Internal("update_record: no columns supplied".into()));
    }
    let mut cols: Vec<&String> = data.keys().collect();
    cols.sort();

    let set_clause = cols
        .iter()
        .map(|c| format!("{} = ?", quote_ident(c)))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "UPDATE {} SET {} WHERE \"id\" = ?",
        quote_ident(table),
        set_clause,
    );

    let mut q = sqlx::query(&sql);
    for col in &cols {
        q = q.bind(data.get(*col).map(String::as_str).unwrap_or(""));
    }
    q = q.bind(id);
    q.execute(db.pool()).await.map_err(Error::from)?;
    Ok(())
}

/// Quote a SQL identifier as `"x"`, escaping embedded double-quotes
/// by doubling them. Defensive layer — column / table names in this
/// codebase are static `&str`s today, but keeping the quoting honest
/// avoids a regression if that ever changes.
fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Fetch a single row by id and return its columns as a flat
/// `column → string` map. NULL becomes an empty string. Returns an
/// **empty** map when no row matches — the GET handler treats that
/// case as "fall back to the create-mode demo form" rather than
/// surfacing an error.
///
/// SQLite columns can be INTEGER / REAL / TEXT (the demo table uses
/// INTEGER id + TEXT for everything else). Each value is decoded by
/// trying `Option<String>` first, then `Option<i64>`, then
/// `Option<f64>`; whichever succeeds gets stringified. This is the
/// minimal coercion that keeps the result type uniform without
/// pulling in a richer SQL value model.
pub async fn get_record_by_id(
    db: &Db,
    table: &str,
    id: &str,
) -> Result<HashMap<String, String>, Error> {
    let sql = format!("SELECT * FROM {} WHERE \"id\" = ?", quote_ident(table));
    let row_opt = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(db.pool())
        .await
        .map_err(Error::from)?;

    let row = match row_opt {
        Some(r) => r,
        None => return Ok(HashMap::new()),
    };

    Ok(row_to_map(&row))
}

/// List rows from `table`, newest first, with a hard `LIMIT` /
/// `OFFSET` window. Both bounds are bound positionally — caller
/// values never enter the SQL text. Each row is flattened to the
/// same `column → string` shape as [`get_record_by_id`].
///
/// Returns an empty `Vec` when there are no rows in the window.
/// DB errors propagate; the caller decides whether to render an
/// empty table or surface them.
pub async fn list_records(
    db: &Db,
    table: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<HashMap<String, String>>, Error> {
    let sql = format!(
        "SELECT * FROM {} ORDER BY \"id\" DESC LIMIT ? OFFSET ?",
        quote_ident(table),
    );
    let rows = sqlx::query(&sql)
        .bind(limit)
        .bind(offset)
        .fetch_all(db.pool())
        .await
        .map_err(Error::from)?;

    Ok(rows.iter().map(row_to_map).collect())
}

/// `SELECT COUNT(*) FROM "<table>"`. Used by the table footer to
/// render the "Showing N of M" label and by the page header to
/// produce the records-count subtitle.
pub async fn count_records(db: &Db, table: &str) -> Result<i64, Error> {
    let sql = format!("SELECT COUNT(*) FROM {}", quote_ident(table));
    let count: i64 = sqlx::query_scalar(&sql)
        .fetch_one(db.pool())
        .await
        .map_err(Error::from)?;
    Ok(count)
}

/// Case-insensitive partial match against `username`, `email`, and
/// `doctor_id`. The query is lower-cased once, wrapped in `%…%`,
/// and bound three times — no interpolation into the SQL text.
/// Results are ordered newest-first and windowed by `LIMIT` /
/// `OFFSET`, matching [`list_records`].
pub async fn search_records(
    db: &Db,
    table: &str,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<HashMap<String, String>>, Error> {
    let q = format!("%{}%", query.to_lowercase());
    let sql = format!(
        "SELECT * FROM {t} \
         WHERE LOWER(\"username\") LIKE ? \
            OR LOWER(\"email\") LIKE ? \
            OR LOWER(\"doctor_id\") LIKE ? \
         ORDER BY \"id\" DESC \
         LIMIT ? OFFSET ?",
        t = quote_ident(table),
    );
    let rows = sqlx::query(&sql)
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .bind(limit)
        .bind(offset)
        .fetch_all(db.pool())
        .await
        .map_err(Error::from)?;
    Ok(rows.iter().map(row_to_map).collect())
}

/// `SELECT COUNT(*)` counterpart of [`search_records`] — same
/// `WHERE` clause, no `ORDER BY` / `LIMIT`. Feeds the "Showing N of
/// M" label when the table is in search mode.
pub async fn count_search_records(db: &Db, table: &str, query: &str) -> Result<i64, Error> {
    let q = format!("%{}%", query.to_lowercase());
    let sql = format!(
        "SELECT COUNT(*) FROM {t} \
         WHERE LOWER(\"username\") LIKE ? \
            OR LOWER(\"email\") LIKE ? \
            OR LOWER(\"doctor_id\") LIKE ?",
        t = quote_ident(table),
    );
    let count: i64 = sqlx::query_scalar(&sql)
        .bind(&q)
        .bind(&q)
        .bind(&q)
        .fetch_one(db.pool())
        .await
        .map_err(Error::from)?;
    Ok(count)
}

// ---------------------------------------------------------------
// Combined filter + search
// ---------------------------------------------------------------

/// Run a windowed `SELECT` against `table` combining
/// metadata-driven filters and free-text search:
///
/// - `eq_filters`   → `column = ?` (one per entry, AND-combined).
///   Used for `Boolean` and `Select` filter types.
/// - `like_filters` → `LOWER(column) LIKE ?` with `%value%` (AND).
///   Used for `Exact` text filters.
/// - `query` (Some) → `(LOWER("username") LIKE ? OR LOWER("email")
///   LIKE ? OR LOWER("doctor_id") LIKE ?)` AND-combined with the
///   filter clauses above.
///
/// Column names go through [`quote_ident`]; values are bound
/// positionally — never interpolated into the SQL text. Sort +
/// window match [`list_records`] / [`search_records`]
/// (`ORDER BY "id" DESC LIMIT ? OFFSET ?`).
#[allow(clippy::too_many_arguments)]
pub async fn filter_records(
    db: &Db,
    table: &str,
    eq_filters: &HashMap<String, String>,
    like_filters: &HashMap<String, String>,
    query: Option<&str>,
    sort: Option<&str>,
    dir: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<HashMap<String, String>>, Error> {
    let (where_sql, binds) = build_filter_where(eq_filters, like_filters, query);
    // Validation lives in the layout layer (column must come from
    // metadata). Persistence trusts the inputs and only applies the
    // safe quoting + the ASC/DESC normalisation. A `None` sort
    // collapses to the default `ORDER BY "id" DESC`.
    let order_sql = match sort {
        Some(col) => {
            let direction = if matches!(dir, Some("desc")) {
                "DESC"
            } else {
                "ASC"
            };
            format!("ORDER BY {} {}", quote_ident(col), direction)
        }
        None => "ORDER BY \"id\" DESC".to_string(),
    };
    let sql = format!(
        "SELECT * FROM {t} WHERE {where_sql} {order_sql} LIMIT ? OFFSET ?",
        t = quote_ident(table),
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b.as_str());
    }
    q = q.bind(limit).bind(offset);
    let rows = q.fetch_all(db.pool()).await.map_err(Error::from)?;
    Ok(rows.iter().map(row_to_map).collect())
}

/// `SELECT COUNT(*)` counterpart of [`filter_records`]. Same
/// `WHERE` shape, no `ORDER BY` / `LIMIT` / `OFFSET`. Feeds the
/// page-count math.
pub async fn count_filtered_records(
    db: &Db,
    table: &str,
    eq_filters: &HashMap<String, String>,
    like_filters: &HashMap<String, String>,
    query: Option<&str>,
) -> Result<i64, Error> {
    let (where_sql, binds) = build_filter_where(eq_filters, like_filters, query);
    let sql = format!(
        "SELECT COUNT(*) FROM {t} WHERE {where_sql}",
        t = quote_ident(table),
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in &binds {
        q = q.bind(b.as_str());
    }
    let count = q.fetch_one(db.pool()).await.map_err(Error::from)?;
    Ok(count)
}

/// Shared `WHERE` clause builder for [`filter_records`] /
/// [`count_filtered_records`]. Returns the clause body (without the
/// leading `WHERE`) plus the ordered list of bind values. Filter
/// keys are sorted so the emitted SQL is deterministic across runs.
fn build_filter_where(
    eq_filters: &HashMap<String, String>,
    like_filters: &HashMap<String, String>,
    query: Option<&str>,
) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = vec!["1=1".to_string()];
    let mut binds: Vec<String> = Vec::new();

    let mut eq_keys: Vec<&String> = eq_filters.keys().collect();
    eq_keys.sort();
    for k in eq_keys {
        clauses.push(format!("{} = ?", quote_ident(k)));
        binds.push(eq_filters.get(k).cloned().unwrap_or_default());
    }

    let mut like_keys: Vec<&String> = like_filters.keys().collect();
    like_keys.sort();
    for k in like_keys {
        clauses.push(format!("LOWER({}) LIKE ?", quote_ident(k)));
        let v = like_filters
            .get(k)
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        binds.push(format!("%{v}%"));
    }

    let trimmed_query = query.map(str::trim).filter(|s| !s.is_empty());
    if let Some(q) = trimmed_query {
        clauses.push(
            "(LOWER(\"username\") LIKE ? OR LOWER(\"email\") LIKE ? OR LOWER(\"doctor_id\") LIKE ?)"
                .to_string(),
        );
        let q_param = format!("%{}%", q.to_lowercase());
        binds.push(q_param.clone());
        binds.push(q_param.clone());
        binds.push(q_param);
    }

    (clauses.join(" AND "), binds)
}

/// Flatten a SQLite row into a `column → string` map. Same coercion
/// chain as the single-row reader: try TEXT, then INTEGER, then
/// REAL; NULL or exotic types collapse to the empty string. Sharing
/// this helper keeps the two read paths in lockstep.
fn row_to_map(row: &sqlx::sqlite::SqliteRow) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for col in row.columns() {
        let name = col.name();
        let value: String = if let Ok(Some(s)) = row.try_get::<Option<String>, _>(name) {
            s
        } else if let Ok(Some(n)) = row.try_get::<Option<i64>, _>(name) {
            n.to_string()
        } else if let Ok(Some(f)) = row.try_get::<Option<f64>, _>(name) {
            f.to_string()
        } else {
            String::new()
        };
        out.insert(name.to_string(), value);
    }
    out
}
