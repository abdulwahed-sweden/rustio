use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

use crate::apps::clients::models::Client;

/// A retail product order placed by a Client. `status` cycles
/// "new" → "paid" → "fulfilled" / "refunded". `price_cents` is öre.
#[derive(Debug, RustioAdmin)]
pub struct Order {
    pub id: i64,
    #[rustio(belongs_to = "Client", display = "name")]
    pub client_id: i64,
    pub item_name: String,
    pub quantity: i32,
    pub price_cents: i64,
    /// enum: "new" | "paid" | "fulfilled" | "refunded"
    pub status: String,
    pub ordered_at: DateTime<Utc>,
}

impl Model for Order {
    const TABLE: &'static str = "orders";
    const COLUMNS: &'static [&'static str] = &[
        "id",
        "client_id",
        "item_name",
        "quantity",
        "price_cents",
        "status",
        "ordered_at",
    ];
    const INSERT_COLUMNS: &'static [&'static str] = &[
        "client_id",
        "item_name",
        "quantity",
        "price_cents",
        "status",
        "ordered_at",
    ];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            client_id: row.get_i64("client_id")?,
            item_name: row.get_string("item_name")?,
            quantity: row.get_i32("quantity")?,
            price_cents: row.get_i64("price_cents")?,
            status: row.get_string("status")?,
            ordered_at: row.get_datetime("ordered_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.client_id.into(),
            self.item_name.clone().into(),
            self.quantity.into(),
            self.price_cents.into(),
            self.status.clone().into(),
            self.ordered_at.into(),
        ]
    }
}
