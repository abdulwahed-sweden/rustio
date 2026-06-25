use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::customers::models::Customer;

/// An Invoice is billing issued to a Customer for their bookings.
///
/// `amount_cents` is integer minor units (cents). `status` is a string
/// enum: `draft` / `sent` / `paid` / `overdue`.
#[derive(Debug, RustioAdmin)]
pub struct Invoice {
    pub id: i64,
    pub invoice_number: String,
    #[rustio(belongs_to = "Customer", display = "name")]
    pub customer_id: i64,
    /// Integer minor units (cents). Never use floats for money.
    pub amount_cents: i64,
    /// enum: "draft" | "sent" | "paid" | "overdue"
    pub status: String,
    pub issued_at: DateTime<Utc>,
}

impl Model for Invoice {
    const TABLE: &'static str = "invoices";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "invoice_number",
        "customer_id",
        "amount_cents",
        "status",
        "issued_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "invoice_number",
        "customer_id",
        "amount_cents",
        "status",
        "issued_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            invoice_number: row.get_string("invoice_number")?,
            customer_id: row.get_i64("customer_id")?,
            amount_cents: row.get_i64("amount_cents")?,
            status: row.get_string("status")?,
            issued_at: row.get_datetime("issued_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.invoice_number.clone().into(),
            self.customer_id.into(),
            self.amount_cents.into(),
            self.status.clone().into(),
            self.issued_at.into(),
        ]
    }
}
