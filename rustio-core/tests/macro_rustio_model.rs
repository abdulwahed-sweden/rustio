//! Integration tests for `#[derive(RustioModel)]` (Phase 14, commit 2).
//!
//! Lives in `tests/` (a separate test crate) so commit 2's strict
//! isolation rule is preserved: NO modifications to
//! `rustio-core/src/contract.rs`, `rustio-core/src/lib.rs`, or any
//! file's dev-dependencies. The macro is exercised against the
//! contract layer EXACTLY as it ships at cc25125.
//!
//! ## Test strategy
//!
//! Positive end-to-end tests only. Each test derives a fresh struct,
//! reads `<Struct>::SCHEMA`, and asserts on the fields the macro
//! should have populated. The behaviour under test is the wire
//! contract between the macro and `rustio_core::contract`.
//!
//! ## Compile-fail tests (explicit deferral)
//!
//! Commit 2's spec lists six compile-fail tests as MANDATORY:
//!   - `id` field with type `i32`
//!   - `f64` paired with `NUMERIC`
//!   - `NaiveDateTime` field
//!   - missing `#[rustio(table = ...)]`
//!   - missing `PRIMARY KEY` in any field's SQL
//!   - missing `#[rustio(sql = ...)]` on a field
//!
//! Under the strict-isolation rules (no `contract.rs`/`lib.rs`
//! changes, no new deps including `trybuild`), there is **no
//! mechanism on stable Rust** to express compile-fail assertions
//! that a CI run would actually verify:
//!
//! - `compile_fail` doctests only extract from `pub` items in the
//!   library crate — adding them requires touching `lib.rs` or a
//!   `pub mod` reachable from it.
//! - Doctests on items in `tests/*.rs` are not extracted by rustdoc
//!   (integration test crates).
//! - Doctests on the `proc_macro_derive` itself in `rustio-macros`
//!   would compile in a context that doesn't import `rustio_core`,
//!   so any failure is for the wrong reason and the test would
//!   trivially "pass" without testing anything.
//! - `trybuild` is the standard alternative; it requires a dev-dep.
//!
//! The compile-fail rules ARE enforced in the macro source — see
//! `rustio_macros::rustio_model::validate_field_rules` and
//! `classify` — and the error messages name the rule each
//! enforces. A documentation block at the bottom of this file
//! shows the input shapes that fail and the expected error
//! message; reviewers can copy-paste any of those into a sandbox
//! crate to verify by hand.
//!
//! Resolving this needs ONE of:
//!   1. Permission to add a single line `#[doc(hidden)] pub mod
//!      compile_fail_examples;` to `rustio-core/src/lib.rs` —
//!      breaks the strict-isolation rule by exactly one line.
//!   2. Permission to add `trybuild = "1"` as a dev-dep on
//!      `rustio-macros` — breaks the no-dep rule.
//!   3. Permission to add `compile_fail_examples` as a `pub mod`
//!      directly in `rustio-core/src/contract.rs` — breaks the
//!      no-touching-contract rule.
//!
//! Until one of those is approved, compile-fail tests stay
//! documented but not auto-asserted. See the bottom of this file.

// `HasSchema` is brought into scope so the macro-generated
// `T::SCHEMA` reference resolves through the trait associated
// const. Without the import, accessing `<T as HasSchema>::SCHEMA`
// requires the explicit qualified syntax.
use rustio_core::contract::{HasSchema, RustType};
use rustio_macros::RustioModel;

// ---------------------------------------------------------------------------
// Stub types
// ---------------------------------------------------------------------------
//
// The macro classifies field types by their LAST PATH SEGMENT only —
// it never reads the type's actual definition. So `Decimal` (a local
// unit struct) and `rust_decimal::Decimal` (the real crate type)
// classify identically. This lets the test crate avoid pulling in
// `rust_decimal` / `uuid` as deps while still exercising every
// classification branch.

#[allow(dead_code)]
struct Decimal;

#[allow(dead_code)]
struct Uuid;

// ---------------------------------------------------------------------------
// Positive tests
// ---------------------------------------------------------------------------

