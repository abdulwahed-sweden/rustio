use rustio_core::admin::Admin;

use super::models::{
    Appointment, AppointmentEvent, Diagnosis, MedicalRecord, Prescription, VitalSigns,
};

pub fn install(admin: Admin) -> Admin {
    admin
        .model::<Appointment>()
        .model::<AppointmentEvent>()
        .model::<MedicalRecord>()
        .model::<Diagnosis>()
        .model::<VitalSigns>()
        .model::<Prescription>()
}
