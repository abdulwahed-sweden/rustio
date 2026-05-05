//! Project-side subprocess hook for `rustio doctor --check-schema`
//! (Phase 14, commit 4).
//!
//! # The big picture
//!
//! The `rustio` CLI binary doesn't know what models a project has
//! registered — registration happens in the project's own `main.rs`
//! via `Admin::new().model::<T>()` calls. To validate the project's
//! schemas, the CLI spawns the project binary as a subprocess with a
//! magic flag (`--rustio-doctor-schema-check`); the project binary
//! short-circuits its normal startup, runs the validator over its
//! schemas, prints the result, and exits.
//!
//! This module is the project-side half of that contract. The CLI
//! lives in `rustio-cli/src/doctor.rs`; nothing in this module
//! depends on the CLI.
//!
//! # Wiring this into a project's `main.rs`
//!
//! Place the call **after** DB connection but **before** server
//! startup:
//!
//! ```ignore
//! let db = Db::connect(&db_url).await?;
//! if rustio_core::contract_doctor::maybe_handle_subprocess(
//!     &db,
//!     &[Project::SCHEMA, Order::SCHEMA],
//! ).await {
//!     return Ok(());
//! }
//! // ... normal admin setup, server start, etc.
//! ```
//!
//! Each `T::SCHEMA` is a `const ModelSchema` — calling site evaluates
//! the const into the array literal. The function takes `&[ModelSchema]`
//! by reference; ownership of the slice's elements stays with the
//! caller.
//!
//! # What it does
//!
//! 1. Reads `std::env::args()` for `--rustio-doctor-schema-check`.
//! 2. If absent, returns `false` immediately — caller proceeds with
//!    normal startup.
//! 3. If present, runs `validate_all` over the supplied schemas.
//! 4. Prints output (JSON if `--json` is also present, human-readable
//!    otherwise) to stdout.
//! 5. Exits the process with code 0 (no errors) or 1 (any errors).
//!
//! # Phase scope
//!
//! Commit 4 ships the helper + its CLI subprocess driver. Nothing
//! in `admin/`, `search/`, `migrations.rs`, or the contract types
//! themselves is changed.

use crate::contract::ModelSchema;
use crate::contract_validator::{validate_all, IssueKind, ReportStatus, SchemaReport};
use crate::orm::Db;

// ---------------------------------------------------------------------------
// Magic flag
// ---------------------------------------------------------------------------

/// The argv flag the CLI passes to the project binary. Long, prefixed
/// with `--rustio-doctor-` so it can't collide with project-defined
/// flags.
pub const SCHEMA_CHECK_FLAG: &str = "--rustio-doctor-schema-check";

/// Optional companion flag that asks for JSON output instead of the
/// human-readable renderer. The CLI always passes this when
/// subprocessing — the CLI's `--json` decides whether to pass-through
/// the JSON or pretty-print, but the project always emits JSON when
/// running headless.
pub const JSON_FLAG: &str = "--json";

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// If the current process was invoked with `--rustio-doctor-schema-check`,
/// run the validator and exit. Returns `true` only after exiting (so
/// the return value is conceptually unreachable — but typed for
/// clarity at call sites). Returns `false` immediately when the flag
/// isn't set, so the caller's normal startup proceeds.
///
/// ```ignore
/// // In project main.rs:
/// if rustio_core::contract_doctor::maybe_handle_subprocess(
///     &db,
///     &[Project::SCHEMA],
/// ).await {
///     return Ok(());
/// }
/// ```
pub async fn maybe_handle_subprocess(db: &Db, schemas: &[ModelSchema]) -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == SCHEMA_CHECK_FLAG) {
        return false;
    }
    let json_mode = args.iter().any(|a| a == JSON_FLAG);

    // The validator's `validate_all` requires `&[&'static ModelSchema]`.
    // We accept owned/borrowed slices for caller ergonomics, then leak
    // each schema one-shot. Acceptable: this function exits the
    // process, so the leak is transient.
    let leaked: Vec<&'static ModelSchema> = schemas
        .iter()
        .cloned()
        .map(|s| &*Box::leak(Box::new(s)))
        .collect();

    let reports = validate_all(db, &leaked).await;
    let exit_code = exit_code_for(&reports);

    if json_mode {
        // Always one line of canonical JSON to stdout — easy for the
        // CLI to capture and pass through.
        let doc = reports_to_json(&reports);
        match serde_json::to_string(&doc) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("contract_doctor: failed to serialise JSON: {e}"),
        }
    } else {
        // Human-readable: one line per table with status symbol +
        // first issue (if any). The CLI also has a renderer, but
        // this path runs when a developer invokes the project
        // binary directly (e.g. for debugging without the CLI).
        for r in &reports {
            print_human_line(r);
        }
    }

    std::process::exit(exit_code);
}

