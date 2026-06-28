use rustio_core::admin::Admin;

use super::models::Staff;

/// Contribute this app's model to the shared admin index.
pub fn install(admin: Admin) -> Admin {
    admin.model::<Staff>()
}
