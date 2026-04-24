//! The auto-generated admin UI.

mod builtin;
mod handlers;
mod render;
mod routes;
mod types;

pub use routes::register_admin_routes;
pub use types::{Admin, AdminEntry, AdminField, AdminModel, AdminRelation, FieldType};