// ---------------------------------------------------------------------------
// Pure helpers — exposed crate-internal so the CLI can reuse them.
// ---------------------------------------------------------------------------

/// Decide the process exit code from the report set. Any error in
/// any report → exit 1; otherwise exit 0. Warnings alone never
/// fail the check (consistent with `rustio doctor`'s
/// READY (DEGRADED) behaviour).
pub(crate) fn exit_code_for(reports: &[SchemaReport]) -> i32 {
    if reports.iter().any(|r| r.has_errors()) {
        1
    } else {
        0
    }
}

/// Top-level overall status string — derived from per-report
/// statuses. Mirrors the doctor's vocabulary: `"ok"` /
/// `"warning"` / `"error"`. Lowercase for JSON.
pub(crate) fn overall_status_str(reports: &[SchemaReport]) -> &'static str {
    if reports.iter().any(|r| r.has_errors()) {
        "error"
    } else if reports.iter().any(|r| !r.warnings.is_empty()) {
        "warning"
    } else {
        "ok"
    }
}

/// Build the canonical JSON document the CLI consumes. Schema:
///
/// ```json
/// {
///   "status": "ok" | "warning" | "error",
///   "tables": [
///     {
///       "table": "...",
///       "status": "...",
///       "errors":   [{ column?, kind, message, expected?, actual? }, ...],
///       "warnings": [...same...]
///     },
///     ...
///   ]
/// }
/// ```
///
/// Manual serialisation (no `Serialize` derive) — touching the
/// `SchemaReport` / `SchemaIssue` types in `contract_validator.rs`
/// would violate commit 4's "don't touch the validator" rule. Keys
/// are stable; new ones can be added without breaking older clients.
pub(crate) fn reports_to_json(reports: &[SchemaReport]) -> serde_json::Value {
    serde_json::json!({
        "status": overall_status_str(reports),
        "tables": reports.iter().map(report_to_json).collect::<Vec<_>>(),
    })
}

fn report_to_json(r: &SchemaReport) -> serde_json::Value {
    serde_json::json!({
        "table": r.table,
        "status": status_str(r.status),
        "errors":   r.errors.iter().map(issue_to_json).collect::<Vec<_>>(),
        "warnings": r.warnings.iter().map(issue_to_json).collect::<Vec<_>>(),
    })
}

fn issue_to_json(i: &crate::contract_validator::SchemaIssue) -> serde_json::Value {
    serde_json::json!({
        "column":   i.column,
        "kind":     issue_kind_str(i.kind),
        "message":  i.message,
        "expected": i.expected,
        "actual":   i.actual,
    })
}

fn status_str(s: ReportStatus) -> &'static str {
    match s {
        ReportStatus::Ok => "ok",
        ReportStatus::Warning => "warning",
        ReportStatus::Error => "error",
    }
}

