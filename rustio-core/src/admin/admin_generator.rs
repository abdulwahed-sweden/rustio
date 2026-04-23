//! Config-driven admin model generator.
//!
//! [`AdminModelConfig`] holds the same metadata a hand-written
//! [`AdminUiModel`] impl exposes (slug, table, fields, search /
//! status / ensure-table SQL). [`GeneratedAdminModel`] then wraps
//! one of these configs and implements [`AdminUiModel`] by simply
//! delegating to the stored config — no per-model code path.
//!
//! The result is that registering a new admin section becomes a
//! single declarative call:
//!
//! ```ignore
//! register_generated(
//!     &mut registry,
//!     AdminModelConfig::new("orders", "Order")
//!         .table("admin_new_demo_orders")
//!         .primary_key("id")
//!         .fields(vec![
//!             AdminUiField::text("order_number", "Order #")
//!                 .required(true).filterable(true).sortable(true),
//!             …
//!         ])
//!         .searchable(vec!["order_number", "customer_email"])
//!         .status_field("is_paid")
//!         .ensure_sql("CREATE TABLE IF NOT EXISTS …"),
//! );
//! ```
//!
//! All routes (`/admin-new/<slug>`, search, filters, sort, pagination,
//! bulk actions, row delete, edit drawer) keep working unchanged —
//! they consume `&dyn AdminUiModel`, and a `GeneratedAdminModel`
//! satisfies that contract just like any unit-struct impl.

use crate::admin::admin_form_bridge::{AdminUiField, AdminUiModel};

/// Declarative description of an admin section. Cheap to clone —
/// the registry's factory closure clones one of these on every
/// lookup so each request gets a fresh boxed model.
#[derive(Debug, Clone)]
pub struct AdminModelConfig {
    pub slug: &'static str,
    pub model_name: &'static str,
    pub table_name: &'static str,
    pub primary_key: &'static str,
    pub fields: Vec<AdminUiField>,
    pub searchable_fields: Vec<&'static str>,
    pub primary_status_field: Option<&'static str>,
    /// `Some(sql)` runs an idempotent `CREATE TABLE IF NOT EXISTS …`
    /// on every request; `None` skips auto-creation (caller owns
    /// migrations). Mirrors the [`AdminUiModel::ensure_table_sql`]
    /// contract directly.
    pub ensure_table_sql: Option<&'static str>,
}

impl AdminModelConfig {
    /// Start a new config with required identity fields. Defaults:
    /// `table_name = ""`, `primary_key = "id"`, no fields, nothing
    /// searchable, no status column, no ensure-table SQL. Call the
    /// builder methods below before registration.
    pub fn new(slug: &'static str, model_name: &'static str) -> Self {
        Self {
            slug,
            model_name,
            table_name: "",
            primary_key: "id",
            fields: Vec::new(),
            searchable_fields: Vec::new(),
            primary_status_field: None,
            ensure_table_sql: None,
        }
    }

    pub fn table(mut self, table_name: &'static str) -> Self {
        self.table_name = table_name;
        self
    }

    pub fn primary_key(mut self, primary_key: &'static str) -> Self {
        self.primary_key = primary_key;
        self
    }

    pub fn fields(mut self, fields: Vec<AdminUiField>) -> Self {
        self.fields = fields;
        self
    }

    pub fn searchable(mut self, searchable_fields: Vec<&'static str>) -> Self {
        self.searchable_fields = searchable_fields;
        self
    }

    pub fn status_field(mut self, name: &'static str) -> Self {
        self.primary_status_field = Some(name);
        self
    }

    pub fn ensure_sql(mut self, sql: &'static str) -> Self {
        self.ensure_table_sql = Some(sql);
        self
    }
}

/// Adapter that turns an [`AdminModelConfig`] into an
/// [`AdminUiModel`]. Holds the config by value so the registry's
/// closure can construct one per request without lifetime gymnastics.
pub struct GeneratedAdminModel {
    pub config: AdminModelConfig,
}

impl AdminUiModel for GeneratedAdminModel {
    fn slug(&self) -> &'static str {
        self.config.slug
    }
    fn model_name(&self) -> &'static str {
        self.config.model_name
    }
    fn table_name(&self) -> &'static str {
        self.config.table_name
    }
    fn primary_key(&self) -> &'static str {
        self.config.primary_key
    }
    fn fields(&self) -> Vec<AdminUiField> {
        self.config.fields.clone()
    }
    fn searchable_fields(&self) -> Vec<&'static str> {
        self.config.searchable_fields.clone()
    }
    fn primary_status_field(&self) -> Option<&'static str> {
        self.config.primary_status_field
    }
    fn ensure_table_sql(&self) -> Option<&'static str> {
        self.config.ensure_table_sql
    }
}

/// Convenience factory matching the registry's `Fn() -> Box<dyn
/// AdminUiModel>` shape.
pub fn from_config(config: AdminModelConfig) -> Box<dyn AdminUiModel> {
    Box::new(GeneratedAdminModel { config })
}
