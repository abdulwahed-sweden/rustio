//! Tests for the suggestion engine.
//!
//! The engine has two public functions (`derive_suggestions`,
//! `find_suggestion`); every assertion below pins a specific
//! behaviour we don't want to drift: no context → no noise, no
//! overlap → no noise, deterministic ordering, URL safety.

use super::suggestions::{derive_suggestions, find_suggestion};
use super::{AdminEntry, AdminField, FieldType};
use crate::ai::ContextConfig;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const APPLICANT_FIELDS: &[AdminField] = &[
    AdminField {
        name: "id",
        ty: FieldType::I64,
        editable: false,
        nullable: false,
    },
    AdminField {
        name: "personnummer",
        ty: FieldType::String,
        editable: true,
        nullable: false,
    },
    AdminField {
        name: "queue_start_date",
        ty: FieldType::DateTime,
        editable: true,
        nullable: false,
    },
    // Deliberately omits `annual_income` so a housing-context
    // suggestion fires for it.
];

const FULLY_COVERED_FIELDS: &[AdminField] = &[
    AdminField {
        name: "id",
        ty: FieldType::I64,
        editable: false,
        nullable: false,
    },
    AdminField {
        name: "personnummer",
        ty: FieldType::String,
        editable: true,
        nullable: false,
    },
    AdminField {
        name: "queue_start_date",
        ty: FieldType::DateTime,
        editable: true,
        nullable: false,
    },
    AdminField {
        name: "annual_income",
        ty: FieldType::I32,
        editable: true,
        nullable: false,
    },
];

const WIDGET_FIELDS: &[AdminField] = &[
    AdminField {
        name: "id",
        ty: FieldType::I64,
        editable: false,
        nullable: false,
    },
    AdminField {
        name: "name",
        ty: FieldType::String,
        editable: true,
        nullable: false,
    },
];

fn applicant_entry() -> AdminEntry {
    AdminEntry {
        admin_name: "applicants",
        display_name: "Applicants",
        singular_name: "Applicant",
        table: "applicants",
        fields: APPLICANT_FIELDS,
        core: false,
    }
}

fn fully_covered_entry() -> AdminEntry {
    AdminEntry {
        admin_name: "applicants",
        display_name: "Applicants",
        singular_name: "Applicant",
        table: "applicants",
        fields: FULLY_COVERED_FIELDS,
        core: false,
    }
}

fn widget_entry() -> AdminEntry {
    AdminEntry {
        admin_name: "widgets",
        display_name: "Widgets",
        singular_name: "Widget",
        table: "widgets",
        fields: WIDGET_FIELDS,
        core: false,
    }
}

fn core_user_entry() -> AdminEntry {
    AdminEntry {
        admin_name: "users",
        display_name: "Users",
        singular_name: "User",
        table: "rustio_users",
        fields: &[],
        core: true,
    }
}

fn housing_context() -> ContextConfig {
    ContextConfig {
        country: Some("SE".into()),
        industry: Some("housing".into()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// derive_suggestions
// ---------------------------------------------------------------------------

#[test]
fn no_context_produces_no_suggestions() {
    let entries = vec![applicant_entry()];
    assert!(derive_suggestions(&entries, None).is_empty());
}

#[test]
fn no_industry_schema_produces_no_suggestions() {
    let entries = vec![applicant_entry()];
    // Country set but no industry → no suggestions (nothing to compare).
    let ctx = ContextConfig {
        country: Some("SE".into()),
        ..Default::default()
    };
    assert!(derive_suggestions(&entries, Some(&ctx)).is_empty());
}

#[test]
fn unrelated_model_gets_no_suggestions() {
    // A Widget model under housing context covers *none* of the
    // convention fields — the engine should not nag about
    // personnummer on widgets.
    let entries = vec![widget_entry()];
    let ctx = housing_context();
    assert!(derive_suggestions(&entries, Some(&ctx)).is_empty());
}

#[test]
fn fully_covered_model_gets_no_suggestions() {
    let entries = vec![fully_covered_entry()];
    let ctx = housing_context();
    assert!(derive_suggestions(&entries, Some(&ctx)).is_empty());
}

#[test]
fn missing_field_triggers_exactly_one_suggestion() {
    let entries = vec![applicant_entry()];
    let ctx = housing_context();
    let suggestions = derive_suggestions(&entries, Some(&ctx));
    assert_eq!(suggestions.len(), 1);
    let s = &suggestions[0];
    assert_eq!(s.field, "annual_income");
    assert_eq!(s.admin_name, "applicants");
    assert_eq!(s.model_singular, "Applicant");
    assert_eq!(s.prompt, "add annual_income to applicants");
    assert_eq!(s.action, "add_field");
    assert!(s.reason.contains("housing"));
}

#[test]
fn core_models_are_skipped() {
    // A core model (e.g. `User`) never gets suggestions even if its
    // fields would match a convention.
    let entries = vec![core_user_entry(), applicant_entry()];
    let ctx = housing_context();
    let all = derive_suggestions(&entries, Some(&ctx));
    assert!(all.iter().all(|s| s.admin_name != "users"));
}

#[test]
fn ordering_is_deterministic() {
    // Same input → same output byte-for-byte. Important because the
    // dashboard button order determines operator trust.
    let entries = vec![applicant_entry()];
    let ctx = housing_context();
    let a = derive_suggestions(&entries, Some(&ctx));
    let b = derive_suggestions(&entries, Some(&ctx));
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// find_suggestion
// ---------------------------------------------------------------------------

#[test]
fn find_returns_the_existing_suggestion() {
    let entries = vec![applicant_entry()];
    let ctx = housing_context();
    let hit = find_suggestion(&entries, Some(&ctx), "applicants", "annual_income");
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().prompt, "add annual_income to applicants");
}

#[test]
fn find_rejects_crafted_urls() {
    // A user visiting `/admin/suggestions/applicants/anything_i_want`
    // must not be able to drive the planner — `find_suggestion`
    // returns `None` for any pair outside the derived list.
    let entries = vec![applicant_entry()];
    let ctx = housing_context();
    assert!(find_suggestion(&entries, Some(&ctx), "applicants", "email").is_none());
    assert!(find_suggestion(&entries, Some(&ctx), "applicants", "annual_income").is_some());
    assert!(find_suggestion(&entries, Some(&ctx), "users", "email").is_none());
    // No context → nothing resolves.
    assert!(find_suggestion(&entries, None, "applicants", "annual_income").is_none());
}

#[test]
fn url_path_uses_admin_name_and_field() {
    let entries = vec![applicant_entry()];
    let ctx = housing_context();
    let s = &derive_suggestions(&entries, Some(&ctx))[0];
    assert_eq!(s.url_path(), "/admin/suggestions/applicants/annual_income");
}
