use rustio_core::admin::Admin;

use super::models::{Appointment, Prescription};

pub fn install(admin: Admin) -> Admin {
    admin.model::<Appointment>().model::<Prescription>()
}
