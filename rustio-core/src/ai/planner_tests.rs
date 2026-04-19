//! Tests for the rule-based AI planner. Kept in its own file (rather
//! than inline) because the grammar surface is wide enough that the
//! test list is long and benefits from being read on its own.
//!
//! Invariants every test enforces:
//!
//! - Planner output **always** passes `Plan::validate(&schema)`.
//! - Planner **never** emits `CreateMigration` (developer-only).
//! - Planner is deterministic — same prompt + schema → same plan.

use super::planner::{
    generate_plan, render_plan_human, render_plan_json, ContextConfig, PlanError, PlanRequest,
};
use super::{Primitive, RelationKind};
use crate::schema::{Schema, SchemaField, SchemaModel, SCHEMA_VERSION};

fn pkg_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
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

fn applicant_schema() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![SchemaModel {
            name: "Applicant".into(),
            table: "applicants".into(),
            admin_name: "applicants".into(),
            display_name: "Applicants".into(),
            singular_name: "Applicant".into(),
            fields: vec![SchemaField {
                name: "id".into(),
                ty: "i64".into(),
                nullable: false,
                editable: false,
                relation: None,
            }],
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

// ---------- simple add field ----------

#[test]
fn simple_add_field_infers_i32_for_priority() {
    let schema = task_schema();
    let res = generate_plan(&schema, None, PlanRequest::new("Add priority to tasks"))
        .expect("plan should succeed");
    assert_eq!(res.plan.steps.len(), 1);
    match &res.plan.steps[0] {
        Primitive::AddField(a) => {
            assert_eq!(a.model, "Task");
            assert_eq!(a.field.name, "priority");
            assert_eq!(a.field.ty, "i32");
            assert!(!a.field.nullable);
            assert!(a.field.editable);
        }
        other => panic!("expected AddField, got {other:?}"),
    }
    // Plan must survive the real Plan::validate against the same schema.
    res.plan.validate(&schema).unwrap();
    assert!(res.explanation.contains("priority"));
}

#[test]
fn monetary_names_infer_i64() {
    // 0.7.2: `*_income`, `*_amount`, `*_total`, `price`, `balance`
    // resolve to `i64` — we store monetary amounts in minor units
    // where `i32` can overflow.
    let schema = task_schema();
    for (prompt, field, expected_ty) in [
        ("add annual_income to tasks", "annual_income", "i64"),
        ("add balance to tasks", "balance", "i64"),
        ("add price to tasks", "price", "i64"),
        ("add total_amount to tasks", "total_amount", "i64"),
        ("add order_total to tasks", "order_total", "i64"),
    ] {
        let res = generate_plan(&schema, None, PlanRequest::new(prompt))
            .unwrap_or_else(|e| panic!("prompt `{prompt}` failed: {e}"));
        match &res.plan.steps[0] {
            Primitive::AddField(a) => {
                assert_eq!(a.field.name, field, "field mismatch for `{prompt}`");
                assert_eq!(a.field.ty, expected_ty, "type mismatch for `{prompt}`",);
            }
            other => panic!("expected AddField for `{prompt}`, got {other:?}"),
        }
    }
}

#[test]
fn count_suffix_still_infers_i32() {
    // Regression canary: adding the `_income`/`_amount`/`_total`
    // branch must not disturb the existing `_count` → i32 rule.
    let schema = task_schema();
    let res = generate_plan(&schema, None, PlanRequest::new("add order_count to tasks")).unwrap();
    match &res.plan.steps[0] {
        Primitive::AddField(a) => assert_eq!(a.field.ty, "i32"),
        _ => panic!("wrong primitive"),
    }
}

#[test]
fn add_due_date_infers_datetime_and_nullable_on_hint() {
    let schema = task_schema();
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("Add optional due date to tasks"),
    )
    .unwrap();
    match &res.plan.steps[0] {
        Primitive::AddField(a) => {
            assert_eq!(a.field.name, "due_date");
            assert_eq!(a.field.ty, "DateTime");
            assert!(a.field.nullable);
        }
        _ => panic!("wrong primitive"),
    }
}

#[test]
fn add_field_with_explicit_type_wins_over_heuristics() {
    let schema = task_schema();
    // `priority` would normally resolve to i32; `as i64` forces it.
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("add priority as i64 to tasks"),
    )
    .unwrap();
    match &res.plan.steps[0] {
        Primitive::AddField(a) => {
            assert_eq!(a.field.ty, "i64");
        }
        _ => panic!("wrong primitive"),
    }
}

