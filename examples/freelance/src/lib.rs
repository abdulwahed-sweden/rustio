//! Freelance — RustIO Phase 14 showcase.
//!
//! Three realistic models — `Client`, `Project`, `Invoice` —
//! defined entirely with `#[derive(RustioModel)]`. No manual
//! `AdminModel` impl, no manual `Searchable` impl, no
//! `AdminEntry` builder calls. Every field flag flows through
//! to admin and search via the bridges introduced in
//! Phase 14 commits 5 and 6.
//!
//! See `README.md` for the full pipeline diagram and a
//! flag-by-flag explanation of how the `#[rustio(...)]`
//! attributes drive the framework's behaviour.

use chrono::{DateTime, Utc};
use rustio_core::contract::{HasSchema, ModelSchema};
use rustio_macros::RustioModel;

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// A freelance client. Trivial CRM target: name + contact email,
/// plus the usual audit timestamp.
#[derive(Debug, Clone, RustioModel)]
#[rustio(table = "clients")]
pub struct Client {
    #[rustio(sql = "BIGSERIAL PRIMARY KEY", readonly)]
    pub id: i64,

    /// Free-text full-text-search target.
    #[rustio(sql = "TEXT NOT NULL", searchable, sortable, label = "Client name")]
    pub name: String,

    /// Filterable so the admin UI's filter sidebar can offer an
    /// "email contains" / "email = ..." filter.
    #[rustio(sql = "TEXT NOT NULL", filterable)]
    pub email: String,

    #[rustio(sql = "TIMESTAMPTZ NOT NULL DEFAULT NOW()", readonly, sortable)]
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// A piece of work for a client. `client_id` references
/// `clients(id)`; the macro-side `references` attribute is
/// captured but the FK enforcement happens in the migration's
/// SQL DDL.
///
/// Money is stored as `i64` cents per the framework's Type
/// Rule #3 (the alternative is `rust_decimal::Decimal`, which
/// would pull in an extra dependency this example deliberately
/// avoids — the cents-as-`i64` form is fully supported by the
/// contract for `NUMERIC` columns).
#[derive(Debug, Clone, RustioModel)]
#[rustio(table = "projects")]
pub struct Project {
    #[rustio(sql = "BIGSERIAL PRIMARY KEY", readonly)]
    pub id: i64,

    #[rustio(sql = "TEXT NOT NULL", searchable, sortable)]
    pub name: String,

    /// Free-text body; nullable to allow stub projects.
    #[rustio(sql = "TEXT", searchable, widget = "textarea")]
    pub description: Option<String>,

    /// FK to `clients(id)`. The `references` attribute is parsed
    /// by the macro and travels with the schema; the actual FK
    /// constraint lives in the migration DDL where Postgres can
    /// enforce it.
    #[rustio(
        sql = "BIGINT NOT NULL",
        filterable,
        sortable,
        references = "clients(id)"
    )]
    pub client_id: i64,

    /// Budget in cents (NUMERIC-compatible per Type Rule #3).
    /// Nullable: a project may be ranged-priced before the
    /// contract is signed.
    #[rustio(sql = "BIGINT", sortable, label = "Budget (cents)")]
    pub budget_cents: Option<i64>,

    #[rustio(sql = "TIMESTAMPTZ NOT NULL DEFAULT NOW()", readonly, sortable)]
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Invoice
// ---------------------------------------------------------------------------

/// An issued invoice against a project. `paid` flips when the
/// payment lands; the dual `issued_at` / `created_at` fields
/// capture when the operator entered the row vs. when the
/// invoice was actually issued (these can differ for
/// retroactive entry).
#[derive(Debug, Clone, RustioModel)]
#[rustio(table = "invoices")]
pub struct Invoice {
    #[rustio(sql = "BIGSERIAL PRIMARY KEY", readonly)]
    pub id: i64,

