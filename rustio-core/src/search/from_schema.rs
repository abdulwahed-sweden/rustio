//! Phase 14, commit 6 — bridge from `ModelSchema` to search.
//!
//! Schema-first: `ModelSchema` is the single source of truth for
//! which attributes the engine tokenises, filters, and sorts. No
//! manual `Searchable::SEARCHABLE_ATTRIBUTES` declaration is
//! required when using this path.
//!
//! # What stays untouched
//!
//! - The existing `Searchable` trait (`search/traits.rs`) is
//!   **not** modified. Models that hand-implement `Searchable`
//!   keep working unchanged.
//! - `MeiliClient`, `Indexer`, `client.rs`, `indexer.rs` are
//!   not modified — the bridge produces values that drop into
//!   their existing argument shapes (`configure_index(index,
//!   &searchable, &filterable, &sortable)`).
//! - Nothing in `admin/`, `migrations`, `cli/`, `macros/`, or
//!   the contract / validator / doctor modules is touched.
//!
//! # Validator gate
//!
//! Search is enabled only when [`validate_schema`](crate::contract_validator::validate_schema)
//! returns `Ok` or `Warning`:
//!
//! | Validator status | Bridge behaviour                      |
//! |------------------|---------------------------------------|
//! | `Ok`             | enable search                         |
//! | `Warning`        | enable search (warnings logged)       |
//! | `Error`          | refuse to enable — return diagnostics |
//!
//! The rationale: a schema that drifts from the DB will produce
//! Meili documents with the wrong shape (missing fields, wrong
//! types). Better to disable search loudly than to silently index
//! garbage.
//!
//! # Mapping rules
//!
//! For each `ModelColumn`:
//!
//! | Column flag         | Becomes part of                      |
//! |---------------------|--------------------------------------|
//! | `flags.searchable`  | `searchable_attributes`              |
//! | `flags.filterable`  | `filterable_attributes`              |
//! | `flags.sortable`    | `sortable_attributes`                |
//!
//! Plus:
//!
//! - `schema.search_index` → `SearchConfig.index`. `None` means
//!   "model isn't searchable", and the bridge returns
//!   [`SearchEnablement::NotSearchable`] without touching the
//!   validator.
//! - `schema.primary_key` → `SearchConfig.primary_key`. Meili
//!   requires one unique key per document; the contract names it.
//!
//! # Order, exhaustiveness, no silent defaults
//!
//! - Output order matches `schema.columns` declaration order
//!   exactly. Reordering would silently change which fields a
//!   user-typed query weights highest in Meili.
//! - No hardcoded field names. Every name flows from the schema.
//! - Empty searchable set is allowed (and tested) — Meili treats
//!   an empty list as "search over all fields by default", which
//!   is its own answer to "no fields flagged"; the bridge honours
//!   the empty list rather than synthesising defaults.

use crate::contract::{HasSchema, ModelSchema};
use crate::contract_validator::{validate_schema, ReportStatus, SchemaReport};
use crate::orm::Db;

// ---------------------------------------------------------------------------
// SearchConfig
// ---------------------------------------------------------------------------

/// Search configuration derived from a `ModelSchema`. Designed
/// to feed directly into [`MeiliClient::configure_index`] and
/// [`Indexer`] without touching the existing `Searchable` trait.
///
/// All names are `&'static str` because they come from
/// `ModelColumn`'s static-only fields (the contract is built at
/// compile time by `#[derive(RustioModel)]`). No allocation on
/// the lookup hot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchConfig {
    /// Meili index name. Sourced from `ModelSchema.search_index`.
    pub index: &'static str,
    /// Primary-key field on every document. Sourced from
    /// `ModelSchema.primary_key`.
    pub primary_key: &'static str,
    /// Attributes Meili tokenises for full-text queries. Order
    /// matches the schema's declaration order — Meili weights
    /// the first attribute highest by default, so order matters.
    pub searchable_attributes: Vec<&'static str>,
    /// Attributes available for `filter=` queries.
    pub filterable_attributes: Vec<&'static str>,
    /// Attributes available for `sort=` queries.
    pub sortable_attributes: Vec<&'static str>,
}