#[test]
fn add_field_that_already_exists_is_rejected() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("add title to tasks"))
        .expect_err("should reject");
    match err {
        PlanError::FieldAlreadyExists { model, field } => {
            assert_eq!(model, "Task");
            assert_eq!(field, "title");
        }
        other => panic!("wrong error: {other:?}"),
    }
}

// ---------- rename field ----------

#[test]
fn rename_field_emits_rename_primitive_not_remove_plus_add() {
    let schema = task_schema();
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("rename title to name in tasks"),
    )
    .unwrap();
    assert_eq!(res.plan.steps.len(), 1);
    match &res.plan.steps[0] {
        Primitive::RenameField(r) => {
            assert_eq!(r.model, "Task");
            assert_eq!(r.from, "title");
            assert_eq!(r.to, "name");
        }
        other => panic!("expected RenameField, got {other:?}"),
    }
    res.plan.validate(&schema).unwrap();
}

#[test]
fn rename_missing_field_errors() {
    let schema = task_schema();
    let err = generate_plan(
        &schema,
        None,
        PlanRequest::new("rename nope to something in tasks"),
    )
    .expect_err("should reject");
    assert!(matches!(err, PlanError::FieldDoesNotExist { .. }));
}

#[test]
fn rename_field_collides_with_existing_target() {
    let mut schema = task_schema();
    schema.models[0].fields.push(SchemaField {
        name: "name".into(),
        ty: "String".into(),
        nullable: false,
        editable: true,
                relation: None,
    });
    let err = generate_plan(
        &schema,
        None,
        PlanRequest::new("rename title to name in tasks"),
    )
    .expect_err("should reject");
    match err {
        PlanError::FieldAlreadyExists { model, field } => {
            assert_eq!(model, "Task");
            assert_eq!(field, "name");
        }
        other => panic!("wrong error: {other:?}"),
    }
}

// ---------- invalid model ----------

#[test]
fn unknown_model_is_reported_with_hint() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("add priority to widgets"))
        .expect_err("should reject");
    match err {
        PlanError::UnknownModel { hint } => assert_eq!(hint, "widgets"),
        other => panic!("wrong error: {other:?}"),
    }
}

// ---------- context-aware ----------

#[test]
fn swedish_context_upgrades_personnummer_to_string() {
    let schema = applicant_schema();
    let ctx = ContextConfig {
        country: Some("SE".into()),
        ..Default::default()
    };
    let res = generate_plan(
        &schema,
        Some(&ctx),
        PlanRequest::new("add personnummer to applicants"),
    )
    .unwrap();
    match &res.plan.steps[0] {
        Primitive::AddField(a) => {
            assert_eq!(a.field.name, "personnummer");
            assert_eq!(a.field.ty, "String");
            assert!(!a.field.nullable);
        }
        other => panic!("expected AddField, got {other:?}"),
    }
    assert!(
        res.explanation.to_lowercase().contains("personnummer"),
        "explanation should mention personnummer: {}",
        res.explanation,
    );
}

#[test]
fn without_context_personnummer_still_parses_but_has_generic_rationale() {
    let schema = applicant_schema();
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("add personnummer to applicants"),
    )
    .unwrap();
    // Without SE context we still infer String (identifier fallback).
    match &res.plan.steps[0] {
        Primitive::AddField(a) => assert_eq!(a.field.ty, "String"),
        other => panic!("expected AddField, got {other:?}"),
    }
    // But no Swedish sentence.
    assert!(
        !res.explanation.contains("Swedish"),
        "explanation should not reference Sweden without context: {}",
        res.explanation,
    );
}

// ---------- plan validation failure ----------

#[test]
fn emitted_plan_always_passes_plan_validate() {
    // Property-style: across every prompt that succeeds, the returned
    // plan must survive `Plan::validate(&schema)`.
    let schema = task_schema();
    let prompts = [
        "add priority to tasks",
        "add optional notes to tasks",
        "rename title to name in tasks",
        "change title in tasks to String",
        "make title in tasks optional",
        "remove title from tasks",
    ];
    for p in prompts {
        let res = generate_plan(&schema, None, PlanRequest::new(p))
            .unwrap_or_else(|e| panic!("prompt `{p}` failed: {e}"));
        res.plan
            .validate(&schema)
            .unwrap_or_else(|e| panic!("prompt `{p}` produced invalid plan: {e}"));
        // And no developer-only primitives slipped in.
        for step in &res.plan.steps {
            assert!(
                !step.is_developer_only(),
                "planner emitted a developer-only primitive for `{p}`",
            );
        }
    }
}

