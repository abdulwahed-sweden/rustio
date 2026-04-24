//! The auto-generated admin UI.

mod builtin;
mod handlers;
mod intelligence;
mod relations;
mod render;
mod routes;
mod types;

#[cfg(test)]
mod admin_intelligence_tests;
#[cfg(test)]
mod relations_tests;

pub use intelligence::{
    classify_field, classify_search, classify_search_for_field, context_global,
    field_ui_metadata, field_ui_metadata_with_relation, format_relation_cell, infer_filters,
    infer_filters_with_relations, mask_pii, FieldRole, FieldUI, FilterDef, FilterKind,
    SearchIntent,
};
pub use relations::{
    InverseRelation, RegistryError, RelationRegistry, ResolvedRelation,
    RELATION_FILTER_DROPDOWN_CAP,
};
pub use routes::register_admin_routes;
pub use types::{Admin, AdminEntry, AdminField, AdminModel, AdminRelation, FieldType};