/// Stable lowercase identifier per `IssueKind` variant. Used in JSON
/// so a CI script doing `jq '.tables[].errors[].kind'` gets a
/// predictable string.
fn issue_kind_str(k: IssueKind) -> &'static str {
    match k {
        IssueKind::MissingTable => "missing_table",
        IssueKind::MissingColumn => "missing_column",
        IssueKind::TypeMismatch => "type_mismatch",
        IssueKind::NullabilityMismatch => "nullability_mismatch",
        IssueKind::WrongPrimaryKey => "wrong_primary_key",
        IssueKind::ExtraDbColumn => "extra_db_column",
        IssueKind::QueryFailed => "query_failed",
    }
}

/// Render one report as a single human-readable line:
///
/// ```text
/// ✓ projects
/// ✗ invoices (missing_column: column `invoices.amount` ...)
/// ⚠ clients (extra_db_column: column `clients.legacy_code` ...)
/// ```
fn print_human_line(r: &SchemaReport) {
    match r.status {
        ReportStatus::Ok => println!("✓ {}", r.table),
        ReportStatus::Warning => {
            let first = r
                .warnings
                .first()
                .map(|i| format!("{}: {}", issue_kind_str(i.kind), i.message))
                .unwrap_or_else(|| "warning".into());
            println!("⚠ {} ({first})", r.table);
        }
        ReportStatus::Error => {
            let first = r
                .errors
                .first()
                .map(|i| format!("{}: {}", issue_kind_str(i.kind), i.message))
                .unwrap_or_else(|| "error".into());
            println!("✗ {} ({first})", r.table);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_validator::{IssueKind, ReportStatus, SchemaIssue, SchemaReport};

    fn ok_report(table: &str) -> SchemaReport {
        SchemaReport {
            table: table.into(),
            status: ReportStatus::Ok,
            errors: vec![],
            warnings: vec![],
        }
    }
    fn warn_report(table: &str, kind: IssueKind, msg: &str) -> SchemaReport {
        SchemaReport {
            table: table.into(),
            status: ReportStatus::Warning,
            errors: vec![],
            warnings: vec![SchemaIssue {
                column: Some("c".into()),
                kind,
                message: msg.into(),
                expected: None,
                actual: None,
            }],
        }
    }
    fn err_report(table: &str, kind: IssueKind, msg: &str) -> SchemaReport {
        SchemaReport {
            table: table.into(),
            status: ReportStatus::Error,
            errors: vec![SchemaIssue {
                column: Some("c".into()),
                kind,
                message: msg.into(),
                expected: Some("expected".into()),
                actual: Some("actual".into()),
            }],
            warnings: vec![],
        }
    }

    // ----- Exit-code derivation -----

    #[test]
    fn exit_code_zero_on_empty_input() {
        assert_eq!(exit_code_for(&[]), 0);
    }

    #[test]
    fn exit_code_zero_on_all_ok() {
        assert_eq!(exit_code_for(&[ok_report("a"), ok_report("b")]), 0);
    }

    #[test]
    fn exit_code_zero_on_warnings_only() {
        // Spec parity with `rustio doctor`: warnings don't fail the
        // check. Only errors do.
        assert_eq!(
            exit_code_for(&[warn_report("a", IssueKind::ExtraDbColumn, "x")]),
            0
        );
    }

    #[test]
    fn exit_code_one_on_any_error() {
        assert_eq!(
            exit_code_for(&[
                ok_report("a"),
                warn_report("b", IssueKind::ExtraDbColumn, "x"),
                err_report("c", IssueKind::MissingColumn, "y"),
            ]),
            1
        );
    }

    // ----- Overall status string -----

    #[test]
    fn overall_status_ok_when_all_clean() {
        assert_eq!(overall_status_str(&[ok_report("a")]), "ok");
        assert_eq!(overall_status_str(&[]), "ok");
    }

    #[test]
    fn overall_status_warning_when_warnings_only() {
        assert_eq!(
            overall_status_str(&[
                ok_report("a"),
                warn_report("b", IssueKind::ExtraDbColumn, "x"),
            ]),
            "warning"
        );
    }

    #[test]
    fn overall_status_error_takes_priority_over_warnings() {
        assert_eq!(
            overall_status_str(&[
                warn_report("a", IssueKind::ExtraDbColumn, "x"),
                err_report("b", IssueKind::TypeMismatch, "y"),
            ]),
            "error"
        );
    }

    // ----- JSON shape -----

    #[test]
    fn json_top_level_has_status_and_tables() {
        let doc = reports_to_json(&[ok_report("projects")]);
        let obj = doc.as_object().expect("top-level must be an object");
        assert!(obj.contains_key("status"));
        assert!(obj.contains_key("tables"));
        assert_eq!(obj.len(), 2, "no extra keys at top level");
    }

    #[test]
    fn json_table_entries_have_required_fields() {
        let doc = reports_to_json(&[err_report("invoices", IssueKind::MissingColumn, "msg")]);
        let table = &doc["tables"][0];
        let obj = table.as_object().expect("table entry must be object");
        for k in ["table", "status", "errors", "warnings"] {
            assert!(obj.contains_key(k), "missing key: {k}");
        }
        assert_eq!(obj.len(), 4, "no extra keys per table");
    }

    #[test]
    fn json_issue_has_all_five_fields() {
        let doc = reports_to_json(&[err_report("t", IssueKind::TypeMismatch, "m")]);
        let issue = &doc["tables"][0]["errors"][0];
        let obj = issue.as_object().expect("issue must be object");
        for k in ["column", "kind", "message", "expected", "actual"] {
            assert!(obj.contains_key(k), "missing issue key: {k}");
        }
        assert_eq!(obj.len(), 5);
    }

    #[test]
    fn json_status_is_lowercase_string() {
        let doc = reports_to_json(&[
            ok_report("a"),
            warn_report("b", IssueKind::ExtraDbColumn, "x"),
        ]);
        assert_eq!(doc["status"], "warning");
        assert_eq!(doc["tables"][0]["status"], "ok");
        assert_eq!(doc["tables"][1]["status"], "warning");
    }

    #[test]
    fn json_kind_uses_stable_snake_case() {
        // The IssueKind serialisation is the contract a CI script
        // does `jq '.tables[].errors[] | select(.kind=="missing_column")'`
        // against. Lock the strings.
        for (kind, expected) in [
            (IssueKind::MissingTable, "missing_table"),
            (IssueKind::MissingColumn, "missing_column"),
            (IssueKind::TypeMismatch, "type_mismatch"),
            (IssueKind::NullabilityMismatch, "nullability_mismatch"),
            (IssueKind::WrongPrimaryKey, "wrong_primary_key"),
            (IssueKind::ExtraDbColumn, "extra_db_column"),
            (IssueKind::QueryFailed, "query_failed"),
        ] {
            assert_eq!(issue_kind_str(kind), expected, "kind {kind:?}");
        }
    }

    #[test]
    fn json_round_trips_through_serde_json() {
        let doc = reports_to_json(&[
            ok_report("projects"),
            warn_report("clients", IssueKind::ExtraDbColumn, "extra"),
            err_report("invoices", IssueKind::MissingColumn, "missing"),
        ]);
        let s = serde_json::to_string(&doc).expect("serialise");
        let parsed: serde_json::Value = serde_json::from_str(&s).expect("round-trip");
        assert_eq!(parsed, doc);
    }

    #[test]
    fn json_overall_status_reflects_table_mix() {
        let doc = reports_to_json(&[
            ok_report("a"),
            err_report("b", IssueKind::MissingColumn, "x"),
        ]);
        // Mixed table statuses → overall "error".
        assert_eq!(doc["status"], "error");
    }

    // ----- Magic flag constants are stable -----

    #[test]
    fn magic_flag_strings_are_stable() {
        // The CLI side hardcodes the same string literals; if either
        // side drifts the subprocess goes silent. This test is a
        // tripwire for that.
        assert_eq!(SCHEMA_CHECK_FLAG, "--rustio-doctor-schema-check");
        assert_eq!(JSON_FLAG, "--json");
    }
}
