//! Schema Contract runtime validator (Phase 14, commit 3).
//!
//! Compares a Rust [`ModelSchema`](crate::contract::ModelSchema)
//! contract against the actual PostgreSQL schema, surfacing every
//! drift point as a structured [`SchemaIssue`].
//!
//! # Architecture
//!
//! ```text
//! Rust ModelSchema (compile-time contract)   Postgres information_schema (live)
//!         │                                                │
//!         └────────────────► validator ◄───────────────────┘
//!                                │
//!                                ▼
//!                          SchemaReport
//!                          { errors, warnings, status }
//! ```
//!
//! The validator is **read-only**: it issues two `SELECT` statements
//! against `information_schema` per model and never mutates the
//! database. Safe to run on a production replica.
//!
//! # What it detects (per the spec)
//!
//! | Case               | Severity |
//! |--------------------|----------|
//! | Missing table      | ERROR    |
//! | Missing column     | ERROR    |
//! | Type mismatch      | ERROR    |
//! | Nullability drift  | ERROR    |
//! | Wrong primary key  | ERROR    |
//! | Extra DB column    | WARNING  |
//!
//! # What it does NOT do (yet)
//!
//! - No human-readable rendering (commit 4 wires this through
//!   `rustio doctor --check-schema --json`).
//! - No CHECK-constraint introspection, no column DEFAULT
//!   comparison, no FK validation. These can be added with new
//!   `IssueKind` variants without breaking the report shape.
//! - No caching. Each call re-queries `information_schema`.
//!
//! # Phase scope
//!
//! Commit 3 ships only the validator + types + PG-gated tests.
//! Nothing in `admin/`, `search/`, `migrations`, or `cli/`
//! references this module yet.

use std::collections::{HashMap, HashSet};

use crate::contract::{HasSchema, ModelSchema};
use crate::orm::Db;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Overall outcome of one model's validation pass. Computed from
/// the error/warning counts in [`SchemaReport`] and stored
/// explicitly so consumers don't have to recompute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    /// Every Rust column resolves correctly against the DB. No
    /// errors, no warnings.
    Ok,
    /// No errors, but at least one warning (e.g. an extra DB column
    /// not declared in the Rust contract). The app boots fine; the
    /// warning is informational.
    Warning,
    /// One or more errors. The app may still boot but at least one
    /// query against this model will fail at runtime — fix before
    /// shipping.
    Error,
}

/// Discrete failure mode for a single drift point.
///
/// `#[non_exhaustive]` so commit 4+ can add new kinds (CHECK
/// constraint mismatch, FK target missing, etc.) without breaking
/// downstream pattern matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IssueKind {
    /// The table named by the Rust contract does not exist in
    /// `information_schema.columns`.
    MissingTable,
    /// A Rust column has no corresponding DB column.
    MissingColumn,
    /// Rust column's `RustType` is not compatible with the DB
    /// column's `data_type` / `udt_name` (per
    /// [`RustType::is_compatible_with`](crate::contract::RustType::is_compatible_with)).
    TypeMismatch,
    /// Rust says nullable, DB says NOT NULL — or vice versa.
    NullabilityMismatch,
    /// The DB's primary key column(s) don't match the contract's
    /// declared `primary_key`.
    WrongPrimaryKey,
    /// A DB column exists that the Rust contract doesn't declare.
    /// Emitted as a warning — could be a deliberate audit column,
    /// could be drift.
    ExtraDbColumn,
    /// Introspection query failed (network, permissions, malformed
    /// schema). Reported as an error so the operator knows the
    /// validator couldn't actually verify anything.
    QueryFailed,
}

