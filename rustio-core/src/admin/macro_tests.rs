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

// Phase 7.6 — fixture exercising every numeric / datetime arm of
// `from_form` so the hardening tests below can hit each path. None of
// these fields are FK-shaped (no `#[relation]`), but at the form-parse
// layer that distinction doesn't exist — the rejection path is the
// same whether the i64 is meant to be a count, a year, or a foreign
// key id. `id` and the trailing timestamps round out the shape so the
// derive emits.
#[derive(Debug, RustioAdmin)]
#[allow(dead_code)]
pub struct HardeningFixture {
    pub id: i64,
    pub title: String,
    pub author_id: i64,
    pub edition: Option<i64>,
    pub published_at: DateTime<Utc>,
}

/// Phase 7.6 — submitting `"abc"` to a required i64 field must
/// surface a validation error, not silently coerce or panic. Pre-7.6
/// already errored on the i64 path; this test pins it as a
/// regression guard alongside the new optional path below.
#[test]
fn invalid_number_input() {
    let form = FormData::from_urlencoded(
        "title=hi&author_id=abc&edition=&published_at=2026-01-01T12:00",
    );
    let errs = HardeningFixture::from_form(&form)
        .expect_err("from_form must reject invalid i64");
    assert!(
        errs.iter().any(|e| e.contains("Author Id") && e.contains("number")),
        "expected an Author Id number error; got: {errs:?}"
    );
}

/// Phase 7.6 — the headline fix: an *Optional* i64 with garbage input
/// used to silently become None. It must now surface the same
/// validation error as the required i64 path. Empty input still
/// resolves to None (legitimate); only non-empty unparseable input
/// errors.
#[test]
fn invalid_optional_number_input() {
    // Garbage input → error.
    let form = FormData::from_urlencoded(
        "title=hi&author_id=1&edition=abc&published_at=2026-01-01T12:00",
    );
    let errs = HardeningFixture::from_form(&form)
        .expect_err("from_form must reject garbage in Option<i64>");
    assert!(
        errs.iter().any(|e| e.contains("Edition") && e.contains("number")),
        "expected an Edition number error; got: {errs:?}"
    );

    // Empty input → still None, no error (legitimate omission).
    let form2 = FormData::from_urlencoded(
        "title=hi&author_id=1&edition=&published_at=2026-01-01T12:00",
    );
    let model = HardeningFixture::from_form(&form2)
        .expect("empty Option<i64> input should NOT error");
    assert_eq!(model.edition, None);

    // Missing field entirely → also None.
    let form3 = FormData::from_urlencoded("title=hi&author_id=1&published_at=2026-01-01T12:00");
    let model = HardeningFixture::from_form(&form3)
        .expect("missing Option<i64> field should NOT error");
    assert_eq!(model.edition, None);
}

/// Phase 7.6 — malformed datetime → "is not a valid date." validation
/// error, not a panic. Pre-7.6 already handled this; pins the
/// behaviour against future edits to the macro.
#[test]
fn invalid_datetime_input() {
    let form = FormData::from_urlencoded(
        "title=hi&author_id=1&edition=&published_at=tomorrow",
    );
    let errs = HardeningFixture::from_form(&form)
        .expect_err("from_form must reject malformed datetime");
    assert!(
        errs.iter().any(|e| e.contains("Published At") && e.contains("not a valid date")),
        "expected a Published At date error; got: {errs:?}"
    );
}

/// Phase 7.6 — string fields trim leading/trailing whitespace AND
/// treat trimmed-empty as missing. A `"   "` submission to a
/// required String must error like an empty string would.
#[test]
fn whitespace_only_string_treated_as_empty() {
    let form = FormData::from_urlencoded(
        "title=%20%20%20&author_id=1&edition=&published_at=2026-01-01T12:00",
    );
    let errs = HardeningFixture::from_form(&form)
        .expect_err("whitespace-only required String must trigger required error");
    assert!(
        errs.iter().any(|e| e.contains("Title") && e.contains("required")),
        "expected a Title required error; got: {errs:?}"
    );

    // And the happy path: padded valid input is trimmed before save.
    let form = FormData::from_urlencoded(
        "title=%20%20hi%20%20&author_id=1&edition=&published_at=2026-01-01T12:00",
    );
    let model = HardeningFixture::from_form(&form).expect("trimmed String parses");
    assert_eq!(model.title, "hi", "leading/trailing whitespace must be stripped");
}