    #[rustio(
        sql = "BIGINT NOT NULL",
        filterable,
        sortable,
        references = "projects(id)"
    )]
    pub project_id: i64,

    /// Amount in cents (Type Rule #3 — `i64` cents over a
    /// `NUMERIC`-compatible column).
    #[rustio(sql = "BIGINT NOT NULL", sortable, label = "Amount (cents)")]
    pub amount_cents: i64,

    #[rustio(sql = "BOOLEAN NOT NULL DEFAULT FALSE", filterable, sortable)]
    pub paid: bool,

    #[rustio(sql = "TIMESTAMPTZ NOT NULL", filterable, sortable)]
    pub issued_at: DateTime<Utc>,

    #[rustio(sql = "TIMESTAMPTZ NOT NULL DEFAULT NOW()", readonly)]
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Schema registry helpers
// ---------------------------------------------------------------------------

/// All schemas the example exposes.
///
/// Phase 14 / commit 8: `search_index` is now derived
/// automatically by `#[derive(RustioModel)]` whenever any
/// column declares `searchable`. The previous commit-7
/// workaround (per-model `with_search_index(...)` calls) is
/// gone — the macro emits the right shape directly.
pub fn all_schemas() -> Vec<ModelSchema> {
    vec![Client::SCHEMA, Project::SCHEMA, Invoice::SCHEMA]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rustio_core::admin::from_schema as admin_bridge;
    use rustio_core::contract::HasSchema;
    use rustio_core::contract_validator::{ReportStatus, SchemaIssue, SchemaReport};
    use rustio_core::search::from_schema as search_bridge;

    // ----- Schema compiles ------------------------------------------------

    /// The three derives compile and produce non-empty schemas.
    /// This is a smoke test — if the macro broke any of the
    /// `#[rustio(...)]` attribute parses, the crate wouldn't link
    /// and this test would never run; reaching the assertions
    /// proves the derive ran end-to-end.
    #[test]
    fn schemas_compile_and_carry_columns() {
        assert_eq!(Client::SCHEMA.table, "clients");
        assert_eq!(Project::SCHEMA.table, "projects");
        assert_eq!(Invoice::SCHEMA.table, "invoices");

        assert!(!Client::SCHEMA.columns.is_empty());
        assert!(!Project::SCHEMA.columns.is_empty());
        assert!(!Invoice::SCHEMA.columns.is_empty());

        // Primary key is `id` for all three.
        assert_eq!(Client::SCHEMA.primary_key, "id");
        assert_eq!(Project::SCHEMA.primary_key, "id");
        assert_eq!(Invoice::SCHEMA.primary_key, "id");
    }

    /// `all_schemas()` returns three schemas. Phase 14 /
    /// commit 8: `search_index` is auto-derived by the macro
    /// from the presence of any `searchable` flag — so models
    /// with at least one searchable column carry
    /// `search_index = Some(table)`, models without any
    /// searchable column stay `None` (search bridge returns
    /// `NotSearchable`).
    #[test]
    fn all_schemas_helper_search_index_follows_searchable_flag() {
        let schemas = all_schemas();
        assert_eq!(schemas.len(), 3);

        let by_table: std::collections::HashMap<&str, Option<&'static str>> =
            schemas.iter().map(|s| (s.table, s.search_index)).collect();

        // `clients` and `projects` have searchable columns:
        // macro auto-derives search_index.
        assert_eq!(by_table.get("clients"), Some(&Some("clients")));
        assert_eq!(by_table.get("projects"), Some(&Some("projects")));
        // `invoices` has no searchable column — opted out by
        // omission, search_index remains None.
        assert_eq!(by_table.get("invoices"), Some(&None));
    }

    // ----- Search bridge end-to-end --------------------------------------

    /// Project's searchable columns (`name`, `description`) and
    /// filterable / sortable columns flow through the bridge in
    /// declaration order.
    #[test]
    fn project_search_config_matches_field_attributes() {
        let schema = Project::SCHEMA;
        let cfg = search_bridge::search_config_from_schema(&schema)
            .expect("schema is searchable");

        assert_eq!(cfg.index, "projects");
        assert_eq!(cfg.primary_key, "id");
        assert_eq!(cfg.searchable_attributes, vec!["name", "description"]);
        // `client_id` is filterable; nothing else.
        assert_eq!(cfg.filterable_attributes, vec!["client_id"]);
        // `name`, `client_id`, `budget_cents`, `created_at` declared sortable.
        assert_eq!(
            cfg.sortable_attributes,
            vec!["name", "client_id", "budget_cents", "created_at"]
        );
    }

    /// `enablement_from` returns `Disabled` when the validator
    /// would report errors, even with a fully-derived schema.
    /// Verifies the gate without needing a live database.
    #[test]
    fn search_disabled_on_validator_error() {
        let schema = Project::SCHEMA;
        let report = SchemaReport {
            table: "projects".into(),
            status: ReportStatus::Error,
            errors: vec![SchemaIssue {
                column: Some("client_id".into()),
                kind: rustio_core::contract_validator::IssueKind::MissingColumn,
                message: "synthetic for the test".into(),
                expected: None,
                actual: None,
            }],
            warnings: vec![],
        };
        let outcome = search_bridge::enablement_from(&schema, report);
        assert!(!outcome.is_enabled(), "Error report must disable search");
    }

    /// `enablement_from` returns `Enabled` when the validator
    /// reports `Ok`; the produced config matches the pure
    /// `search_config_from_schema` output.
    #[test]
    fn search_enabled_on_validator_ok() {
        let schema = Project::SCHEMA;
        let report = SchemaReport {
            table: "projects".into(),
            status: ReportStatus::Ok,
            errors: vec![],
            warnings: vec![],
        };
        let outcome = search_bridge::enablement_from(&schema, report);
        let cfg = outcome.config().expect("Ok report enables search");
        assert_eq!(cfg.index, "projects");
        assert_eq!(cfg.searchable_attributes, vec!["name", "description"]);
    }

    // ----- Admin bridge end-to-end ---------------------------------------

    /// The admin bridge produces one field per column, in
    /// declaration order, with explicit labels honoured and
    /// `editable = !readonly` set.
    #[test]
    fn admin_bridge_produces_one_field_per_column_in_order() {
        let schema = Client::SCHEMA;
        let bridged = admin_bridge::bridged_fields_from_schema(&schema);

        let names: Vec<&str> = bridged.iter().map(|b| b.field.name).collect();
        assert_eq!(names, vec!["id", "name", "email", "created_at"]);

        // `id` and `created_at` are readonly → editable=false.
        let by_name: std::collections::HashMap<&str, &admin_bridge::BridgedField> =
            bridged.iter().map(|b| (b.field.name, b)).collect();
        assert!(!by_name["id"].field.editable, "id is readonly");
        assert!(!by_name["created_at"].field.editable, "created_at is readonly");
        assert!(by_name["name"].field.editable);
        assert!(by_name["email"].field.editable);

        // Explicit label override on `name`.
        assert_eq!(by_name["name"].field.label, "Client name");
        // Fallback (no override) on `email` — label = column name verbatim.
        assert_eq!(by_name["email"].field.label, "email");
    }

    /// Project's `description` column carries the `widget =
    /// "textarea"` override through to `BridgedField.widget`.
    #[test]
    fn admin_bridge_preserves_widget_override() {
        let schema = Project::SCHEMA;
        let bridged = admin_bridge::bridged_fields_from_schema(&schema);
        let desc = bridged
            .iter()
            .find(|b| b.field.name == "description")
            .expect("description column exists");
        assert_eq!(desc.widget, Some("textarea"));
    }

    // ----- Doctor subprocess wire contract -------------------------------

    /// Sanity: the magic flag the doctor subprocess hook looks
    /// for is the same one the CLI passes. Tripwire — if either
    /// side renames the flag, this test fails before the
    /// example's `cargo run -- --rustio-doctor-schema-check`
    /// silently stops responding.
    #[test]
    fn doctor_subprocess_flag_matches_cli() {
        // Constants live in rustio-core; importing them here
        // proves they're visible from a downstream consumer (and
        // pinning them in this test's expectations means the
        // example breaks loud if either side drifts).
        assert_eq!(
            rustio_core::contract_doctor::SCHEMA_CHECK_FLAG,
            "--rustio-doctor-schema-check"
        );
        assert_eq!(rustio_core::contract_doctor::JSON_FLAG, "--json");
    }
}
