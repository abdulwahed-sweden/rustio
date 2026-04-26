//! Phase 1/a — `#[derive(RustioAdmin)]` macro behaviour around
//! framework-managed timestamps.
//!
//! The macro promotes `created_at` and `updated_at` (when typed
//! `DateTime<Utc>`) to `FieldKind::DateTimeAuto`, which makes the field
//! non-editable (hidden from forms) and auto-fills it with
//! `Utc::now()` inside the generated `from_form`. Tested here rather
//! than inside `rustio-macros` because proc macros can't expand within
//! their own crate.

use chrono::{DateTime, Utc};

use crate::admin::AdminModel;
use crate::http::FormData;
use rustio_macros::RustioAdmin;

/// Fixture mirroring the shape of `examples/blog::Post` but with both
/// timestamp conventions present so a single struct exercises both
/// promotions.
#[derive(Debug, RustioAdmin)]
#[allow(dead_code)]
pub struct StampedFixture {
    pub id: i64,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[test]
fn auto_timestamp_fields_are_not_editable() {
    let fields = StampedFixture::FIELDS;

    let created = fields
        .iter()
        .find(|f| f.name == "created_at")
        .expect("created_at field present in FIELDS");
    let updated = fields
        .iter()
        .find(|f| f.name == "updated_at")
        .expect("updated_at field present in FIELDS");

    assert!(!created.editable, "created_at must be non-editable so the form filters it out");
    assert!(!updated.editable, "updated_at must be non-editable so the form filters it out");

    // The plain timestamp without a recognised name is left alone — sanity
    // check by confirming a writable field stays writable.
    let title = fields
        .iter()
        .find(|f| f.name == "title")
        .expect("title field present");
    assert!(title.editable, "regular fields must remain editable");
}

#[test]
fn from_form_accepts_submission_without_auto_timestamps() {
    // The browser POST never carries `created_at` / `updated_at`. The
    // macro must default both to `Utc::now()` rather than failing
    // validation as a missing required field.
    let before = Utc::now();
    let form = FormData::from_urlencoded("title=hello");
    let model = StampedFixture::from_form(&form).expect("from_form succeeds with no timestamps");
    let after = Utc::now();

    assert_eq!(model.title, "hello");
    assert!(
        model.created_at >= before && model.created_at <= after,
        "created_at should be defaulted to Utc::now()",
    );
    assert!(
        model.updated_at >= before && model.updated_at <= after,
        "updated_at should be defaulted to Utc::now()",
    );
}
