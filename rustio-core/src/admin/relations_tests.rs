//! Unit tests for the Relation Intelligence Layer (0.9.0).
//!
//! Coverage goals from the design brief:
//!   1. Relation metadata survives schema serde round-trip.
//!   2. Registry builds a correct `belongs_to` index.
//!   3. Registry builds a correct `has_many` inverse index.
//!   4. Registry silently drops dangling target references but
//!      reports them via `validate`.
//!   5. Registry treats a missing `display_field` as "render #id"
//!      and never infers a column.
//!   6. Filter cap constant is exported and usable.
//!   7. An empty schema produces an empty registry (safe default).

use super::relations::{
    InverseRelation, RegistryError, RelationRegistry, ResolvedRelation,
    RELATION_FILTER_DROPDOWN_CAP,
};
use crate::schema::{
    Relation, RelationKind, Schema, SchemaField, SchemaModel, SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn plain_field(name: &str, ty: &str) -> SchemaField {
    SchemaField {
        name: name.to_string(),
        ty: ty.to_string(),
        nullable: false,
        editable: true,
        relation: None,
    }
}

fn fk_field(name: &str, target: &str, display: Option<&str>) -> SchemaField {
    SchemaField {
        name: name.to_string(),
        ty: "i64".to_string(),
        nullable: false,
        editable: true,
        relation: Some(Relation {
            model: target.to_string(),
            field: "id".to_string(),
            kind: RelationKind::BelongsTo,
            display_field: display.map(|s| s.to_string()),
        }),
    }
}

fn model(name: &str, table: &str, admin: &str, display: &str, fields: Vec<SchemaField>) -> SchemaModel {
    SchemaModel {
        name: name.to_string(),
        table: table.to_string(),
        admin_name: admin.to_string(),
        display_name: display.to_string(),
        singular_name: name.to_string(),
        fields,
        relations: Vec::new(),
        core: false,
    }
}

fn healthcare_schema() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: "0.9.0-test".to_string(),
        models: vec![
            model(
                "Patient",
                "patients",
                "patients",
                "Patients",
                vec![
                    plain_field("id", "i64"),
                    plain_field("full_name", "String"),
                ],
            ),
            model(
                "Doctor",
                "doctors",
                "doctors",
                "Doctors",
                vec![
                    plain_field("id", "i64"),
                    plain_field("full_name", "String"),
                    fk_field("department_id", "Department", Some("name")),
                ],
            ),
            model(
                "Department",
                "departments",
                "departments",
                "Departments",
                vec![plain_field("id", "i64"), plain_field("name", "String")],
            ),
            model(
                "Appointment",
                "appointments",
                "appointments",
                "Appointments",
                vec![
                    plain_field("id", "i64"),
                    fk_field("patient_id", "Patient", Some("full_name")),
                    fk_field("doctor_id", "Doctor", Some("full_name")),
                ],
            ),
            model(
                "Invoice",
                "invoices",
                "invoices",
                "Invoices",
                vec![
                    plain_field("id", "i64"),
                    // Deliberately without display_field to test the
                    // "render #id, no guessing" rule.
                    fk_field("patient_id", "Patient", None),
                ],
            ),
        ],
    }
}

// ---------------------------------------------------------------------------
// Test 1 — schema serde round-trip preserves display_field
// ---------------------------------------------------------------------------

#[test]
fn relation_metadata_round_trips_through_json() {
    let schema = healthcare_schema();
    let json = schema.to_pretty_json().expect("pretty json");
    let parsed = Schema::parse(&json).expect("parsed");

    let appointments = parsed
        .models
        .iter()
        .find(|m| m.name == "Appointment")
        .expect("Appointment model present after round-trip");

    let patient_id = appointments
        .fields
        .iter()
        .find(|f| f.name == "patient_id")
        .expect("patient_id field");
    let rel = patient_id
        .relation
        .as_ref()
        .expect("patient_id carries a relation");
    assert_eq!(rel.model, "Patient");
    assert_eq!(rel.field, "id");
    assert_eq!(rel.kind, RelationKind::BelongsTo);
    assert_eq!(
        rel.display_field.as_deref(),
        Some("full_name"),
        "display_field must survive round-trip"
    );
}

