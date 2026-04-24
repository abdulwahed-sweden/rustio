//! Placeholder for the entry_builder module — full port deferred.
//!
//! OLD's `entry_builder` derives runtime [`DynamicAdminEntry`] values
//! from a live [`Schema`](crate::schema::Schema), so the admin dashboard
//! can list rows for models that don't have a compile-time `AdminModel`
//! impl. NEW hasn't landed a caller for that path yet, so this module
//! is stubbed: the types keep OLD's field shape so existing call sites
//! (stubbed functions in `suggestions.rs`, ignored fixtures in
//! `suggestions_tests.rs`) type-check, and `build_admin_entries`
//! returns an empty `Vec`.
//!
//! Unblocking: port OLD's full `entry_builder` body (the real
//! `DynamicAdminEntry::from_schema_model` constructor + the
//! `build_admin_entries` iterator). At that point the `*_from_entries`
//! stubs in `suggestions.rs` can be un-stubbed, the five
//! `#[ignore]`d tests in `suggestions_tests.rs` can be re-enabled,
//! and the `#[allow(dead_code)]` on this module can be dropped.

#![allow(dead_code)]

use crate::admin::FieldType;
use crate::schema::Schema;

#[derive(Debug, Clone)]
pub struct DynamicAdminEntry {
    pub admin_name: String,
    pub display_name: String,
    pub singular_name: String,
    pub table: String,
    pub fields: Vec<DynamicAdminField>,
    pub core: bool,
}

#[derive(Debug, Clone)]
pub struct DynamicAdminField {
    pub name: String,
    pub ty: FieldType,
    pub editable: bool,
    pub nullable: bool,
}

pub fn build_admin_entries(_schema: &Schema) -> Vec<DynamicAdminEntry> {
    Vec::new()
}
