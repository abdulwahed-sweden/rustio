//! AdminUiModel → Form bridge + model registry.
//!
//! Lifts an admin-level metadata description (column data types,
//! relations, options) into a [`FormConfig`] the existing form engine
//! already knows how to render. The mapping is purely declarative and
//! deterministic: same input → same output, every time.
//!
//! The trait + struct here are deliberately named [`AdminUiModel`] /
//! [`AdminUiField`] (not `AdminModel` / `AdminField`). The framework's
//! existing admin layer in `crate::admin` already owns the unsuffixed
//! names with a different shape; the `Ui` suffix keeps the two
//! vocabularies unambiguous in any glob import.
//!
//! As of this step the trait uses **`&self` methods** (object-safe)
//! so an [`AdminRegistry`] can store `Box<dyn AdminUiModel>` and the
//! `/admin-new/<slug>` route can pick a model by URL slug.

use std::collections::HashMap;

use crate::admin::auto_form::FormBuilder;
use crate::admin::form::{FieldConfig, FieldType, FormConfig};

// ---------------------------------------------------------------
// Admin-level metadata
// ---------------------------------------------------------------

/// Storage / semantic data type for a column. Translated into a form
/// [`FieldType`] by [`form_from_admin_ui_model`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminDataType {
    String,
    Text,
    Integer,
    Float,
    Boolean,
    DateTime,
    Email,
}

#[derive(Debug, Clone)]
pub struct AdminUiField {
    pub name: &'static str,
    pub label: &'static str,

    pub data_type: AdminDataType,

    pub required: bool,
    pub readonly: bool,

    /// `true` when the column points at another model (FK). Forces
    /// the bridge to emit [`FieldType::ForeignKey`] regardless of
    /// `data_type` — a `<select>` populated from `options` is the
    /// only correct rendering.
    pub is_relation: bool,

    /// `(value, label)` pairs supplied for FK / enum-like columns.
    pub options: Vec<(String, String)>,

    /// `true` → field is rendered as a default filter in the
    /// toolbar. The control type is decided by [`resolve_filter_type`]
    /// from the field's `data_type` / `is_relation` / `options`.
    pub filterable: bool,

    /// `true` → field is offered in the "+ Add filter" advanced
    /// dropdown, not the always-visible toolbar. A field can set
    /// neither (no filter), one, or both.
    pub advanced_filter: bool,

    /// `true` → table column header for this field becomes a
    /// clickable sort link (`?sort=<name>&dir=asc|desc`). Fields
    /// with `sortable = false` are silently rejected even if the
    /// URL asks for them — metadata is the gate.
    pub sortable: bool,

    /// `true` → column appears in the listing table. `false` keeps
    /// the field in the form / detail view but hides it from the
    /// rows. Defaults to `true` for editable columns.
    pub visible_in_table: bool,
}

/// How a filter input should render and how SQL should query it.
/// Resolved from [`AdminUiField`] via [`resolve_filter_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterType {
    /// Boolean column → tri-state `<select>` (All / true / false),
    /// SQL: `column = ?`.
    Boolean,
    /// Enum-like / FK column → `<select>` populated from `options`,
    /// SQL: `column = ?`.
    Select,
    /// Free-text column → `<input type="text">`,
    /// SQL: `LOWER(column) LIKE ?` with `%value%`.
    Exact,
}

/// Decide which control + SQL operator a filter on `field` should
/// use. Pure function — does not look at any data, only metadata.
pub fn resolve_filter_type(field: &AdminUiField) -> FilterType {
    if field.data_type == AdminDataType::Boolean {
        FilterType::Boolean
    } else if field.is_relation || !field.options.is_empty() {
        FilterType::Select
    } else {
        FilterType::Exact
    }
}

