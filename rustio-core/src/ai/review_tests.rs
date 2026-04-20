//! Tests for the Plan Review Layer.
//!
//! Invariants every test reinforces:
//!
//! - Review is **deterministic** — risk, impact, warnings derive from
//!   the plan alone (and the schema, for impact/stale detection).
//! - Review **never executes** anything — no FS, no DB.
//! - `CreateMigration` and core-model modifications always produce
//!   `Critical` risk with a matching warning.
//! - Stale-plan detection produces `ValidationOutcome::Invalid` with
//!   the exact failing step index.

use chrono::{TimeZone, Utc};

use super::planner::PlanResult;
use super::review::{
    build_plan_document_with_timestamp, classify_risk, compute_impact, load_plan,
    render_plan_document_json, render_review_human, review_plan, warnings_for, LoadedPlan,
    PlanImpact, PlanReview, ReviewError, ReviewHeader, RiskLevel, ValidationOutcome,
    PLAN_DOCUMENT_VERSION,
};
use super::{
    AddField, AddModel, ChangeFieldNullability, ChangeFieldType, CreateMigration, FieldSpec, Plan,
    Primitive, RemoveField, RenameField, RenameModel,
};
use crate::schema::{Schema, SchemaField, SchemaModel, SCHEMA_VERSION};

fn pkg_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap()
}

fn task_schema() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![SchemaModel {
            name: "Task".into(),
            table: "tasks".into(),
            admin_name: "tasks".into(),
            display_name: "Tasks".into(),
            singular_name: "Task".into(),
            fields: vec![
                SchemaField {
                    name: "id".into(),
                    ty: "i64".into(),
                    nullable: false,
                    editable: false,
                    relation: None,
                },
                SchemaField {
                    name: "title".into(),
                    ty: "String".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                },
            ],
            relations: vec![],
            core: false,
        }],
    }
}

fn schema_with_core_user() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![SchemaModel {
            name: "User".into(),
            table: "rustio_users".into(),
            admin_name: "users".into(),
            display_name: "Users".into(),
            singular_name: "User".into(),
            fields: vec![
                SchemaField {
                    name: "id".into(),
                    ty: "i64".into(),
                    nullable: false,
                    editable: false,
                    relation: None,
                },
                SchemaField {
                    name: "email".into(),
                    ty: "String".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                },
            ],
            relations: vec![],
            core: true,
        }],
    }
}

fn add_field_plan(model: &str, name: &str, ty: &str, nullable: bool) -> Plan {
    Plan::new(vec![Primitive::AddField(AddField {
        model: model.into(),
        field: FieldSpec {
            name: name.into(),
            ty: ty.into(),
            nullable,
            editable: true,
        },
    })])
}

// ---------- building a document from an AddField ----------

#[test]
fn build_document_from_simple_add_field() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);
    let result = PlanResult {
        plan,
        explanation: "Adds field `priority` (i32) to model `Task`.".into(),
    };
    let doc = build_plan_document_with_timestamp(
        &schema,
        "Add priority to tasks",
        &result,
        fixed_ts(),
        None,
    )
    .unwrap();
    assert_eq!(doc.version, PLAN_DOCUMENT_VERSION);
    assert_eq!(doc.created_at, "2026-01-01T00:00:00Z");
    assert_eq!(doc.prompt, "Add priority to tasks");
    assert_eq!(doc.risk, RiskLevel::Low);
    assert_eq!(doc.impact.adds_fields, 1);
    assert!(!doc.impact.destructive);
    assert!(!doc.impact.touches_core_models);
    assert_eq!(doc.plan.steps.len(), 1);
}

#[test]
fn build_document_rejects_invalid_plan() {
    // Hand-craft a plan whose AddField targets a non-existent model.
    let schema = task_schema();
    let bad_plan = add_field_plan("Ghost", "x", "i32", false);
    let result = PlanResult {
        plan: bad_plan,
        explanation: String::new(),
    };
    let err = build_plan_document_with_timestamp(&schema, "stuff", &result, fixed_ts(), None)
        .expect_err("should refuse to document an invalid plan");
    assert!(matches!(err, ReviewError::InvalidPlan(_)));
}

// ---------- risk classification ----------

#[test]
fn low_risk_for_plain_nullable_add_field() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "notes", "String", true);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Low);
    assert!(review.validation.is_valid());
    assert!(
        review.warnings.is_empty(),
        "warnings: {:?}",
        review.warnings
    );
}

#[test]
fn low_risk_for_nonnull_add_field_too() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Low);
}