/// Happy path: two-field struct → SCHEMA is populated correctly,
/// primary key is detected from the SQL, the `searchable` flag flows.
#[test]
fn rustio_model_derive_happy_path() {
    #[derive(RustioModel)]
    #[rustio(table = "projects")]
    #[allow(dead_code)]
    struct Project {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "TEXT NOT NULL", searchable)]
        title: String,
    }

    let s = Project::SCHEMA;
    assert_eq!(s.table, "projects");
    assert_eq!(s.primary_key, "id");
    assert_eq!(s.columns.len(), 2);
    assert!(s.search_index.is_none()); // not auto-set in commit 2

    let id = s.column("id").expect("id column missing");
    assert_eq!(id.name, "id");
    assert_eq!(id.sql_decl, "BIGSERIAL PRIMARY KEY");
    assert_eq!(id.rust_type, RustType::I64);
    assert!(id.primary_key);
    assert!(!id.nullable);

    let title = s.column("title").expect("title column missing");
    assert_eq!(title.rust_type, RustType::String);
    assert!(title.flags.searchable);
    assert!(!title.flags.filterable);
    assert!(!title.primary_key);
}

/// Complex struct: `Option<String>` (nullable), multiple flags on
/// one field, label/widget overrides, every primitive type the
/// macro recognises. `Decimal` and `DateTime<Utc>` use stub types
/// (see top of file) so no dev-dep additions are required.
#[test]
fn rustio_model_derive_complex_struct() {
    use chrono::{DateTime, Utc};

    #[derive(RustioModel)]
    #[rustio(table = "invoices")]
    #[allow(dead_code)]
    struct Invoice {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "BIGINT NOT NULL", filterable)]
        contract_id: i64,
        #[rustio(sql = "NUMERIC(12,2) NOT NULL", sortable, filterable)]
        amount: Decimal,
        #[rustio(sql = "TIMESTAMPTZ NOT NULL DEFAULT NOW()", readonly)]
        issued_at: DateTime<Utc>,
        #[rustio(sql = "TEXT", searchable, widget = "textarea", label = "Memo")]
        notes: Option<String>,
        #[rustio(sql = "BOOLEAN NOT NULL DEFAULT FALSE")]
        paid: bool,
    }

    let s = Invoice::SCHEMA;
    assert_eq!(s.table, "invoices");
    assert_eq!(s.columns.len(), 6);

    // Primary key detected.
    assert_eq!(s.primary_key, "id");

    // Filterable single-flag.
    let cid = s.column("contract_id").unwrap();
    assert!(cid.flags.filterable);
    assert!(!cid.flags.searchable);
    assert!(!cid.flags.sortable);

    // Multiple flags on one field — exercises the mut-block emission.
    let amt = s.column("amount").unwrap();
    assert_eq!(amt.rust_type, RustType::Decimal);
    assert!(amt.flags.sortable);
    assert!(amt.flags.filterable);
    assert!(!amt.flags.searchable);

    // DateTime<Utc> + readonly.
    let iss = s.column("issued_at").unwrap();
    assert_eq!(iss.rust_type, RustType::DateTimeUtc);
    assert!(iss.flags.readonly);
    assert!(!iss.nullable);

    // Optional<String> + searchable + widget + label.
    let notes = s.column("notes").unwrap();
    assert_eq!(notes.rust_type, RustType::String);
    assert!(notes.nullable);
    assert!(notes.flags.searchable);
    assert_eq!(notes.admin_widget, Some("textarea"));
    assert_eq!(notes.admin_label, Some("Memo"));

    // Bool — sanity check the type classifies correctly.
    let paid = s.column("paid").unwrap();
    assert_eq!(paid.rust_type, RustType::Bool);
}