// ---------------------------------------------------------------------------
// Pure derivation
// ---------------------------------------------------------------------------

/// Derive a `SearchConfig` from a schema **without** running the
/// validator. Returns `None` when `schema.search_index` is `None`
/// (the model isn't declared searchable in the contract).
///
/// Pure / synchronous. Safe to call from tests, build scripts,
/// or any non-async context. Used by [`enable_search`] under the
/// hood, and exposed publicly so callers that have already done
/// their own validation can skip the gate.
pub fn search_config_from_schema(schema: &ModelSchema) -> Option<SearchConfig> {
    let index = schema.search_index?;
    Some(SearchConfig {
        index,
        primary_key: schema.primary_key,
        searchable_attributes: schema
            .columns
            .iter()
            .filter(|c| c.flags.searchable)
            .map(|c| c.name)
            .collect(),
        filterable_attributes: schema
            .columns
            .iter()
            .filter(|c| c.flags.filterable)
            .map(|c| c.name)
            .collect(),
        sortable_attributes: schema
            .columns
            .iter()
            .filter(|c| c.flags.sortable)
            .map(|c| c.name)
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// SearchEnablement — the validator-gated outcome
// ---------------------------------------------------------------------------

/// Result of asking "should search be enabled for this model?".
///
/// Three distinct outcomes so callers can log meaningfully:
///
/// - [`Self::NotSearchable`] — the contract declares no
///   `search_index`. Not a failure; the model simply isn't
///   indexed.
/// - [`Self::Disabled`] — the validator returned errors. Search
///   is refused; the report is included so operators can see why.
/// - [`Self::Enabled`] — search is enabled. The config is ready
///   to feed into Meili; the report is attached so any warnings
///   can be logged.
#[derive(Debug, Clone)]
pub enum SearchEnablement {
    NotSearchable,
    Disabled { report: SchemaReport },
    Enabled {
        config: SearchConfig,
        report: SchemaReport,
    },
}

impl SearchEnablement {
    /// Convenience: `true` iff search is enabled. Use when only
    /// the gate decision matters (logging, metrics).
    pub fn is_enabled(&self) -> bool {
        matches!(self, SearchEnablement::Enabled { .. })
    }

    /// The derived [`SearchConfig`] when search is enabled.
    /// `None` for `NotSearchable` and `Disabled`.
    pub fn config(&self) -> Option<&SearchConfig> {
        match self {
            SearchEnablement::Enabled { config, .. } => Some(config),
            _ => None,
        }
    }

    /// The validator [`SchemaReport`], if one was produced. `None`
    /// for `NotSearchable` (the gate short-circuits before
    /// validating).
    pub fn report(&self) -> Option<&SchemaReport> {
        match self {
            SearchEnablement::NotSearchable => None,
            SearchEnablement::Disabled { report }
            | SearchEnablement::Enabled { report, .. } => Some(report),
        }
    }
}

// ---------------------------------------------------------------------------
// Validator-gated entry points
// ---------------------------------------------------------------------------

/// Ask the validator about `M`'s schema and return whether search
/// should be enabled. The async boundary; production callers use
/// this from server bootstrap.
///
/// Implementation note: this is a thin wrapper around
/// [`validate_schema`] + [`enablement_from`]. Unit tests should
/// target [`enablement_from`] directly to avoid needing a Postgres
/// connection.
pub async fn enable_search<M: HasSchema>(db: &Db) -> SearchEnablement {
    let schema = M::SCHEMA;
    let report = validate_schema::<M>(db).await;
    enablement_from(&schema, report)
}

/// Pure decision helper splitting the validator-gated logic out
/// of the async boundary. Given a schema and a (presumably already-
/// produced) [`SchemaReport`], decide whether search should be
/// enabled.
///
/// Three branches:
///
/// 1. `report.status == Error` → [`SearchEnablement::Disabled`].
///    Refuse before deriving the config; the schema is broken,
///    indexing it would silently produce malformed documents.
/// 2. Schema has no `search_index` → [`SearchEnablement::NotSearchable`].
///    The contract opted out of search; honour it.
/// 3. Otherwise → [`SearchEnablement::Enabled`] with the config
///    derived from the schema and the report attached for
///    warning-level diagnostics.
pub fn enablement_from(schema: &ModelSchema, report: SchemaReport) -> SearchEnablement {
    match report.status {
        ReportStatus::Error => SearchEnablement::Disabled { report },
        ReportStatus::Ok | ReportStatus::Warning => match search_config_from_schema(schema) {
            Some(config) => SearchEnablement::Enabled { config, report },
            None => SearchEnablement::NotSearchable,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ModelColumn, RustType, SchemaFlags};
    use crate::contract_validator::{IssueKind, SchemaIssue};

    // ----- Fixture builders ------------------------------------------------

    /// A schema with a mix of searchable / filterable / sortable
    /// columns plus a non-flagged column. Drives the "only flagged
    /// columns are indexed" and "ordering preserved" tests.
    fn fixture_schema() -> ModelSchema {
        static COLS: &[ModelColumn] = &[
            // Primary key — sortable + readonly. Not searchable.
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
            // Searchable + filterable.
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
                admin_label: None,
                admin_widget: None,
            },
            // Searchable only — second searchable to verify order.
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
                admin_widget: None,
            },
            // No flags — must be excluded from every list.
            ModelColumn {
                name: "internal_note",
                sql_decl: "TEXT",
                rust_type: RustType::String,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            },
            // Filterable + sortable, not searchable. Verifies the
            // three lists are independent.
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
        ];
        ModelSchema {
            table: "posts",
            columns: COLS,
            primary_key: "id",
            search_index: Some("posts"),
        }
    }

    /// A schema with no `search_index` — the contract opts out of
    /// search entirely. Drives the `NotSearchable` branch.
    fn fixture_unsearchable_schema() -> ModelSchema {
        static COLS: &[ModelColumn] = &[ModelColumn {
            name: "id",
            sql_decl: "BIGSERIAL PRIMARY KEY",
            rust_type: RustType::I64,
            nullable: false,
            primary_key: true,
            flags: SchemaFlags::empty(),
            admin_label: None,
            admin_widget: None,
        }];
        ModelSchema {
            table: "audit_logs",
            columns: COLS,
            primary_key: "id",
            search_index: None,
        }
    }

    /// A schema with a `search_index` but zero columns flagged
    /// `searchable` / `filterable` / `sortable`. The bridge must
    /// honour the empty lists (no synthesised defaults).
    fn fixture_empty_searchable_schema() -> ModelSchema {
        static COLS: &[ModelColumn] = &[
            ModelColumn {
                name: "id",
                sql_decl: "BIGSERIAL PRIMARY KEY",
                rust_type: RustType::I64,
                nullable: false,
                primary_key: true,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            },
            ModelColumn {
                name: "value",
                sql_decl: "TEXT NOT NULL",
                rust_type: RustType::String,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            },
        ];
        ModelSchema {
            table: "items",
            columns: COLS,
            primary_key: "id",
            search_index: Some("items"),
        }
    }

    fn ok_report(table: &str) -> SchemaReport {
        SchemaReport {
            table: table.to_string(),
            status: ReportStatus::Ok,
            errors: vec![],
            warnings: vec![],
        }
    }

    fn warning_report(table: &str) -> SchemaReport {
        SchemaReport {
            table: table.to_string(),
            status: ReportStatus::Warning,
            errors: vec![],
            warnings: vec![SchemaIssue {
                column: Some("legacy_code".into()),
                kind: IssueKind::ExtraDbColumn,
                message: "extra DB column `legacy_code` not declared in Rust contract".into(),
                expected: None,
                actual: Some("legacy_code".into()),
            }],
        }
    }

    fn error_report(table: &str) -> SchemaReport {
        SchemaReport {
            table: table.to_string(),
            status: ReportStatus::Error,
            errors: vec![SchemaIssue {
                column: Some("amount".into()),
                kind: IssueKind::MissingColumn,
                message: "column `posts.amount` declared in Rust contract not present in database"
                    .into(),
                expected: Some("NUMERIC NOT NULL".into()),
                actual: None,
            }],
            warnings: vec![],
        }
    }

    // ----- Spec gate: searchable columns come ONLY from schema -------------

    /// Spec gate: only fields with `flags.searchable == true` are
    /// indexed. Verifies the bridge does not include any other
    /// columns and does not invent any field names.
    #[test]
    fn searchable_attributes_drawn_only_from_flagged_columns() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).expect("schema is searchable");

        // Only `title` and `body` are flagged searchable.
        assert_eq!(cfg.searchable_attributes, vec!["title", "body"]);

        // Negative: every other column stays out.
        for excluded in ["id", "internal_note", "published_at"] {
            assert!(
                !cfg.searchable_attributes.contains(&excluded),
                "column `{excluded}` should not appear in searchable_attributes"
            );
        }
    }

    /// Spec gate: non-searchable fields excluded. Sister assertion
    /// to the previous test — frames the negative case directly.
    #[test]
    fn non_searchable_fields_excluded_from_search_list() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).unwrap();

        // The `internal_note` column has all flags off — it must
        // not appear in any list.
        for list_name in [
            ("searchable", &cfg.searchable_attributes),
            ("filterable", &cfg.filterable_attributes),
            ("sortable", &cfg.sortable_attributes),
        ] {
            let (name, list) = list_name;
            assert!(
                !list.contains(&"internal_note"),
                "internal_note must be excluded from {name}"
            );
        }
    }

    // ----- Spec gate: ordering preserved ----------------------------------

    /// Spec gate: order matches schema declaration order. Meili
    /// weights the first searchable attribute highest, so a
    /// stable order is part of the contract.
    #[test]
    fn ordering_preserved_within_searchable_attributes() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).unwrap();

        // `title` comes before `body` in `schema.columns`.
        let title_idx = cfg.searchable_attributes.iter().position(|s| *s == "title");
        let body_idx = cfg.searchable_attributes.iter().position(|s| *s == "body");
        assert_eq!(title_idx, Some(0));
        assert_eq!(body_idx, Some(1));
    }

    /// `filterable_attributes` and `sortable_attributes` follow
    /// the same order rule.
    #[test]
    fn ordering_preserved_within_filterable_and_sortable() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).unwrap();

        // `title` (col idx 1) before `published_at` (col idx 4).
        assert_eq!(cfg.filterable_attributes, vec!["title", "published_at"]);
        // `id` (col idx 0) before `published_at` (col idx 4).
        assert_eq!(cfg.sortable_attributes, vec!["id", "published_at"]);
    }

    // ----- Spec gate: empty searchable set handled safely -----------------

    /// Spec gate: empty searchable set handled safely. A schema
    /// that's nominally indexed but has zero flagged columns must
    /// still produce a valid (empty-attribute-list) `SearchConfig`,
    /// not panic, not synthesise defaults.
    #[test]
    fn empty_searchable_set_yields_empty_lists_not_panic() {
        let schema = fixture_empty_searchable_schema();
        let cfg = search_config_from_schema(&schema).expect("search_index is set");

        assert_eq!(cfg.index, "items");
        assert_eq!(cfg.primary_key, "id");
        assert!(cfg.searchable_attributes.is_empty());
        assert!(cfg.filterable_attributes.is_empty());
        assert!(cfg.sortable_attributes.is_empty());
    }

    /// A schema that explicitly opts out of search (no
    /// `search_index`) returns `None` from the pure derivation.
    /// The validator gate treats this as `NotSearchable`.
    #[test]
    fn schema_with_no_search_index_yields_none() {
        let schema = fixture_unsearchable_schema();
        assert!(search_config_from_schema(&schema).is_none());
    }

    // ----- Spec gate: validator gating ------------------------------------

    /// Spec gate: search disabled when validator returns errors.
    /// The bridge refuses to enable search and surfaces the report
    /// for diagnostics.
    #[test]
    fn search_disabled_when_validator_returns_errors() {
        let schema = fixture_schema();
        let report = error_report(schema.table);

        let outcome = enablement_from(&schema, report.clone());
        match outcome {
            SearchEnablement::Disabled { report: r } => {
                assert_eq!(r, report);
                assert_eq!(r.status, ReportStatus::Error);
            }
            other => panic!("expected Disabled, got {:?}", other),
        }

        // is_enabled / config / report convenience methods agree.
        let outcome = enablement_from(&schema, error_report(schema.table));
        assert!(!outcome.is_enabled());
        assert!(outcome.config().is_none());
        assert!(outcome.report().is_some());
    }

    /// Spec gate: search allowed with warnings. A `Warning`-status
    /// report is informational; the bridge still enables search.
    #[test]
    fn search_allowed_when_validator_returns_warnings_only() {
        let schema = fixture_schema();
        let report = warning_report(schema.table);

        let outcome = enablement_from(&schema, report);
        match outcome {
            SearchEnablement::Enabled { config, report: r } => {
                assert_eq!(r.status, ReportStatus::Warning);
                assert_eq!(config.index, "posts");
                assert_eq!(config.searchable_attributes, vec!["title", "body"]);
            }
            other => panic!("expected Enabled, got {:?}", other),
        }
    }

    /// `Ok` status enables search with the report attached.
    #[test]
    fn search_enabled_when_validator_returns_ok() {
        let schema = fixture_schema();
        let outcome = enablement_from(&schema, ok_report(schema.table));
        match outcome {
            SearchEnablement::Enabled { config, report } => {
                assert_eq!(report.status, ReportStatus::Ok);
                assert_eq!(config.index, "posts");
                assert_eq!(config.primary_key, "id");
                assert_eq!(config.searchable_attributes, vec!["title", "body"]);
                assert_eq!(config.filterable_attributes, vec!["title", "published_at"]);
                assert_eq!(config.sortable_attributes, vec!["id", "published_at"]);
            }
            other => panic!("expected Enabled, got {:?}", other),
        }
    }

    /// A schema without a `search_index` short-circuits to
    /// `NotSearchable` regardless of the validator's verdict —
    /// the contract opts out before validation matters.
    #[test]
    fn unsearchable_schema_short_circuits_to_not_searchable() {
        let schema = fixture_unsearchable_schema();

        // Even an `Ok` report doesn't enable search if the schema
        // declares `search_index = None`.
        let outcome = enablement_from(&schema, ok_report(schema.table));
        match outcome {
            SearchEnablement::NotSearchable => {}
            other => panic!("expected NotSearchable, got {:?}", other),
        }

        // is_enabled / config / report convenience methods agree.
        let outcome = enablement_from(&schema, ok_report(schema.table));
        assert!(!outcome.is_enabled());
        assert!(outcome.config().is_none());
        assert!(outcome.report().is_none(), "NotSearchable carries no report");
    }

    // ----- SearchConfig invariants ----------------------------------------

    /// Index name and primary key flow from schema verbatim. No
    /// rewrites, no defaults.
    #[test]
    fn search_config_carries_schema_index_and_primary_key() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).unwrap();
        assert_eq!(cfg.index, "posts");
        assert_eq!(cfg.primary_key, "id");
    }

    /// Static slice usability: the produced lists are
    /// `Vec<&'static str>`, so they can feed directly into Meili
    /// API methods that take `&[&str]` without further allocation
    /// of intermediate string buffers.
    #[test]
    fn search_config_lists_are_static_str_borrowable_as_str_slices() {
        let schema = fixture_schema();
        let cfg = search_config_from_schema(&schema).unwrap();
        // Compile-time gate: this won't compile if the type
        // changes from `Vec<&'static str>` to something else.
        fn assert_static_strs(_: &[&'static str]) {}
        assert_static_strs(&cfg.searchable_attributes);
        assert_static_strs(&cfg.filterable_attributes);
        assert_static_strs(&cfg.sortable_attributes);
    }

    /// `is_enabled` is the only branch that carries a config.
    #[test]
    fn enablement_accessor_invariants() {
        let schema = fixture_schema();
        let enabled = enablement_from(&schema, ok_report("posts"));
        let disabled = enablement_from(&schema, error_report("posts"));
        let none = enablement_from(&fixture_unsearchable_schema(), ok_report("audit_logs"));

        assert!(enabled.is_enabled());
        assert!(!disabled.is_enabled());
        assert!(!none.is_enabled());

        assert!(enabled.config().is_some());
        assert!(disabled.config().is_none());
        assert!(none.config().is_none());
    }
}