#[test]
fn high_risk_for_remove_field() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::RemoveField(RemoveField {
        model: "Task".into(),
        field: "title".into(),
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::High);
    assert!(review.impact.destructive);
    assert_eq!(review.impact.removes_fields, 1);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("removes a field")));
}

#[test]
fn medium_risk_for_rename_field() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::RenameField(RenameField {
        model: "Task".into(),
        from: "title".into(),
        to: "headline".into(),
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Medium);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("renames a field")));
}

#[test]
fn medium_risk_for_rename_model() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::RenameModel(RenameModel {
        from: "Task".into(),
        to: "Todo".into(),
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Medium);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("renames a model")));
}

#[test]
fn high_risk_for_tightening_nullability() {
    // 0.5.3: tightening to required rewrites the table and replaces
    // NULLs with a type default — conservative bump to High.
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::ChangeFieldNullability(
        ChangeFieldNullability {
            model: "Task".into(),
            field: "title".into(),
            nullable: false,
        },
    )]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::High);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("nullable to required")));
    assert!(
        review
            .warnings
            .iter()
            .any(|w| w.contains("rewrites the entire table")),
        "expected table-rewrite warning, got {:?}",
        review.warnings,
    );
}

#[test]
fn low_risk_for_nullable_change() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::ChangeFieldNullability(
        ChangeFieldNullability {
            model: "Task".into(),
            field: "title".into(),
            nullable: true,
        },
    )]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Low);
}

#[test]
fn medium_risk_for_type_change() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Task".into(),
        field: "title".into(),
        new_type: "String".into(),
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Medium);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("changes a field's type")));
}

// ---------- multiple operations + mixed plan ----------

#[test]
fn plan_with_multiple_ops_gets_warning() {
    let schema = task_schema();
    let plan = Plan::new(vec![
        Primitive::AddField(AddField {
            model: "Task".into(),
            field: FieldSpec {
                name: "priority".into(),
                ty: "i32".into(),
                nullable: false,
                editable: true,
            },
        }),
        Primitive::RenameField(RenameField {
            model: "Task".into(),
            from: "title".into(),
            to: "headline".into(),
        }),
    ]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("performs 2 operations")));
}

#[test]
fn mixed_add_and_remove_bumps_to_high() {
    let schema = task_schema();
    let plan = Plan::new(vec![
        Primitive::AddField(AddField {
            model: "Task".into(),
            field: FieldSpec {
                name: "priority".into(),
                ty: "i32".into(),
                nullable: false,
                editable: true,
            },
        }),
        Primitive::RemoveField(RemoveField {
            model: "Task".into(),
            field: "title".into(),
        }),
    ]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::High);
    assert!(review.impact.destructive);
    assert_eq!(review.impact.adds_fields, 1);
    assert_eq!(review.impact.removes_fields, 1);
}

// ---------- critical: core models, invalid plan, developer-only ----------

#[test]
fn touching_core_model_is_critical() {
    let schema = schema_with_core_user();
    // Hand-craft a plan that modifies User — the planner would refuse
    // this, but a saved document / hand-edited JSON might carry it.
    let plan = add_field_plan("User", "nickname", "String", true);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Critical);
    assert!(review.impact.touches_core_models);
}

#[test]
fn invalid_plan_is_critical() {
    let schema = task_schema();
    // `title` already exists — add_field will fail validation.
    let plan = add_field_plan("Task", "title", "String", false);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert!(matches!(
        review.validation,
        ValidationOutcome::Invalid { .. }
    ));
    assert_eq!(review.risk, RiskLevel::Critical);
}

#[test]
fn developer_only_primitive_in_plan_is_critical_and_warned() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::CreateMigration(CreateMigration {
        name: "bad".into(),
        sql: "DROP TABLE tasks".into(),
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Critical);
    assert!(review
        .warnings
        .iter()
        .any(|w| w.contains("developer-only primitive")));
    // And validation itself must reject it.
    assert!(matches!(
        review.validation,
        ValidationOutcome::Invalid { .. }
    ));
}

// ---------- stale plan detection ----------