/// `T::SCHEMA` is usable in a `const` context — proves the macro's
/// emission is genuinely const-fn-composed all the way through. If
/// any step in the macro's output stops being const-evaluable, this
/// test fails to compile (not at runtime).
#[test]
fn rustio_model_derive_const_context() {
    #[derive(RustioModel)]
    #[rustio(table = "tags")]
    #[allow(dead_code)]
    struct Tag {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "TEXT NOT NULL")]
        name: String,
    }

    const TABLE_NAME: &str = Tag::SCHEMA.table;
    const COL_COUNT: usize = Tag::SCHEMA.columns.len();
    const PK_NAME: &str = Tag::SCHEMA.primary_key;
    assert_eq!(TABLE_NAME, "tags");
    assert_eq!(COL_COUNT, 2);
    assert_eq!(PK_NAME, "id");
}

/// `serde_json::Value` (already a `rustio-core` runtime dep) classifies
/// as `RustType::JsonValue`. The macro inspects only the last path
/// segment — `Value`, `serde_json::Value`, and any re-export ending
/// in `Value` all hit this branch.
#[test]
fn rustio_model_derive_jsonb_value() {
    #[derive(RustioModel)]
    #[rustio(table = "events")]
    #[allow(dead_code)]
    struct Event {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "JSONB NOT NULL DEFAULT '{}'::jsonb")]
        payload: serde_json::Value,
    }
    assert_eq!(
        Event::SCHEMA.column("payload").unwrap().rust_type,
        RustType::JsonValue
    );
}

/// `Uuid` classifies as `RustType::Uuid`. Stub type at top of file
/// is sufficient — the macro doesn't need the real `uuid::Uuid`
/// type to exist.
#[test]
fn rustio_model_derive_uuid_classifies() {
    #[derive(RustioModel)]
    #[rustio(table = "tokens")]
    #[allow(dead_code)]
    struct Token {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "UUID NOT NULL UNIQUE")]
        external_id: Uuid,
    }
    assert_eq!(
        Token::SCHEMA.column("external_id").unwrap().rust_type,
        RustType::Uuid
    );
}

/// `references = "..."` parses without error but does NOT emit
/// code (cc25125's `ModelColumn` has no `references` field — see
/// the deferral note in `rustio_macros::rustio_model::build_column_expr`).
/// This test confirms the attribute is accepted without breaking
/// the macro; future commits that add `references` to `ModelColumn`
/// will re-enable emission with a one-line restoration.
#[test]
fn rustio_model_derive_accepts_references_attr_silently() {
    #[derive(RustioModel)]
    #[rustio(table = "proposals")]
    #[allow(dead_code)]
    struct Proposal {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        // `references` IS parsed; nothing breaks. The value is
        // silently dropped because the contract layer doesn't
        // store FK metadata yet.
        #[rustio(sql = "BIGINT NOT NULL", references = "projects(id)")]
        project_id: i64,
    }

    let s = Proposal::SCHEMA;
    assert_eq!(s.column("project_id").unwrap().rust_type, RustType::I64);
    // We can't assert the references value (no field) but we CAN
    // assert nothing else broke — the column is present with the
    // expected type and the SQL declaration is intact.
    assert_eq!(s.column("project_id").unwrap().sql_decl, "BIGINT NOT NULL");
}

/// Empty-flags case: a column with no flag attributes emits
/// `SchemaFlags::empty()` (the no-mutations branch).
#[test]
fn rustio_model_derive_empty_flags_emits_empty_constant() {
    #[derive(RustioModel)]
    #[rustio(table = "plain")]
    #[allow(dead_code)]
    struct Plain {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "TEXT NOT NULL")]
        name: String,
    }

    let name = Plain::SCHEMA.column("name").unwrap();
    assert!(!name.flags.searchable);
    assert!(!name.flags.filterable);
    assert!(!name.flags.sortable);
    assert!(!name.flags.readonly);
}

/// `i32` is a valid type for non-id integer columns. The "id must
/// be i64" rule fires only when the field is named `id`.
#[test]
fn rustio_model_derive_i32_allowed_for_non_id_fields() {
    #[derive(RustioModel)]
    #[rustio(table = "stats")]
    #[allow(dead_code)]
    struct Stat {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "INTEGER NOT NULL")]
        count: i32,
    }
    assert_eq!(Stat::SCHEMA.column("count").unwrap().rust_type, RustType::I32);
}

