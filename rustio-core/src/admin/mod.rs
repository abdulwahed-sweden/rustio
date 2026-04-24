//! The auto-generated admin UI.

mod builtin;
mod handlers;
mod relations;
mod render;
mod routes;
mod types;

#[cfg(test)]
mod relations_tests;

pub use relations::{
    InverseRelation, RegistryError, RelationRegistry, ResolvedRelation,
    RELATION_FILTER_DROPDOWN_CAP,
};
pub use routes::register_admin_routes;
pub use types::{Admin, AdminEntry, AdminField, AdminModel, AdminRelation, FieldType};