#[test]
fn plan_valid_today_becomes_stale_after_schema_change() {
    let mut schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);

    // Today: valid.
    let review = review_plan(&schema, &plan, None).unwrap();
    assert!(review.validation.is_valid());

    // Someone (a human, a migration, another plan) adds `priority`
    // to Task. Our saved plan is now stale.
    schema.models[0].fields.push(SchemaField {
        name: "priority".into(),
        ty: "i32".into(),
        nullable: false,
        editable: true,
        relation: None,
    });

    let review2 = review_plan(&schema, &plan, None).unwrap();
    match review2.validation {
        ValidationOutcome::Invalid { step, reason: _ } => assert_eq!(step, 0),
        _ => panic!("expected Invalid, got {:?}", review2.validation),
    }
    assert_eq!(review2.risk, RiskLevel::Critical);
    // Human renderer must spell out the "stale" story.
    let text = render_review_human(&review2, None);
    assert!(
        text.contains("FAILS at step 0"),
        "human renderer should point at the failing step: {text}",
    );
    assert!(
        text.contains("stale"),
        "human renderer should use the word `stale`: {text}",
    );
}

#[test]
fn stale_detection_points_at_correct_step() {
    let schema = task_schema();
    // Two-step plan. Step 0 is fine. Step 1 removes a field that
    // doesn't exist → should invalidate specifically at step 1.
    let plan = Plan::new(vec![
        Primitive::AddField(AddField {
            model: "Task".into(),
            field: FieldSpec {
                name: "priority".into(),
                ty: "i32".into(),
                nullable: false,
                editable: true,
            },
        }),
        Primitive::RemoveField(RemoveField {
            model: "Task".into(),
            field: "ghost".into(),
        }),
    ]);
    let review = review_plan(&schema, &plan, None).unwrap();
    match review.validation {
        ValidationOutcome::Invalid { step, .. } => assert_eq!(step, 1),
        _ => panic!("expected Invalid"),
    }
}

// ---------- loading: document + raw plan ----------

#[test]
fn load_full_plan_document_round_trips() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);
    let result = PlanResult {
        plan,
        explanation: "doc".into(),
    };
    let doc =
        build_plan_document_with_timestamp(&schema, "add priority", &result, fixed_ts(), None)
            .unwrap();
    let json = render_plan_document_json(&doc).unwrap();
    match load_plan(&json).unwrap() {
        LoadedPlan::Document(d) => {
            assert_eq!(d, doc);
            assert_eq!(d.version, PLAN_DOCUMENT_VERSION);
        }
        LoadedPlan::RawPlan(_) => panic!("expected Document, got RawPlan"),
    }
}

#[test]
fn load_raw_plan_also_works() {
    let raw = r#"{
  "steps": [
    {"op": "add_field", "model": "Task", "name": "priority", "type": "i32", "nullable": false}
  ]
}"#;
    match load_plan(raw).unwrap() {
        LoadedPlan::RawPlan(p) => {
            assert_eq!(p.steps.len(), 1);
        }
        LoadedPlan::Document(_) => panic!("expected RawPlan, got Document"),
    }
}

#[test]
fn load_refuses_unknown_document_version() {
    let json = r#"{
  "version": 99,
  "created_at": "2026-01-01T00:00:00Z",
  "prompt": "",
  "explanation": "",
  "risk": "low",
  "impact": {
    "adds_fields": 0, "removes_fields": 0, "renames": 0, "type_changes": 0,
    "nullability_changes": 0, "touches_core_models": false, "destructive": false
  },
  "plan": {"steps": []}
}"#;
    let err = load_plan(json).expect_err("should refuse");
    match err {
        ReviewError::UnknownVersion { found, expected } => {
            assert_eq!(found, 99);
            assert_eq!(expected, PLAN_DOCUMENT_VERSION);
        }
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn load_refuses_garbage() {
    let err = load_plan("not json at all").expect_err("should refuse");
    assert!(matches!(err, ReviewError::Parse(_)));
}

#[test]
fn load_refuses_unknown_fields_in_document() {
    // Extra `sneaky` field must be rejected by deny_unknown_fields.
    let json = r#"{
  "version": 1,
  "created_at": "2026-01-01T00:00:00Z",
  "prompt": "",
  "explanation": "",
  "risk": "low",
  "impact": {
    "adds_fields": 0, "removes_fields": 0, "renames": 0, "type_changes": 0,
    "nullability_changes": 0, "touches_core_models": false, "destructive": false
  },
  "plan": {"steps": []},
  "sneaky": "execute me"
}"#;
    // With an unknown field the document parse fails; the loader
    // falls through to the raw-plan parse, which also fails (no
    // `steps` top-level) → Parse error.
    assert!(matches!(load_plan(json), Err(ReviewError::Parse(_))));
}

// ---------- rendering ----------