/// `f64` is a valid type when paired with a non-NUMERIC column.
/// Type Rule #3 fires only when SQL contains NUMERIC/DECIMAL.
#[test]
fn rustio_model_derive_f64_allowed_for_non_numeric_columns() {
    #[derive(RustioModel)]
    #[rustio(table = "measurements")]
    #[allow(dead_code)]
    struct Measurement {
        #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
        id: i64,
        #[rustio(sql = "DOUBLE PRECISION NOT NULL")]
        temperature_celsius: f64,
    }
    assert_eq!(
        Measurement::SCHEMA.column("temperature_celsius").unwrap().rust_type,
        RustType::F64
    );
}

// ---------------------------------------------------------------------------
// Compile-fail rule documentation (NOT auto-tested under strict
// isolation — see top-of-file deferral note)
// ---------------------------------------------------------------------------
//
// Each block below shows a struct shape that the macro MUST reject
// at compile time, plus the error message the macro emits. A
// reviewer can copy any block into a sandbox crate, run `cargo
// build`, and verify the rejection by hand.
//
// ─── Type Rule #1 — `id` MUST be `i64` ─────────────────────────────
//
//     #[derive(RustioModel)]
//     #[rustio(table = "x")]
//     struct Bad {
//         #[rustio(sql = "INTEGER PRIMARY KEY")]
//         id: i32,
//     }
//
//     // error: Type Rule #1: field `id` must be `i64`
//     //        (mapped to BIGINT/BIGSERIAL). Using a smaller
//     //        integer type for IDs silently truncates at 2.1B rows.
//
// ─── Type Rule #3 — NUMERIC requires `Decimal` ─────────────────────
//
//     #[derive(RustioModel)]
//     #[rustio(table = "x")]
//     struct Bad {
//         #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
//         id: i64,
//         #[rustio(sql = "NUMERIC(12,2) NOT NULL")]
//         amount: f64,
//     }
//
//     // error: Type Rule #3: NUMERIC/DECIMAL columns must pair with
//     //        `rust_decimal::Decimal`. Using `f64` (or any other
//     //        type) for money loses precision under arithmetic.
//
// ─── Type Rule #2 — `NaiveDateTime` is forbidden ───────────────────
//
//     use chrono::NaiveDateTime;
//
//     #[derive(RustioModel)]
//     #[rustio(table = "x")]
//     struct Bad {
//         #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
//         id: i64,
//         #[rustio(sql = "TIMESTAMP NOT NULL")]
//         created_at: NaiveDateTime,
//     }
//
//     // error: RustioModel: `NaiveDateTime` is forbidden
//     //        (Type Rule #2) — use `chrono::DateTime<chrono::Utc>`
//     //        for all timestamp columns
//
// ─── Missing `#[rustio(table = "...")]` on the struct ──────────────
//
//     #[derive(RustioModel)]
//     struct Bad {
//         #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
//         id: i64,
//     }
//
//     // error: RustioModel requires a `#[rustio(table = "...")]`
//     //        attribute on the struct
//
// ─── Missing PRIMARY KEY anywhere ──────────────────────────────────
//
//     #[derive(RustioModel)]
//     #[rustio(table = "x")]
//     struct Bad {
//         #[rustio(sql = "BIGINT NOT NULL")]
//         id: i64,
//     }
//
//     // error: RustioModel requires at least one field whose
//     //        `sql = "..."` declares PRIMARY KEY
//
// ─── Missing `#[rustio(sql = "...")]` on a field ───────────────────
//
//     #[derive(RustioModel)]
//     #[rustio(table = "x")]
//     struct Bad {
//         #[rustio(sql = "BIGSERIAL PRIMARY KEY")]
//         id: i64,
//         title: String,
//     }
//
//     // error: field `title` is missing the required
//     //        `#[rustio(sql = "...")]` attribute
//
// All six rules are enforced in `rustio-macros/src/lib.rs` —
// `validate_field_rules`, `classify`, `parse_table_attr`, and
// `parse_field_attr` are the call sites.