/// A model that can describe its admin-UI shape (display name +
/// table mapping + column list + searchable / sortable / filterable
/// / status semantics).
///
/// **Object-safe** (`&self` methods + `Send + Sync + 'static`) so
/// implementations can be stored as `Box<dyn AdminUiModel>` in
/// [`AdminRegistry`] for dynamic-by-URL-slug dispatch.
pub trait AdminUiModel: Send + Sync + 'static {
    /// URL slug used as the `:model` path segment, e.g. `"users"` →
    /// `/admin-new/users`. Must be unique within a registry.
    fn slug(&self) -> &'static str;

    /// Human-readable display name shown in subtitles / banners,
    /// e.g. `"User"`. Used in `format!("{} · {} records", …)`.
    fn model_name(&self) -> &'static str;

    /// SQL table name. `quote_ident` is applied by the persistence
    /// layer, so callers can safely pass a static identifier.
    fn table_name(&self) -> &'static str;

    /// Primary-key column name. Used by the persistence layer for
    /// `WHERE pk = ?` lookups and by the form engine to skip the
    /// PK from auto-generated INSERT / UPDATE column maps.
    fn primary_key(&self) -> &'static str;

    fn fields(&self) -> Vec<AdminUiField>;

    /// Field names participating in free-text search (`?q=…`).
    /// Persistence emits a single `OR`-clause across these columns
    /// with `LOWER(col) LIKE ?`.
    fn searchable_fields(&self) -> Vec<&'static str>;

    /// Boolean column the bulk Activate/Deactivate actions flip
    /// (typically `is_active`). `None` disables those bulk actions
    /// for the model — Delete still works since it doesn't depend on
    /// a status column.
    fn primary_status_field(&self) -> Option<&'static str>;

    /// Optional `CREATE TABLE IF NOT EXISTS …` statement run on
    /// every request. Returning `None` skips auto-creation (caller
    /// is responsible for migrations). Idempotent SQL is required.
    fn ensure_table_sql(&self) -> Option<&'static str>;
}

// ---------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------

/// Build a [`FormConfig`] from an [`AdminUiModel`] impl. The drawer
/// title becomes `"Edit <model_name>"`; subtitle is empty (the
/// `AdminUiModel` contract has no subtitle slot today).
pub fn form_from_admin_ui_model(model: &dyn AdminUiModel) -> FormConfig {
    let fields = model.fields().into_iter().map(field_config_from).collect();
    FormConfig {
        title: format!("Edit {}", model.model_name()),
        subtitle: String::new(),
        fields,
        submitted: false,
        save_failed: false,
        hidden_fields: Vec::new(),
    }
}

fn field_config_from(f: AdminUiField) -> FieldConfig {
    // 1. Base mapping from storage type → form widget.
    let mut ty = match f.data_type {
        AdminDataType::String => FieldType::Text,
        AdminDataType::Text => FieldType::TextArea,
        AdminDataType::Integer => FieldType::Number,
        AdminDataType::Float => FieldType::Number,
        AdminDataType::Boolean => FieldType::Boolean,
        AdminDataType::DateTime => FieldType::DateTime,
        AdminDataType::Email => FieldType::Email,
    };

    // 2. Relation override — FK always wins over the data-type
    //    mapping. The widget is a `<select>`; the user never sees a
    //    raw row id.
    if f.is_relation {
        ty = FieldType::ForeignKey;
    }

    // 3. Options promote non-Boolean / non-FK columns to Select.
    //    (Boolean stays a switch; FK is already handled.)
    if !f.options.is_empty() && ty != FieldType::Boolean && !f.is_relation {
        ty = FieldType::Select;
    }

    FieldConfig {
        name: f.name.to_string(),
        label: f.label.to_string(),
        field_type: ty,
        required: f.required,
        readonly: f.readonly,
        placeholder: None,
        help: None,
        value: None,
        options: f.options,
        error: None,
    }
}

// ---------------------------------------------------------------
// FormBuilder integration
// ---------------------------------------------------------------

impl FormBuilder {
    /// Construct a builder seeded from an [`AdminUiModel`] impl.
    pub fn from_admin_ui_model(model: &dyn AdminUiModel) -> Self {
        Self {
            form: form_from_admin_ui_model(model),
        }
    }
}

// ---------------------------------------------------------------
// Registry: URL slug → boxed model
// ---------------------------------------------------------------

/// Slug → factory mapping for the `/admin-new/<slug>` dispatcher.
///
/// Each registered model is stored as a constructor function; a
/// fresh `Box<dyn AdminUiModel>` is built per lookup. Models are
/// typically zero-sized unit structs so the allocation is
/// effectively free.
pub struct AdminRegistry {
    factories: HashMap<&'static str, fn() -> Box<dyn AdminUiModel>>,
}

impl AdminRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register(&mut self, slug: &'static str, factory: fn() -> Box<dyn AdminUiModel>) {
        self.factories.insert(slug, factory);
    }

    /// Look the slug up; returns a fresh boxed model on hit, `None`
    /// on miss (the route handler turns that into a 404).
    pub fn get(&self, slug: &str) -> Option<Box<dyn AdminUiModel>> {
        self.factories.get(slug).map(|f| f())
    }

    /// Iterate registered slugs (for future model-index pages).
    pub fn slugs(&self) -> impl Iterator<Item = &&'static str> {
        self.factories.keys()
    }
}

impl Default for AdminRegistry {
    fn default() -> Self {
        Self::new()
    }
}