#[test]
fn human_render_reads_like_changelog() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);
    let review = review_plan(&schema, &plan, None).unwrap();
    let header = ReviewHeader {
        prompt: Some("Add priority to tasks".into()),
        explanation: Some("Adds priority to Task for sorting.".into()),
        source: Some("tasks-priority.json".into()),
    };
    let text = render_review_human(&review, Some(&header));
    assert!(text.starts_with("Plan review\n"));
    assert!(text.contains("Prompt:\n  Add priority to tasks"));
    assert!(text.contains("Risk:\n  Low"));
    assert!(text.contains("Impact:"));
    assert!(text.contains("Planned changes:"));
    assert!(text.contains("Validation:\n  - Passes against the current schema."));
    assert!(text.contains("Warnings:\n  - None"));
}

#[test]
fn render_plan_document_json_is_deterministic() {
    let schema = task_schema();
    let plan = add_field_plan("Task", "priority", "i32", false);
    let result = PlanResult {
        plan,
        explanation: "x".into(),
    };
    let doc =
        build_plan_document_with_timestamp(&schema, "add priority", &result, fixed_ts(), None)
            .unwrap();
    let a = render_plan_document_json(&doc).unwrap();
    let b = render_plan_document_json(&doc).unwrap();
    assert_eq!(a, b);
    // Trailing newline (convention shared with rustio.schema.json).
    assert!(a.ends_with('\n'));
    // Timestamp is the pinned one, not "now".
    assert!(a.contains("\"created_at\": \"2026-01-01T00:00:00Z\""));
    // Keys we promised are all present.
    for key in [
        "\"version\"",
        "\"prompt\"",
        "\"explanation\"",
        "\"risk\"",
        "\"impact\"",
        "\"plan\"",
    ] {
        assert!(a.contains(key), "missing key {key} in:\n{a}");
    }
}

#[test]
fn warnings_for_is_free_standing_and_deterministic() {
    let plan = Plan::new(vec![
        Primitive::RemoveField(RemoveField {
            model: "Task".into(),
            field: "title".into(),
        }),
        Primitive::RenameField(RenameField {
            model: "Task".into(),
            from: "priority".into(),
            to: "rank".into(),
        }),
    ]);
    let a = warnings_for(&plan, None);
    let b = warnings_for(&plan, None);
    assert_eq!(a, b, "warnings must be deterministic");
    assert!(a.iter().any(|w| w.contains("removes a field")));
    assert!(a.iter().any(|w| w.contains("renames a field")));
    assert!(a.iter().any(|w| w.contains("performs 2 operations")));
}

// ---------- impact and classifier are free-standing helpers ----------

#[test]
fn compute_impact_counts_each_kind() {
    let schema = task_schema();
    let plan = Plan::new(vec![
        Primitive::AddField(AddField {
            model: "Task".into(),
            field: FieldSpec {
                name: "priority".into(),
                ty: "i32".into(),
                nullable: true,
                editable: true,
            },
        }),
        Primitive::RenameField(RenameField {
            model: "Task".into(),
            from: "title".into(),
            to: "headline".into(),
        }),
        Primitive::ChangeFieldType(ChangeFieldType {
            model: "Task".into(),
            field: "title".into(),
            new_type: "String".into(),
        }),
        Primitive::ChangeFieldNullability(ChangeFieldNullability {
            model: "Task".into(),
            field: "title".into(),
            nullable: true,
        }),
    ]);
    let i = compute_impact(&plan, &schema);
    assert_eq!(i.adds_fields, 1);
    assert_eq!(i.renames, 1);
    assert_eq!(i.type_changes, 1);
    assert_eq!(i.nullability_changes, 1);
    assert!(!i.destructive);
    assert!(!i.touches_core_models);
}

#[test]
fn classify_risk_is_pure_over_plan_plus_impact_plus_validation() {
    // A trivial plan and the same inputs must always yield the same
    // risk — no Utc::now, no globals.
    let plan = add_field_plan("Task", "priority", "i32", false);
    let impact = PlanImpact::default();
    let a = classify_risk(&plan, &impact, &ValidationOutcome::Valid, None);
    let b = classify_risk(&plan, &impact, &ValidationOutcome::Valid, None);
    assert_eq!(a, b);
}

// ---------- sanity around add_model ----------

#[test]
fn add_model_primitive_is_low_risk_and_not_core() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::AddModel(AddModel {
        name: "Tag".into(),
        table: "tags".into(),
        fields: vec![FieldSpec {
            name: "label".into(),
            ty: "String".into(),
            nullable: false,
            editable: true,
        }],
    })]);
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Low);
    assert!(!review.impact.touches_core_models);
    assert!(review.validation.is_valid());
}

// ---------- symmetry check for the ReviewReview output ----------

