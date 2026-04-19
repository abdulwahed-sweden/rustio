use rustio_core::admin::Admin;

use super::models::Post;

/// Contribute this app's models to the shared admin index.
pub fn install(admin: Admin) -> Admin {
    admin.model::<Post>()
}
