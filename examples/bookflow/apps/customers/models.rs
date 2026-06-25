use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A Customer is the party making a booking. Generic on purpose: a
/// shipper, a tenant, a patient, a club member — anyone who reserves a
/// resource.
///
/// `customer_type` is a small string enum: `business` / `individual`.
/// The admin renders it as free text; swap in a real widget once you
/// have a fixed vocabulary.
#[derive(Debug, RustioAdmin)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    /// enum: "business" | "individual"
    pub customer_type: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Customer {
    const TABLE: &'static str = "customers";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "customer_type",
        "email",
        "phone",
        "address",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "name",
        "customer_type",
        "email",
        "phone",
        "address",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            customer_type: row.get_string("customer_type")?,
            email: row.get_string("email")?,
            phone: row.get_string("phone")?,
            address: row.get_string("address")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.customer_type.clone().into(),
            self.email.clone().into(),
            self.phone.clone().into(),
            self.address.clone().into(),
            self.created_at.into(),
        ]
    }
}
