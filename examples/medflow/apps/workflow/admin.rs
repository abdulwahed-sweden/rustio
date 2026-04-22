use rustio_core::admin::Admin;

use super::models::{CheckIn, Room, Staff};

pub fn install(admin: Admin) -> Admin {
    admin.model::<Staff>().model::<Room>().model::<CheckIn>()
}
