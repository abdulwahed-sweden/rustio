use rustio_core::admin::Admin;

use super::models::{Invoice, Payment};

pub fn install(admin: Admin) -> Admin {
    admin.model::<Invoice>().model::<Payment>()
}
