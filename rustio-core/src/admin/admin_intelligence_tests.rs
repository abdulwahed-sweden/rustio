//! Tests for the admin intelligence layer.
//!
//! Every test exercises one of the five public helpers:
//! [`classify_field`], [`field_ui_metadata`], [`infer_filters`],
//! [`classify_search`], [`mask_pii`]. The fixtures are minimal by
//! design — the whole point of the module is that decisions come
//! from `(field, context)` with no per-project configuration.

use super::intelligence::{
    classify_field, classify_search, classify_search_for_field, field_ui_metadata,
    field_ui_metadata_with_relation, format_relation_cell, infer_filters,
    infer_filters_with_relations, mask_pii, FieldRole, FilterKind, SearchIntent,
};
use super::{AdminField, FieldType};
use crate::ai::ContextConfig;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn text(name: &'static str) -> AdminField {
    AdminField {
        name,
        ty: FieldType::String,
        editable: true,
        nullable: false,
    }
}

fn bigint(name: &'static str, editable: bool) -> AdminField {
    AdminField {
        name,
        ty: FieldType::I64,
        editable,
        nullable: false,
    }
}

fn boolean(name: &'static str) -> AdminField {
    AdminField {
        name,
        ty: FieldType::Bool,
        editable: true,
        nullable: false,
    }
}

fn datetime(name: &'static str) -> AdminField {
    AdminField {
        name,
        ty: FieldType::DateTime,
        editable: true,
        nullable: false,
    }
}

fn se_context() -> ContextConfig {
    ContextConfig {
        country: Some("SE".into()),
        industry: Some("housing".into()),
        ..Default::default()
    }
}

fn healthcare_context() -> ContextConfig {
    ContextConfig {
        industry: Some("healthcare".into()),
        ..Default::default()
    }
}