#[test]
fn planner_never_emits_create_migration() {
    let schema = task_schema();
    let res = generate_plan(&schema, None, PlanRequest::new("add priority to tasks")).unwrap();
    for step in &res.plan.steps {
        assert!(
            !matches!(step, Primitive::CreateMigration(_)),
            "planner must never emit CreateMigration"
        );
    }
}

#[test]
fn developer_only_requests_are_rejected_loudly() {
    let schema = task_schema();
    for bad in [
        "create migration foo",
        "run sql: drop table tasks",
        "execute sql DELETE FROM tasks",
        "add raw sql to tasks",
    ] {
        let err = generate_plan(&schema, None, PlanRequest::new(bad))
            .expect_err(&format!("should reject `{bad}`"));
        assert!(
            matches!(err, PlanError::DeveloperOnlyRequested(_)),
            "wrong error for `{bad}`: {err:?}",
        );
    }
}

// ---------- chaining (rename → change type) via two sequential calls ----------

#[test]
fn rename_then_change_type_composes_as_two_validated_plans() {
    // Each plan is single-step in 0.5.0; the caller chains by applying
    // the shadow of plan 1 to the schema before building plan 2.
    let mut schema = task_schema();

    let plan1 = generate_plan(
        &schema,
        None,
        PlanRequest::new("rename title to name in tasks"),
    )
    .unwrap();
    plan1.plan.validate(&schema).unwrap();
    // Shadow-apply step 1 to the schema so step 2 sees the new name.
    for step in &plan1.plan.steps {
        crate::ai::validate_against(step, &schema).unwrap();
    }
    // Simulate the shadow using the public Plan::validate on a
    // fresh schema (mutation path inside ai.rs is private; the public
    // surface is `Plan::validate` which runs the shadow internally).
    // For chaining across calls we mutate the schema ourselves:
    if let Primitive::RenameField(r) = &plan1.plan.steps[0] {
        let model = schema
            .models
            .iter_mut()
            .find(|m| m.name == r.model)
            .unwrap();
        model
            .fields
            .iter_mut()
            .find(|f| f.name == r.from)
            .unwrap()
            .name = r.to.clone();
    } else {
        panic!("expected RenameField");
    }

    let plan2 = generate_plan(
        &schema,
        None,
        PlanRequest::new("change name in tasks to String"),
    )
    .unwrap();
    plan2.plan.validate(&schema).unwrap();
    match &plan2.plan.steps[0] {
        Primitive::ChangeFieldType(c) => {
            assert_eq!(c.model, "Task");
            assert_eq!(c.field, "name");
            assert_eq!(c.new_type, "String");
        }
        _ => panic!("wrong primitive"),
    }
}

// ---------- misc robustness ----------

#[test]
fn empty_prompt_is_rejected() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("   ")).expect_err("should reject");
    assert!(matches!(err, PlanError::EmptyPrompt));
}

#[test]
fn unknown_intent_returns_supported_forms_list() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("please do something nice"))
        .expect_err("should reject");
    match err {
        PlanError::InvalidIntent(msg) => {
            for needle in ["add ", "rename ", "remove ", "change ", "make "] {
                assert!(
                    msg.contains(needle),
                    "supported-forms message should list `{needle}` — got: {msg}",
                );
            }
        }
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn remove_field_refuses_on_missing_field() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("remove nope from tasks"))
        .expect_err("should reject");
    assert!(matches!(err, PlanError::FieldDoesNotExist { .. }));
}

#[test]
fn remove_model_is_developer_only_not_a_plan() {
    let schema = task_schema();
    let err = generate_plan(&schema, None, PlanRequest::new("remove model tasks"))
        .expect_err("should reject");
    assert!(matches!(err, PlanError::DeveloperOnlyRequested(_)));
}

#[test]
fn core_models_are_protected() {
    let schema = schema_with_core_user();
    let err = generate_plan(&schema, None, PlanRequest::new("add nickname to users"))
        .expect_err("should reject");
    assert!(
        matches!(err, PlanError::CoreModelProtected(ref n) if n == "User"),
        "wrong error: {err:?}",
    );
}

