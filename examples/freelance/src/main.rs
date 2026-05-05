//! Freelance — Phase 14 end-to-end demo.
//!
//! Pipeline (top to bottom):
//!
//! ```text
//!  Models (#[derive(RustioModel)])
//!         │
//!         ▼
//!  T::SCHEMA  ───► all_schemas()
//!         │
//!         ▼
//!  contract_doctor::maybe_handle_subprocess
//!  (intercepts when invoked with --rustio-doctor-schema-check)
//!         │
//!         ▼
//!  contract_validator::validate_schema::<T>(&db)
//!         │
//!         ├─► search::from_schema::enable_search::<T>(&db)
//!         │       │
//!         │       └─► SearchEnablement::{NotSearchable | Disabled | Enabled}
//!         │
//!         └─► admin::from_schema::bridged_fields_from_schema(&schema)
//!                 │
//!                 └─► one BridgedField per column (admin metadata)
//! ```
//!
//! Quick start:
//!
//! ```sh
//! createdb rustio_freelance
//! export DATABASE_URL=postgres://postgres:dev@localhost/rustio_freelance
//! cargo run
//! ```
//!
//! Doctor subprocess:
//!
//! ```sh
//! rustio doctor --check-schema           # human-readable
//! rustio doctor --check-schema --json    # CI-friendly
//! ```

use std::error::Error;

use rustio_core::admin::from_schema as admin_bridge;
use rustio_core::contract::HasSchema;
use rustio_core::contract_doctor;
use rustio_core::contract_validator;
use rustio_core::orm::Db;
use rustio_core::search::from_schema as search_bridge;