fn banking_context() -> ContextConfig {
    ContextConfig {
        industry: Some("banking".into()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// classify_field
// ---------------------------------------------------------------------------

#[test]
fn id_is_classified_without_context() {
    assert_eq!(classify_field(&bigint("id", false), None), FieldRole::Id);
}

#[test]
fn datetime_is_timestamp_regardless_of_context() {
    assert_eq!(
        classify_field(&datetime("created_at"), None),
        FieldRole::Timestamp
    );
}

#[test]
fn status_string_is_classified_as_status() {
    assert_eq!(classify_field(&text("status"), None), FieldRole::Status);
    // The `_status` suffix also triggers the role.
    assert_eq!(
        classify_field(&text("payment_status"), None),
        FieldRole::Status
    );
}

#[test]
fn foreign_key_columns_end_in_underscore_id() {
    assert_eq!(
        classify_field(&bigint("user_id", true), None),
        FieldRole::ForeignKey
    );
    assert_eq!(
        classify_field(&bigint("listing_id", true), None),
        FieldRole::ForeignKey
    );
}

#[test]
fn booleans_are_booleans() {
    assert_eq!(classify_field(&boolean("is_active"), None), FieldRole::Bool);
}

#[test]
fn plain_text_is_the_default() {
    assert_eq!(
        classify_field(&text("nickname"), None),
        FieldRole::PlainText
    );
}

// ---------------------------------------------------------------------------
// classify_field — context-aware
// ---------------------------------------------------------------------------

#[test]
fn personnummer_is_classified_under_se() {
    let ctx = se_context();
    assert_eq!(
        classify_field(&text("personnummer"), Some(&ctx)),
        FieldRole::Personnummer,
    );
    // Aliases resolve to the same role so the planner/UI agree.
    assert_eq!(
        classify_field(&text("personal_id"), Some(&ctx)),
        FieldRole::Personnummer,
    );
}

#[test]
fn personnummer_is_not_classified_without_country_context() {
    // Without a country flag the layer doesn't assume PII — a project
    // might use the word `personnummer` for a decorative column.
    // (The shape-only path still falls through to PlainText.)
    assert_eq!(
        classify_field(&text("personnummer"), None),
        FieldRole::PlainText,
    );
}

#[test]
fn email_is_email_under_gdpr_context() {
    let ctx = se_context(); // EU → GDPR
    assert_eq!(classify_field(&text("email"), Some(&ctx)), FieldRole::Email);
    // Without GDPR, shape-only: still Email because the field NAME is email.
    assert_eq!(classify_field(&text("email"), None), FieldRole::Email);
}

#[test]
fn patient_id_under_healthcare_is_opaque_identifier() {
    let ctx = healthcare_context();
    assert_eq!(
        classify_field(&text("patient_id"), Some(&ctx)),
        FieldRole::OpaqueIdentifier,
    );
    // Without industry, it's just a ForeignKey (shape suffix `_id`).
    assert_eq!(
        classify_field(&text("patient_id"), None),
        FieldRole::ForeignKey,
    );
}

#[test]
fn balance_under_banking_is_money() {
    let ctx = banking_context();
    assert_eq!(
        classify_field(&bigint("balance", true), Some(&ctx)),
        FieldRole::Money,
    );
    assert_eq!(
        classify_field(&bigint("tx_amount", true), Some(&ctx)),
        FieldRole::Money,
    );
}

#[test]
fn sensitive_roles_flag_themselves() {
    for role in [
        FieldRole::Personnummer,
        FieldRole::Email,
        FieldRole::Phone,
        FieldRole::OpaqueIdentifier,
    ] {
        assert!(role.is_sensitive(), "{role:?} should be sensitive");
    }
    for role in [
        FieldRole::Id,
        FieldRole::Timestamp,
        FieldRole::Bool,
        FieldRole::NumericCount,
        FieldRole::ForeignKey,
        FieldRole::Status,
        FieldRole::Money,
        FieldRole::PlainText,
    ] {
        assert!(!role.is_sensitive(), "{role:?} should not be sensitive");
    }
}

// ---------------------------------------------------------------------------
// field_ui_metadata
// ---------------------------------------------------------------------------

#[test]
fn personnummer_ui_carries_placeholder_and_sensitivity() {
    let ctx = se_context();
    let ui = field_ui_metadata(&text("personnummer"), Some(&ctx));
    assert_eq!(ui.role, FieldRole::Personnummer);
    assert_eq!(ui.label, "Personnummer");
    assert_eq!(ui.placeholder.as_deref(), Some("YYYYMMDD-XXXX"));
    assert!(ui.hint.is_some());
    assert!(ui.sensitive);
    assert!(ui.sensitivity_note.is_some());
}

#[test]
fn patient_id_ui_carries_opaque_hint_and_sensitivity() {
    let ctx = healthcare_context();
    let ui = field_ui_metadata(&text("patient_id"), Some(&ctx));
    assert_eq!(ui.role, FieldRole::OpaqueIdentifier);
    assert!(ui.hint.as_deref().unwrap().contains("Opaque"));
    assert!(ui.sensitive);
}

#[test]
fn money_ui_documents_minor_units() {
    let ctx = banking_context();
    let ui = field_ui_metadata(&bigint("balance", true), Some(&ctx));
    assert_eq!(ui.role, FieldRole::Money);
    assert!(ui.hint.as_deref().unwrap().contains("minor units"));
    // Money itself isn't sensitive by default; GDPR doesn't reach balances.
    assert!(!ui.sensitive);
}

#[test]
fn email_ui_is_sensitive_only_under_gdpr() {
    let gdpr = se_context();
    let no_ctx: Option<&ContextConfig> = None;
    let with = field_ui_metadata(&text("email"), Some(&gdpr));
    let without = field_ui_metadata(&text("email"), no_ctx);
    assert!(with.sensitive);
    assert!(!without.sensitive);
    // Either way the placeholder shows up.
    assert_eq!(with.placeholder.as_deref(), Some("name@example.com"));
    assert_eq!(without.placeholder.as_deref(), Some("name@example.com"));
}

#[test]
fn datetime_ui_documents_utc() {
    let ui = field_ui_metadata(&datetime("created_at"), None);
    assert_eq!(ui.role, FieldRole::Timestamp);
    assert_eq!(ui.placeholder.as_deref(), Some("YYYY-MM-DDTHH:MM"));
    assert!(ui.hint.as_deref().unwrap().contains("UTC"));
}

#[test]
fn plain_field_has_no_extra_annotations() {
    let ui = field_ui_metadata(&text("nickname"), None);
    assert_eq!(ui.role, FieldRole::PlainText);
    assert_eq!(ui.label, "Nickname");
    assert!(ui.placeholder.is_none());
    assert!(ui.hint.is_none());
    assert!(!ui.sensitive);
}

// ---------------------------------------------------------------------------
// infer_filters
// ---------------------------------------------------------------------------

#[test]
fn filters_include_status_bool_and_datetime() {
    let fields = vec![
        bigint("id", false),
        text("title"),
        text("status"),
        boolean("is_active"),
        datetime("created_at"),
    ];
    let filters = infer_filters(&fields, None);
    let kinds: Vec<(&str, FilterKind)> = filters
        .iter()
        .map(|f| (f.field.as_str(), f.kind.clone()))
        .collect();
    assert!(
        kinds
            .iter()
            .any(|(n, k)| *n == "status" && *k == FilterKind::DropdownText),
        "status should dropdown: {kinds:?}",
    );
    assert!(
        kinds
            .iter()
            .any(|(n, k)| *n == "is_active" && *k == FilterKind::BoolYesNo),
        "bool should yes/no: {kinds:?}",
    );
    assert!(
        kinds
            .iter()
            .any(|(n, k)| *n == "created_at" && *k == FilterKind::DateRange),
        "datetime should date-range: {kinds:?}",
    );
    // id is never a filter — removal is explicit in the inferrer.
    assert!(!filters.iter().any(|f| f.field == "id"));
    // `title` (plain text) has no stock filter — live search covers it.
    assert!(!filters.iter().any(|f| f.field == "title"));
}

#[test]
fn personnummer_filter_is_exact_match_under_se() {
    let ctx = se_context();
    let fields = vec![bigint("id", false), text("personnummer"), text("name")];
    let filters = infer_filters(&fields, Some(&ctx));
    assert!(filters
        .iter()
        .any(|f| f.field == "personnummer" && f.kind == FilterKind::ExactMatch));
}

#[test]
fn foreign_key_filter_is_numeric_exact() {
    let fields = vec![bigint("id", false), bigint("listing_id", true)];
    let filters = infer_filters(&fields, None);
    assert!(filters
        .iter()
        .any(|f| f.field == "listing_id" && f.kind == FilterKind::NumericExact));
}

#[test]
fn filters_preserve_field_order() {
    let fields = vec![text("status"), boolean("is_active"), datetime("created_at")];
    let names: Vec<_> = infer_filters(&fields, None)
        .iter()
        .map(|f| f.field.clone())
        .collect();
    assert_eq!(names, vec!["status", "is_active", "created_at"]);
}

// ---------------------------------------------------------------------------
// classify_search
// ---------------------------------------------------------------------------

#[test]
fn numeric_query_classifies_as_id() {
    let s = classify_search("42");
    assert_eq!(s, SearchIntent::NumericId(42));
    assert_eq!(s.label(), "ID");
}

#[test]
fn email_query_classifies_as_email() {
    let s = classify_search("alice@example.com");
    match s {
        SearchIntent::Email(q) => assert_eq!(q, "alice@example.com"),
        other => panic!("wrong intent: {other:?}"),
    }
}

#[test]
fn personnummer_query_classifies_correctly() {
    for q in ["19870512-4521", "198705124521"] {
        let s = classify_search(q);
        match s {
            SearchIntent::Personnummer(v) => assert_eq!(v, q),
            other => panic!("`{q}` should be Personnummer, got {other:?}"),
        }
    }
    // 11 digits is not a personnummer — falls through to text.
    match classify_search("12345678901") {
        SearchIntent::Text(_) | SearchIntent::NumericId(_) => {}
        other => panic!("11 digits should not be personnummer: {other:?}"),
    }
}

#[test]
fn text_query_is_the_fallback() {
    let s = classify_search("södermalm");
    match s {
        SearchIntent::Text(q) => assert_eq!(q, "södermalm"),
        other => panic!("wrong intent: {other:?}"),
    }
}

#[test]
fn whitespace_and_empty_become_empty_text() {
    match classify_search("   ") {
        SearchIntent::Text(q) => assert!(q.is_empty()),
        other => panic!("wrong intent: {other:?}"),
    }
}

#[test]
fn negative_numbers_are_not_id_searches() {
    match classify_search("-5") {
        SearchIntent::Text(_) => {}
        other => panic!("negative input should be Text, got {other:?}"),
    }
}

#[test]
fn email_detection_rejects_obvious_non_emails() {
    // No TLD dot.
    assert!(matches!(classify_search("x@y"), SearchIntent::Text(_)));
    // Leading/trailing @.
    assert!(matches!(
        classify_search("@example.com"),
        SearchIntent::Text(_)
    ));
}

// ---------------------------------------------------------------------------
// mask_pii
// ---------------------------------------------------------------------------

#[test]
fn mask_pii_keeps_first_chars_and_replaces_the_rest() {
    // 13 chars: keep floor(13/3)=4 → "1987" + 9 dots
    let out = mask_pii("19870512-4521");
    assert_eq!(out.chars().count(), 13);
    assert!(out.starts_with("1987"));
    assert!(out.ends_with("•••"));
}

#[test]
fn mask_pii_clamps_prefix_to_reasonable_bounds() {
    // Short input: keep at least 2.
    let short = mask_pii("ab");
    assert_eq!(short.chars().count(), 2);
    // The bound keeps enough of a short value to disambiguate.
    let five = mask_pii("abcde");
    assert!(five.starts_with("ab"));
}

#[test]
fn mask_pii_handles_empty_and_unicode() {
    assert_eq!(mask_pii(""), "");
    // Unicode preserves char count, not byte count.
    let s = "öl@öl.se";
    let masked = mask_pii(s);
    assert_eq!(masked.chars().count(), s.chars().count());
}

#[test]
fn mask_pii_is_deterministic() {
    let a = mask_pii("alice@example.com");
    let b = mask_pii("alice@example.com");
    assert_eq!(a, b);
}

// ---------------------------------------------------------------------------
// 0.8.0 — relations
// ---------------------------------------------------------------------------

#[test]
fn field_ui_metadata_without_relation_target_has_no_label() {
    let ui = field_ui_metadata(&bigint("applicant_id", true), None);
    assert_eq!(ui.role, FieldRole::ForeignKey);
    assert_eq!(
        ui.relation_label, None,
        "no relation target passed — no label invented from the column name",
    );
}

#[test]
fn field_ui_metadata_with_relation_carries_label_and_hint() {
    let ui =
        field_ui_metadata_with_relation(&bigint("applicant_id", true), None, Some("Applicant"));
    assert_eq!(ui.role, FieldRole::ForeignKey);
    assert_eq!(ui.relation_label.as_deref(), Some("Applicant"));
    assert_eq!(
        ui.hint.as_deref(),
        Some("Foreign key to Applicant."),
        "relation-aware hint should name the target",
    );
}

#[test]
fn field_ui_metadata_with_relation_escalates_non_id_column_names() {
    // The column is just `owner`, not `owner_id` — only the relation
    // lookup tells us it's a FK. The helper must still set the
    // ForeignKey role so downstream filter/search code routes it right.
    let ui = field_ui_metadata_with_relation(&bigint("owner", true), None, Some("User"));
    assert_eq!(ui.role, FieldRole::ForeignKey);
    assert_eq!(ui.relation_label.as_deref(), Some("User"));
}

#[test]
fn format_relation_cell_renders_target_and_id() {
    assert_eq!(format_relation_cell(42, Some("Applicant")), "Applicant #42");
}

#[test]
fn format_relation_cell_falls_back_when_target_unknown() {
    assert_eq!(format_relation_cell(42, None), "42");
    assert_eq!(format_relation_cell(42, Some("")), "42");
}

#[test]
fn infer_filters_with_relations_emits_relation_select() {
    let fields = [bigint("id", false), bigint("applicant_id", true)];
    let filters = infer_filters_with_relations(&fields, None, |f| match f.name {
        "applicant_id" => Some("Applicant".to_string()),
        _ => None,
    });
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].field, "applicant_id");
    assert_eq!(
        filters[0].kind,
        FilterKind::RelationSelect {
            target_model: "Applicant".into()
        },
    );
}

#[test]
fn infer_filters_without_relation_lookup_keeps_numeric_exact() {
    // Existing callers that use the plain `infer_filters` must still
    // get the pre-0.8 behaviour for FK columns.
    let fields = [bigint("id", false), bigint("applicant_id", true)];
    let filters = infer_filters(&fields, None);
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].kind, FilterKind::NumericExact);
}

#[test]
fn classify_search_for_field_emits_relation_id_for_numeric() {
    let intent = classify_search_for_field("42", Some("Applicant"));
    assert_eq!(
        intent,
        SearchIntent::RelationId {
            model: "Applicant".into(),
            id: 42
        },
    );
    assert_eq!(intent.label(), "relation");
}

#[test]
fn classify_search_for_field_without_target_falls_back() {
    // Without a relation target, the helper behaves like the plain
    // `classify_search` — a lone integer is a NumericId lookup, not
    // a RelationId.
    let intent = classify_search_for_field("42", None);
    assert_eq!(intent, SearchIntent::NumericId(42));
}

#[test]
fn classify_search_for_field_rejects_non_integer() {
    // A FK field can still be searched by text (e.g. the user typed
    // a name). The helper falls back to `classify_search`.
    let intent = classify_search_for_field("alice", Some("Applicant"));
    assert_eq!(intent, SearchIntent::Text("alice".into()));
}
