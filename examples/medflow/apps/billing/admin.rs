use rustio_core::admin::Admin;

use super::models::Invoice;

pub fn install(admin: Admin) -> Admin {
    admin.model::<Invoice>()
}