use freelance::{all_schemas, Client, Invoice, Project};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // ----- Step 1 — Connect ------------------------------------------------

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:dev@localhost/rustio_freelance".into());
    log::info!("connecting to {}", redacted(&db_url));
    let db = Db::connect(&db_url).await?;

    // ----- Step 2 — Doctor subprocess hook --------------------------------
    //
    // When invoked with `--rustio-doctor-schema-check`, the helper
    // validates every schema, prints the result (JSON when
    // `--json` is also present), and exits the process before any
    // server work. The CLI's `rustio doctor --check-schema` flag
    // spawns this binary with that magic flag, so the same `cargo
    // run` command serves both the demo and the doctor's check.

    let schemas = all_schemas();
    if contract_doctor::maybe_handle_subprocess(&db, &schemas).await {
        // The hook prints + exits internally; the unreachable
        // return here is for the type checker only.
        return Ok(());
    }

    // ----- Step 3 — Schema reports + bridge demonstrations -----------------
    //
    // For each model: validate against the live DB, derive the
    // search config (gated on the validator's verdict), and dump
    // the admin bridge's per-column metadata.

    log::info!("--- Phase 14 pipeline demonstration ---");
    // String literals here are `&'static str` — required by
    // `ModelSchema::with_search_index`'s `&'static str` bound.
    demonstrate::<Client>(&db, "clients").await;
    demonstrate::<Project>(&db, "projects").await;
    demonstrate::<Invoice>(&db, "invoices").await;

    // ----- Step 4 — Server bind (smoke) -----------------------------------
    //
    // The spec's required flow ends with `start_server()`. A full
    // admin server requires `AdminModel` impls — those are
    // intentionally absent from this example (Phase 14 commit 7
    // forbids manual admin config), so the "server" here is a
    // smoke step: log that bridges are ready and exit. Setting
    // `RUSTIO_FREELANCE_HOLD=1` keeps the process alive so an
    // operator can attach a debugger; otherwise we exit clean.

    log::info!("freelance bridges initialised — pipeline complete.");
    if std::env::var("RUSTIO_FREELANCE_HOLD").ok().as_deref() == Some("1") {
        log::info!("RUSTIO_FREELANCE_HOLD=1 — sleeping forever (Ctrl+C to exit).");
        std::future::pending::<()>().await;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Demonstration helper — one per model
// ---------------------------------------------------------------------------

async fn demonstrate<M: HasSchema>(db: &Db, label: &'static str) {
    log::info!("\n=== {label} ===");

    // Step A — validator
    let report = contract_validator::validate_schema::<M>(db).await;
    log::info!(
        "validator: status={:?} errors={} warnings={}",
        report.status,
        report.errors.len(),
        report.warnings.len()
    );
    for e in &report.errors {
        log::warn!("  [error] {}: {}", kind_str(e.kind), e.message);
    }
    for w in &report.warnings {
        log::info!("  [warn]  {}: {}", kind_str(w.kind), w.message);
    }

    // Step B — search bridge (gated on validator verdict).
    // Override `search_index` because the macro doesn't yet emit
    // it (see lib.rs::all_schemas for the rationale).
    let schema = M::SCHEMA.with_search_index(label);
    let outcome = search_bridge::enablement_from(&schema, report.clone());
    if outcome.is_enabled() {
        let cfg = outcome.config().unwrap();
        log::info!("search:    enabled (index={})", cfg.index);
        log::info!("           searchable = {:?}", cfg.searchable_attributes);
        log::info!("           filterable = {:?}", cfg.filterable_attributes);
        log::info!("           sortable   = {:?}", cfg.sortable_attributes);
    } else {
        match outcome {
            search_bridge::SearchEnablement::Disabled { .. } => {
                log::warn!("search:    disabled (validator returned errors — fail-safe)");
            }
            search_bridge::SearchEnablement::NotSearchable => {
                log::info!("search:    not searchable (no search_index in schema)");
            }
            search_bridge::SearchEnablement::Enabled { .. } => unreachable!(),
        }
    }

    // Step C — admin bridge (purely derived, not gated).
    let bridged = admin_bridge::bridged_fields_from_schema(&schema);
    log::info!("admin:     {} fields auto-generated", bridged.len());
    for b in &bridged {
        log::info!(
            "           - {:<14} label={:<16} editable={} flags={}",
            b.field.name,
            format!("{:?}", b.field.label),
            b.field.editable,
            flag_summary(b),
        );
    }
}

// ---------------------------------------------------------------------------
// Pretty-printing helpers
// ---------------------------------------------------------------------------

fn flag_summary(b: &admin_bridge::BridgedField) -> String {
    let mut bits = Vec::new();
    if b.primary_key {
        bits.push("pk");
    }
    if b.searchable {
        bits.push("searchable");
    }
    if b.filterable {
        bits.push("filterable");
    }
    if b.sortable {
        bits.push("sortable");
    }
    if b.readonly {
        bits.push("readonly");
    }
    if let Some(w) = b.widget {
        bits.push(w);
    }
    if bits.is_empty() {
        "(none)".into()
    } else {
        bits.join(",")
    }
}

fn kind_str(k: rustio_core::contract_validator::IssueKind) -> &'static str {
    use rustio_core::contract_validator::IssueKind::*;
    // `IssueKind` is `#[non_exhaustive]` cross-crate; the wildcard
    // arm covers any future variants that ship without a labelled
    // mapping here.
    match k {
        MissingTable => "missing_table",
        MissingColumn => "missing_column",
        TypeMismatch => "type_mismatch",
        NullabilityMismatch => "nullability_mismatch",
        WrongPrimaryKey => "wrong_primary_key",
        ExtraDbColumn => "extra_db_column",
        QueryFailed => "query_failed",
        _ => "unknown",
    }
}

/// Hide credentials from the connection-string log line.
/// `postgres://user:secret@host/db` → `postgres://user:***@host/db`.
fn redacted(url: &str) -> String {
    if let Some(at) = url.rfind('@') {
        if let Some(scheme_end) = url.find("://") {
            let prefix_end = scheme_end + 3;
            if let Some(colon) = url[prefix_end..at].find(':') {
                let cut = prefix_end + colon + 1;
                return format!("{}***{}", &url[..cut], &url[at..]);
            }
        }
    }
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::redacted;

    #[test]
    fn redacted_masks_password_only() {
        let url = "postgres://postgres:supersecret@localhost:5432/rustio_freelance";
        let out = redacted(url);
        assert!(!out.contains("supersecret"));
        assert!(out.contains("postgres://postgres:***"));
        assert!(out.contains("@localhost:5432/rustio_freelance"));
    }

    #[test]
    fn redacted_passes_through_when_no_password() {
        let url = "postgres://localhost/rustio_freelance";
        assert_eq!(redacted(url), url);
    }
}