/// One drift point — column-scoped or table-scoped (`column = None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIssue {
    /// Column name, when the issue is column-scoped. `None` for
    /// table-scoped issues (`MissingTable`, `WrongPrimaryKey` when
    /// the PK columns disagree).
    pub column: Option<String>,
    /// Discrete kind — drives how a renderer / CI script reacts.
    pub kind: IssueKind,
    /// Human-readable single-line description. Templates may
    /// inline this verbatim.
    pub message: String,
    /// What the contract expected, formatted for display. `None`
    /// when expected can't be summarised in a short string.
    pub expected: Option<String>,
    /// What the DB actually has, formatted for display. `None`
    /// when actual is irrelevant (e.g. `MissingColumn` — by
    /// definition there's no DB-side value).
    pub actual: Option<String>,
}

/// Validation result for one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaReport {
    /// Table name from the Rust contract — matches the
    /// `ModelSchema::table` field of the input.
    pub table: String,
    /// Overall status. Cached version of "has errors → Error,
    /// has warnings only → Warning, else → Ok".
    pub status: ReportStatus,
    pub errors: Vec<SchemaIssue>,
    pub warnings: Vec<SchemaIssue>,
}

impl SchemaReport {
    /// Convenience: `true` when no errors AND no warnings.
    pub fn is_ok(&self) -> bool {
        matches!(self.status, ReportStatus::Ok)
    }