#[test]
fn relation_without_display_field_round_trips() {
    let schema = healthcare_schema();
    let json = schema.to_pretty_json().unwrap();
    let parsed = Schema::parse(&json).unwrap();

    let invoices = parsed
        .models
        .iter()
        .find(|m| m.name == "Invoice")
        .unwrap();
    let patient_id = invoices
        .fields
        .iter()
        .find(|f| f.name == "patient_id")
        .unwrap();
    let rel = patient_id.relation.as_ref().unwrap();
    assert!(
        rel.display_field.is_none(),
        "Invoice.patient_id was declared without display_field and must stay None"
    );
}

// ---------------------------------------------------------------------------
// Test 2 — belongs_to index is correct
// ---------------------------------------------------------------------------

#[test]
fn registry_indexes_belongs_to_entries() {
    let reg = RelationRegistry::from_schema(&healthcare_schema());

    let resolved = reg
        .belongs_to("Appointment", "patient_id")
        .expect("Appointment.patient_id resolved");
    assert_eq!(resolved.target_model, "Patient");
    assert_eq!(resolved.target_table, "patients");
    assert_eq!(resolved.target_admin_name, "patients");
    assert_eq!(resolved.target_display_field.as_deref(), Some("full_name"));

    // Doctor → Department with display="name"
    let dep = reg
        .belongs_to("Doctor", "department_id")
        .expect("Doctor.department_id resolved");
    assert_eq!(dep.target_model, "Department");
    assert_eq!(dep.target_display_field.as_deref(), Some("name"));

    // Invoice → Patient without display_field → None stays None (no
    // inference).
    let inv = reg
        .belongs_to("Invoice", "patient_id")
        .expect("Invoice.patient_id resolved");
    assert!(
        inv.target_display_field.is_none(),
        "no display_field declared → None (no full_name / name / title guessing)"
    );

    assert!(
        reg.belongs_to("Patient", "full_name").is_none(),
        "plain columns must not appear in the belongs_to index"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — has_many inverse index is correct
// ---------------------------------------------------------------------------

#[test]
fn registry_inverts_every_stored_belongs_to() {
    let reg = RelationRegistry::from_schema(&healthcare_schema());

    let patient_inverses: Vec<&InverseRelation> = reg.has_many("Patient").iter().collect();
    let sources: Vec<&str> = patient_inverses
        .iter()
        .map(|i| i.source_model.as_str())
        .collect();
    assert!(
        sources.contains(&"Appointment"),
        "Patient must see Appointment among its inverses: {sources:?}"
    );
    assert!(
        sources.contains(&"Invoice"),
        "Patient must see Invoice among its inverses: {sources:?}"
    );

    let doctor_inverses = reg.has_many("Doctor");
    assert!(
        doctor_inverses.iter().any(|i| i.source_model == "Appointment"),
        "Doctor must see Appointment as an inverse"
    );

    let dept_inverses = reg.has_many("Department");
    assert_eq!(dept_inverses.len(), 1);
    assert_eq!(dept_inverses[0].source_model, "Doctor");
    assert_eq!(dept_inverses[0].source_field, "department_id");

    // A model with no inverses returns an empty slice, not None.
    assert!(reg.has_many("Appointment").is_empty());
}

// ---------------------------------------------------------------------------
// Test 4 — dangling target references are skipped + reported
// ---------------------------------------------------------------------------

#[test]
fn dangling_target_is_skipped_at_build_and_reported_by_validate() {
    let mut schema = healthcare_schema();
    // Corrupt the schema: point Appointment.patient_id at a model
    // that doesn't exist. Simulates a hand-edited rustio.schema.json
    // referencing a deleted model.
    schema
        .models
        .iter_mut()
        .find(|m| m.name == "Appointment")
        .unwrap()
        .fields
        .iter_mut()
        .find(|f| f.name == "patient_id")
        .unwrap()
        .relation
        .as_mut()
        .unwrap()
        .model = "Ghost".to_string();

    let reg = RelationRegistry::from_schema(&schema);
    assert!(
        reg.belongs_to("Appointment", "patient_id").is_none(),
        "dangling relation must not appear in the index"
    );

    let errors = reg.validate(&schema);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            RegistryError::UnknownTarget { model, field, target }
                if model == "Appointment" && field == "patient_id" && target == "Ghost"
        )),
        "validate() must surface the dangling target: {errors:?}"
    );
}