#[test]
fn rename_model_emits_schema_level_rename() {
    let schema = task_schema();
    let res = generate_plan(&schema, None, PlanRequest::new("rename model Task to Todo")).unwrap();
    match &res.plan.steps[0] {
        Primitive::RenameModel(r) => {
            assert_eq!(r.from, "Task");
            assert_eq!(r.to, "Todo");
        }
        _ => panic!("wrong primitive"),
    }
    res.plan.validate(&schema).unwrap();
}

#[test]
fn change_field_type_accepts_synonyms() {
    let schema = task_schema();
    for (prompt, expected) in [
        ("change title in tasks to text", "String"),
        ("change title in tasks to datetime", "DateTime"),
        ("change title in tasks to int", "i32"),
        ("change title in tasks to bigint", "i64"),
        ("change title in tasks to boolean", "bool"),
    ] {
        let res = generate_plan(&schema, None, PlanRequest::new(prompt))
            .unwrap_or_else(|e| panic!("prompt `{prompt}` failed: {e}"));
        match &res.plan.steps[0] {
            Primitive::ChangeFieldType(c) => assert_eq!(c.new_type, expected, "prompt `{prompt}`"),
            _ => panic!("wrong primitive for `{prompt}`"),
        }
    }
}

#[test]
fn change_field_type_unknown_type_is_rejected() {
    let schema = task_schema();
    let err = generate_plan(
        &schema,
        None,
        PlanRequest::new("change title in tasks to foo"),
    )
    .expect_err("should reject");
    assert!(matches!(err, PlanError::UnknownType(_)));
}

#[test]
fn make_optional_emits_change_nullability() {
    let schema = task_schema();
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("make title in tasks optional"),
    )
    .unwrap();
    match &res.plan.steps[0] {
        Primitive::ChangeFieldNullability(c) => {
            assert_eq!(c.model, "Task");
            assert_eq!(c.field, "title");
            assert!(c.nullable);
        }
        _ => panic!("wrong primitive"),
    }
}

// ---------- planner is deterministic ----------

#[test]
fn same_prompt_produces_byte_for_byte_same_plan_json() {
    let schema = task_schema();
    let req = || PlanRequest::new("add priority to tasks");
    let a = generate_plan(&schema, None, req()).unwrap();
    let b = generate_plan(&schema, None, req()).unwrap();
    assert_eq!(
        render_plan_json(&a.plan, &a.explanation),
        render_plan_json(&b.plan, &b.explanation),
    );
}

// ---------- relation kind is unused in 0.5.0 planner ----------

#[test]
fn relation_kind_symbol_is_linked_for_future_use() {
    // Prevents a dead-import warning in release mode; the grammar
    // doesn't emit relations in 0.5.0 but the symbol is wired up so
    // future parsers can see it.
    let _ = RelationKind::HasMany;
}

// ---------- renderers ----------

#[test]
fn render_plan_json_matches_documented_shape() {
    let schema = task_schema();
    let res = generate_plan(&schema, None, PlanRequest::new("add priority to tasks")).unwrap();
    let json = render_plan_json(&res.plan, &res.explanation);
    // Spot-check the documented keys at a string level.
    assert!(
        json.contains(r#""op": "AddField""#),
        "missing op tag: {json}"
    );
    assert!(
        json.contains(r#""field": "priority""#),
        "missing field key: {json}",
    );
    assert!(
        json.contains(r#""type": "i32""#),
        "missing type key: {json}"
    );
    assert!(
        json.contains(r#""nullable": false"#),
        "missing nullable: {json}"
    );
    assert!(
        json.contains(r#""explanation""#),
        "missing explanation: {json}",
    );
}

#[test]
fn render_plan_human_reads_like_a_changelog() {
    let schema = task_schema();
    let res = generate_plan(
        &schema,
        None,
        PlanRequest::new("rename title to name in tasks"),
    )
    .unwrap();
    let text = render_plan_human(&res.plan, &res.explanation);
    assert!(
        text.starts_with("Plan:\n"),
        "should start with Plan:\n\n{text}"
    );
    assert!(
        text.contains("Rename field \"Task.title\" to \"name\""),
        "human summary shape: {text}",
    );
    assert!(text.contains("Explanation:"), "missing Explanation block");
}
