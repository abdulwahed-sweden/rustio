use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// A Location is a service area or delivery point — a depot, a clinic
/// site, a city zone. Resources live at a Location; bookings are
/// fulfilled within one.
#[derive(Debug, RustioAdmin)]
pub struct Location {
    pub id: i64,
    pub name: String,
    /// Short region/zone code, e.g. "SE-STO", "US-WEST".
    pub region_code: String,
    pub address: String,
    pub active: bool,
}

impl Model for Location {
    const TABLE: &'static str = "locations";
    const COLUMNS: &'static [&'static str] = &["id", "name", "region_code", "address", "active"];
    const INSERT_COLUMNS: &'static [&'static str] = &["name", "region_code", "address", "active"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            region_code: row.get_string("region_code")?,
            address: row.get_string("address")?,
            active: row.get_bool("active")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.name.clone().into(),
            self.region_code.clone().into(),
            self.address.clone().into(),
            self.active.into(),
        ]
    }
}
