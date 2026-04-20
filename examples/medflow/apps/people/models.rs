use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

// ───────────────────────────────────────────────────────────────
// Department
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Department {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub is_active: bool,
    #[rustio(belongs_to = "Doctor", display = "full_name")]
    pub head_doctor_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

impl Model for Department {
    const TABLE: &'static str = "departments";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "code",
        "is_active",
        "head_doctor_id",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "name",
        "code",
        "is_active",
        "head_doctor_id",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            code: row.get_string("code")?,
            is_active: row.get_bool("is_active")?,
            head_doctor_id: row.get_optional_i64("head_doctor_id")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.code.clone().into(),
            self.is_active.into(),
            self.head_doctor_id.into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Doctor
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Doctor {
    pub id: i64,
    pub full_name: String,
    pub specialty: String,
    #[rustio(belongs_to = "Department", display = "name")]
    pub department_id: i64,
    pub license_no: String,
    pub email: String,
    pub phone: String,
    pub years_experience: i32,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Doctor {
    const TABLE: &'static str = "doctors";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "full_name",
        "specialty",
        "department_id",
        "license_no",
        "email",
        "phone",
        "years_experience",
        "is_active",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "full_name",
        "specialty",
        "department_id",
        "license_no",
        "email",
        "phone",
        "years_experience",
        "is_active",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            full_name: row.get_string("full_name")?,
            specialty: row.get_string("specialty")?,
            department_id: row.get_i64("department_id")?,
            license_no: row.get_string("license_no")?,
            email: row.get_string("email")?,
            phone: row.get_string("phone")?,
            years_experience: row.get_i32("years_experience")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.full_name.clone().into(),
            self.specialty.clone().into(),
            self.department_id.into(),
            self.license_no.clone().into(),
            self.email.clone().into(),
            self.phone.clone().into(),
            self.years_experience.into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Patient
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Patient {
    pub id: i64,
    pub full_name: String,
    pub date_of_birth: DateTime<Utc>,
    pub gender: String,
    pub national_id: String,
    pub phone: String,
    pub email: String,
    pub blood_type: String,
    pub allergies: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Patient {
    const TABLE: &'static str = "patients";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "full_name",
        "date_of_birth",
        "gender",
        "national_id",
        "phone",
        "email",
        "blood_type",
        "allergies",
        "is_active",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "full_name",
        "date_of_birth",
        "gender",
        "national_id",
        "phone",
        "email",
        "blood_type",
        "allergies",
        "is_active",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            full_name: row.get_string("full_name")?,
            date_of_birth: row.get_datetime("date_of_birth")?,
            gender: row.get_string("gender")?,
            national_id: row.get_string("national_id")?,
            phone: row.get_string("phone")?,
            email: row.get_string("email")?,
            blood_type: row.get_string("blood_type")?,
            allergies: row.get_string("allergies")?,
            is_active: row.get_bool("is_active")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.full_name.clone().into(),
            self.date_of_birth.into(),
            self.gender.clone().into(),
            self.national_id.clone().into(),
            self.phone.clone().into(),
            self.email.clone().into(),
            self.blood_type.clone().into(),
            self.allergies.clone().into(),
            self.is_active.into(),
            self.created_at.into(),
        ]
    }
}
