use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

// Relation targets on other apps. Brought into scope so the
// `#[rustio(belongs_to = "...")]` compile-time checks can resolve
// `<Target as Model>::TABLE` and `COLUMNS`.
use crate::apps::care::models::Appointment;
use crate::apps::people::models::Patient;

// ───────────────────────────────────────────────────────────────
// Invoice
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Invoice {
    pub id: i64,
    pub invoice_number: String,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    #[rustio(belongs_to = "Appointment", display = "reason")]
    pub appointment_id: Option<i64>,
    pub amount_cents: i64,
    pub currency: String,
    pub status: String,
    pub issued_at: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Invoice {
    const TABLE: &'static str = "invoices";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "invoice_number",
        "patient_id",
        "appointment_id",
        "amount_cents",
        "currency",
        "status",
        "issued_at",
        "paid_at",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "invoice_number",
        "patient_id",
        "appointment_id",
        "amount_cents",
        "currency",
        "status",
        "issued_at",
        "paid_at",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            invoice_number: row.get_string("invoice_number")?,
            patient_id: row.get_i64("patient_id")?,
            appointment_id: row.get_optional_i64("appointment_id")?,
            amount_cents: row.get_i64("amount_cents")?,
            currency: row.get_string("currency")?,
            status: row.get_string("status")?,
            issued_at: row.get_datetime("issued_at")?,
            paid_at: row.get_optional_datetime("paid_at")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.invoice_number.clone().into(),
            self.patient_id.into(),
            self.appointment_id.into(),
            self.amount_cents.into(),
            self.currency.clone().into(),
            self.status.clone().into(),
            self.issued_at.into(),
            self.paid_at.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}

// ───────────────────────────────────────────────────────────────
// Payment — one row per payment received against an Invoice.
//
// An invoice may have many payments (partials, insurance + patient
// split, refunds tracked as negative amounts). Totals are computed
// at read time, not denormalised onto the invoice row.
// ───────────────────────────────────────────────────────────────

#[derive(Debug, RustioAdmin)]
pub struct Payment {
    pub id: i64,
    #[rustio(belongs_to = "Invoice", display = "invoice_number")]
    pub invoice_id: i64,
    #[rustio(belongs_to = "Patient", display = "full_name")]
    pub patient_id: i64,
    pub amount_cents: i64,
    pub currency: String,
    pub method: String,
    pub reference: String,
    pub received_at: DateTime<Utc>,
    pub notes: String,
    pub created_at: DateTime<Utc>,
}

impl Model for Payment {
    const TABLE: &'static str = "payments";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "invoice_id",
        "patient_id",
        "amount_cents",
        "currency",
        "method",
        "reference",
        "received_at",
        "notes",
        "created_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "invoice_id",
        "patient_id",
        "amount_cents",
        "currency",
        "method",
        "reference",
        "received_at",
        "notes",
        "created_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            invoice_id: row.get_i64("invoice_id")?,
            patient_id: row.get_i64("patient_id")?,
            amount_cents: row.get_i64("amount_cents")?,
            currency: row.get_string("currency")?,
            method: row.get_string("method")?,
            reference: row.get_string("reference")?,
            received_at: row.get_datetime("received_at")?,
            notes: row.get_string("notes")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.invoice_id.into(),
            self.patient_id.into(),
            self.amount_cents.into(),
            self.currency.clone().into(),
            self.method.clone().into(),
            self.reference.clone().into(),
            self.received_at.into(),
            self.notes.clone().into(),
            self.created_at.into(),
        ]
    }
}