#[test]
fn unknown_display_field_is_skipped_at_build_and_reported_by_validate() {
    let mut schema = healthcare_schema();
    // Point Appointment.patient_id at Patient with a nonexistent
    // display column. Patient has `full_name` but not `ghost`.
    schema
        .models
        .iter_mut()
        .find(|m| m.name == "Appointment")
        .unwrap()
        .fields
        .iter_mut()
        .find(|f| f.name == "patient_id")
        .unwrap()
        .relation
        .as_mut()
        .unwrap()
        .display_field = Some("ghost".to_string());

    let reg = RelationRegistry::from_schema(&schema);
    let resolved = reg.belongs_to("Appointment", "patient_id").unwrap();
    assert!(
        resolved.target_display_field.is_none(),
        "registry must drop a display_field that doesn't exist on the target (falls back to #id)"
    );

    let errors = reg.validate(&schema);
    assert!(
        errors.iter().any(|e| matches!(
            e,
            RegistryError::UnknownDisplayField { display, .. } if display == "ghost"
        )),
        "validate() must report the missing display_field: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — empty schema + empty registry
// ---------------------------------------------------------------------------

#[test]
fn empty_schema_produces_empty_registry() {
    let schema = Schema {
        version: SCHEMA_VERSION,
        rustio_version: "test".into(),
        models: Vec::new(),
    };
    let reg = RelationRegistry::from_schema(&schema);
    assert!(reg.is_empty());
    assert!(reg.belongs_to("anything", "anywhere").is_none());
    assert!(reg.has_many("anything").is_empty());
    assert!(reg.validate(&schema).is_empty());
}

#[test]
fn empty_registry_is_safe_default() {
    let reg = RelationRegistry::empty();
    assert!(reg.is_empty());
    assert!(reg.iter_belongs_to().next().is_none());
}

// ---------------------------------------------------------------------------
// Test 6 — filter cap constant
// ---------------------------------------------------------------------------

#[test]
fn relation_filter_dropdown_cap_is_500() {
    assert_eq!(RELATION_FILTER_DROPDOWN_CAP, 500);
}

// ---------------------------------------------------------------------------
// Test 7 — belongs_to_of and iter_belongs_to determinism
// ---------------------------------------------------------------------------

#[test]
fn belongs_to_of_lists_every_fk_on_a_model() {
    let reg = RelationRegistry::from_schema(&healthcare_schema());
    let on_appointment = reg.belongs_to_of("Appointment");
    let field_names: Vec<&str> = on_appointment
        .iter()
        .map(|r| r.source_field.as_str())
        .collect();
    assert_eq!(field_names, ["doctor_id", "patient_id"], "sorted");

    // A model with no FKs returns an empty slice.
    assert!(reg.belongs_to_of("Patient").is_empty());
}

#[test]
fn iter_belongs_to_is_deterministic() {
    let reg = RelationRegistry::from_schema(&healthcare_schema());
    let first: Vec<(String, String)> = reg
        .iter_belongs_to()
        .map(|r| (r.source_model.clone(), r.source_field.clone()))
        .collect();
    let second: Vec<(String, String)> = reg
        .iter_belongs_to()
        .map(|r| (r.source_model.clone(), r.source_field.clone()))
        .collect();
    assert_eq!(first, second, "iter order must be stable across calls");
}

// ---------------------------------------------------------------------------
// Resolved-shape sanity
// ---------------------------------------------------------------------------

#[test]
fn resolved_relation_carries_admin_slug_and_table() {
    let reg = RelationRegistry::from_schema(&healthcare_schema());
    let r: &ResolvedRelation = reg.belongs_to("Appointment", "patient_id").unwrap();
    assert_eq!(r.source_field, "patient_id");
    assert_eq!(r.target_admin_name, "patients");
    assert_eq!(r.target_table, "patients");
    // Both admin_name and table happen to be "patients" here, but
    // the registry carries them separately so future models with
    // admin_name != table_name work.
}