    /// Convenience: `true` when at least one error was recorded.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate the schema of a single model against the live DB.
///
/// The model's `T::SCHEMA` (from the [`HasSchema`](crate::contract::HasSchema)
/// trait — typically generated by `#[derive(RustioModel)]`) is the
/// expectation. The DB is the source of truth at runtime; any drift
/// is recorded in the returned [`SchemaReport`].
///
/// Read-only — issues two `SELECT` statements against
/// `information_schema` and exits.
pub async fn validate_schema<M: HasSchema>(db: &Db) -> SchemaReport {
    // Trait associated consts evaluate to a value; bind locally so
    // we can pass it by reference to the inner async helper.
    let schema = M::SCHEMA;
    validate_one(db, &schema).await
}

/// Validate every schema in the given slice. Sequential — running
/// in parallel would saturate the connection pool with introspection
/// queries that are usually cheap individually but multiply when
/// every model is checked at once.
///
/// The slice takes `&'static ModelSchema` references to keep the
/// caller's allocation strategy explicit. Typical usage:
///
/// ```ignore
/// static A: ModelSchema = MyModel::SCHEMA;     // requires const-evaluable ModelSchema (it is)
/// static B: ModelSchema = OtherModel::SCHEMA;
/// let reports = validate_all(&db, &[&A, &B]).await;
/// ```
pub async fn validate_all(
    db: &Db,
    schemas: &[&'static ModelSchema],
) -> Vec<SchemaReport> {
    let mut out = Vec::with_capacity(schemas.len());
    for s in schemas {
        out.push(validate_one(db, s).await);
    }
    out
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Inner validation pass — pure on `(Db, &ModelSchema) → SchemaReport`.
/// All public entry points funnel through this.
async fn validate_one(db: &Db, schema: &ModelSchema) -> SchemaReport {
    let mut errors: Vec<SchemaIssue> = Vec::new();
    let mut warnings: Vec<SchemaIssue> = Vec::new();
    let table = schema.table;

    // Step 1 — introspect columns. Failure is a hard stop; nothing
    // else can be checked without column metadata.
    let db_cols = match query_columns(db, table).await {
        Ok(cols) => cols,
        Err(e) => {
            errors.push(SchemaIssue {
                column: None,
                kind: IssueKind::QueryFailed,
                message: format!(
                    "could not query information_schema.columns for table `{table}`: {e}"
                ),
                expected: None,
                actual: None,
            });
            return finalize(table.to_string(), errors, warnings);
        }
    };

    // Empty result set ⇒ table doesn't exist (or is in a non-public
    // schema — out of scope for this commit). Treat as missing.
    if db_cols.is_empty() {
        errors.push(SchemaIssue {
            column: None,
            kind: IssueKind::MissingTable,
            message: format!(
                "table `{table}` declared in Rust contract not found in database (schema 'public')"
            ),
            expected: Some(table.to_string()),
            actual: None,
        });
        return finalize(table.to_string(), errors, warnings);
    }

    // Build name → column map for O(1) forward lookups.
    let db_map: HashMap<&str, &DbColumn> = db_cols
        .iter()
        .map(|c| (c.column_name.as_str(), c))
        .collect();

    // Step 2 — forward checks: every Rust column must be present in
    // the DB AND its type + nullability must agree.
    for rc in schema.columns {
        let dc = match db_map.get(rc.name) {
            Some(c) => c,
            None => {
                errors.push(SchemaIssue {
                    column: Some(rc.name.to_string()),
                    kind: IssueKind::MissingColumn,
                    message: format!(
                        "column `{table}.{}` declared in Rust contract not present in database",
                        rc.name
                    ),
                    expected: Some(rc.sql_decl.to_string()),
                    actual: None,
                });
                continue;
            }
        };

        // Type compatibility. PG exposes both the long form
        // (`data_type`, e.g. "timestamp with time zone") and the
        // short `udt_name` (e.g. "timestamptz") for the same column;
        // try both.
        let type_ok = rc.rust_type.is_compatible_with(&dc.data_type)
            || rc.rust_type.is_compatible_with(&dc.udt_name);
        if !type_ok {
            errors.push(SchemaIssue {
                column: Some(rc.name.to_string()),
                kind: IssueKind::TypeMismatch,
                message: format!(
                    "column `{table}.{}`: Rust type {:?} is not compatible with PG type `{}` (udt: `{}`)",
                    rc.name, rc.rust_type, dc.data_type, dc.udt_name
                ),
                expected: Some(format!(
                    "{:?} (compatible with one of {:?})",
                    rc.rust_type,
                    rc.rust_type.pg_compatible()
                )),
                actual: Some(dc.data_type.clone()),
            });
        }

        // Nullability. PG returns "YES" / "NO" as strings.
        let pg_nullable = dc.is_nullable.eq_ignore_ascii_case("YES");
        if pg_nullable != rc.nullable {
            errors.push(SchemaIssue {
                column: Some(rc.name.to_string()),
                kind: IssueKind::NullabilityMismatch,
                message: format!(
                    "column `{table}.{}`: contract says nullable={}, DB says nullable={}",
                    rc.name, rc.nullable, pg_nullable
                ),
                expected: Some(format!("nullable = {}", rc.nullable)),
                actual: Some(format!("nullable = {pg_nullable}")),
            });
        }
    }

    // Step 3 — primary-key check. Must exactly equal the contract's
    // single declared PK. Composite PKs are not supported in
    // commit 1's contract surface.
    match query_primary_key(db, table).await {
        Ok(pk_cols) => {
            let mismatch = pk_cols.len() != 1 || pk_cols[0] != schema.primary_key;
            if mismatch {
                errors.push(SchemaIssue {
                    column: Some(schema.primary_key.to_string()),
                    kind: IssueKind::WrongPrimaryKey,
                    message: format!(
                        "primary key drift on `{table}`: contract expects `{}`, DB has [{}]",
                        schema.primary_key,
                        pk_cols.join(", ")
                    ),
                    expected: Some(schema.primary_key.to_string()),
                    actual: Some(if pk_cols.is_empty() {
                        "<no primary key>".to_string()
                    } else {
                        pk_cols.join(", ")
                    }),
                });
            }
        }
        Err(e) => {
            errors.push(SchemaIssue {
                column: None,
                kind: IssueKind::QueryFailed,
                message: format!(
                    "could not query primary-key constraints for `{table}`: {e}"
                ),
                expected: None,
                actual: None,
            });
        }
    }

    // Step 4 — reverse check: DB columns not in the Rust contract.
    // Warning only; could be a deliberate audit column added by an
    // out-of-band migration.
    let rust_names: HashSet<&str> = schema.columns.iter().map(|c| c.name).collect();
    for dc in &db_cols {
        if !rust_names.contains(dc.column_name.as_str()) {
            warnings.push(SchemaIssue {
                column: Some(dc.column_name.clone()),
                kind: IssueKind::ExtraDbColumn,
                message: format!(
                    "DB column `{table}.{}` not declared in Rust contract (could be deliberate)",
                    dc.column_name
                ),
                expected: None,
                actual: Some(dc.data_type.clone()),
            });
        }
    }

    finalize(table.to_string(), errors, warnings)
}

fn finalize(
    table: String,
    errors: Vec<SchemaIssue>,
    warnings: Vec<SchemaIssue>,
) -> SchemaReport {
    let status = if !errors.is_empty() {
        ReportStatus::Error
    } else if !warnings.is_empty() {
        ReportStatus::Warning
    } else {
        ReportStatus::Ok
    };
    SchemaReport {
        table,
        status,
        errors,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Postgres introspection
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct DbColumn {
    column_name: String,
    data_type: String,
    udt_name: String,
    is_nullable: String, // "YES" / "NO" per ISO SQL information_schema spec.
}

/// Query `information_schema.columns` for a single table in the
/// `public` schema. Returns rows in declaration (ordinal) order.
async fn query_columns(db: &Db, table: &str) -> Result<Vec<DbColumn>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT column_name, data_type, udt_name, is_nullable
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(DbColumn {
            column_name: row.try_get("column_name")?,
            data_type: row.try_get("data_type")?,
            udt_name: row.try_get("udt_name")?,
            is_nullable: row.try_get("is_nullable")?,
        });
    }
    Ok(out)
}

/// Query the primary-key columns of a table. Returns names in
/// `ordinal_position` order so composite keys (if/when supported)
/// preserve their declared sequence.
async fn query_primary_key(db: &Db, table: &str) -> Result<Vec<String>, sqlx::Error> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT kcu.column_name
         FROM information_schema.table_constraints tc
         JOIN information_schema.key_column_usage kcu
           ON tc.constraint_name = kcu.constraint_name
          AND tc.table_schema    = kcu.table_schema
         WHERE tc.constraint_type = 'PRIMARY KEY'
           AND tc.table_schema    = 'public'
           AND tc.table_name      = $1
         ORDER BY kcu.ordinal_position",
    )
    .bind(table)
    .fetch_all(db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.try_get::<String, _>("column_name")?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Unit tests — pure (no DB)
// ---------------------------------------------------------------------------
//
// PG-gated end-to-end tests live in `tests/contract_validator_pg.rs`
// and only run under `RUSTIO_TEST_DB=1 cargo test --ignored`.
// The unit tests here exercise the small pure helpers
// (`finalize`, `ReportStatus` derivation) that don't need a DB.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_no_issues_is_ok() {
        let r = finalize("t".into(), vec![], vec![]);
        assert_eq!(r.status, ReportStatus::Ok);
        assert!(r.is_ok());
        assert!(!r.has_errors());
    }

    #[test]
    fn finalize_only_warnings_is_warning() {
        let warn = SchemaIssue {
            column: Some("extra".into()),
            kind: IssueKind::ExtraDbColumn,
            message: "x".into(),
            expected: None,
            actual: None,
        };
        let r = finalize("t".into(), vec![], vec![warn]);
        assert_eq!(r.status, ReportStatus::Warning);
        assert!(!r.is_ok());
        assert!(!r.has_errors());
    }

    #[test]
    fn finalize_any_error_is_error() {
        let err = SchemaIssue {
            column: Some("c".into()),
            kind: IssueKind::TypeMismatch,
            message: "x".into(),
            expected: None,
            actual: None,
        };
        // Even with warnings present, a single error promotes to Error.
        let warn = SchemaIssue {
            column: None,
            kind: IssueKind::ExtraDbColumn,
            message: "y".into(),
            expected: None,
            actual: None,
        };
        let r = finalize("t".into(), vec![err], vec![warn]);
        assert_eq!(r.status, ReportStatus::Error);
        assert!(r.has_errors());
    }
}
