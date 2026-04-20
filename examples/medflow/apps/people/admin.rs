use rustio_core::admin::Admin;

use super::models::{Department, Doctor, Patient};

pub fn install(admin: Admin) -> Admin {
    admin
        .model::<Department>()
        .model::<Doctor>()
        .model::<Patient>()
}