fn as_simple_review(r: &PlanReview) -> (RiskLevel, bool, usize) {
    (r.risk, r.validation.is_valid(), r.warnings.len())
}

#[test]
fn reviewing_same_plan_twice_is_byte_identical() {
    let schema = task_schema();
    let plan = Plan::new(vec![Primitive::RemoveField(RemoveField {
        model: "Task".into(),
        field: "title".into(),
    })]);
    let a = review_plan(&schema, &plan, None).unwrap();
    let b = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(as_simple_review(&a), as_simple_review(&b));
    assert_eq!(a.warnings, b.warnings);
    assert_eq!(a.impact, b.impact);
}

// ---------- 0.8.0 relations ----------

fn housing_schema_for_review() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![
            SchemaModel {
                name: "Applicant".into(),
                table: "applicants".into(),
                admin_name: "applicants".into(),
                display_name: "Applicants".into(),
                singular_name: "Applicant".into(),
                fields: vec![
                    SchemaField {
                        name: "id".into(),
                        ty: "i64".into(),
                        nullable: false,
                        editable: false,
                        relation: None,
                    },
                    SchemaField {
                        name: "personnummer".into(),
                        ty: "String".into(),
                        nullable: false,
                        editable: true,
                        relation: None,
                    },
                ],
                relations: vec![],
                core: false,
            },
            SchemaModel {
                name: "Application".into(),
                table: "applications".into(),
                admin_name: "applications".into(),
                display_name: "Applications".into(),
                singular_name: "Application".into(),
                fields: vec![SchemaField {
                    name: "id".into(),
                    ty: "i64".into(),
                    nullable: false,
                    editable: false,
                    relation: None,
                }],
                relations: vec![],
                core: false,
            },
        ],
    }
}

fn add_relation_plan_review(from: &str, to: &str, via: &str) -> Plan {
    Plan::new(vec![Primitive::AddRelation(super::AddRelation {
        from: from.into(),
        kind: crate::schema::RelationKind::BelongsTo,
        to: to.into(),
        via: via.into(),
    })])
}

#[test]
fn add_relation_is_low_risk() {
    let schema = housing_schema_for_review();
    let plan = add_relation_plan_review("Application", "Applicant", "applicant_id");
    let review = review_plan(&schema, &plan, None).unwrap();
    assert_eq!(review.risk, RiskLevel::Low);
    assert!(review.validation.is_valid());
}

#[test]
fn add_relation_warns_about_missing_fk_constraint() {
    let schema = housing_schema_for_review();
    let plan = add_relation_plan_review("Application", "Applicant", "applicant_id");
    let review = review_plan(&schema, &plan, None).unwrap();
    assert!(
        review
            .warnings
            .iter()
            .any(|w| w.contains("foreign-key") && w.contains("Applicant")),
        "expected a FK-gap warning naming the target; got {:?}",
        review.warnings,
    );
}

#[test]
fn add_relation_to_pii_target_raises_gdpr_warning() {
    // Target model `Applicant` carries `personnummer` — PII under
    // country=SE. Linking something to it must flag the GDPR risk.
    let schema = housing_schema_for_review();
    let plan = add_relation_plan_review("Application", "Applicant", "applicant_id");
    let ctx = super::planner::ContextConfig {
        country: Some("SE".into()),
        ..Default::default()
    };
    let review = review_plan(&schema, &plan, Some(&ctx)).unwrap();
    assert_eq!(
        review.risk,
        RiskLevel::Low,
        "PII on target bumps via a warning, not risk — the relation itself is additive",
    );
    assert!(
        review
            .warnings
            .iter()
            .any(|w| w.contains("personnummer") && w.contains("GDPR")),
        "expected a GDPR warning naming the PII field; got {:?}",
        review.warnings,
    );
}

#[test]
fn add_relation_to_non_pii_target_does_not_raise_gdpr_warning() {
    // Country=SE but target has no PII columns — no GDPR bullet.
    let mut schema = housing_schema_for_review();
    schema
        .models
        .iter_mut()
        .find(|m| m.name == "Applicant")
        .unwrap()
        .fields
        .retain(|f| f.name != "personnummer");
    let plan = add_relation_plan_review("Application", "Applicant", "applicant_id");
    let ctx = super::planner::ContextConfig {
        country: Some("SE".into()),
        ..Default::default()
    };
    let review = review_plan(&schema, &plan, Some(&ctx)).unwrap();
    assert!(
        !review.warnings.iter().any(|w| w.contains("GDPR")),
        "no GDPR warning should fire when target has no PII: {:?}",
        review.warnings,
    );
}
