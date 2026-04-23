//! AdminUiModel → Form bridge.
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
/// column list). Invoked through the turbofish:
/// `FormBuilder::from_admin_ui_model::<UserAdmin>()`.
pub trait AdminUiModel {
    fn model_name() -> &'static str;
    fn fields() -> Vec<AdminUiField>;
}

// ---------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------

/// Build a [`FormConfig`] from an [`AdminUiModel`] impl. The drawer
/// title becomes `"Edit <model_name>"`; subtitle is empty (the
/// `AdminUiModel` contract has no subtitle slot today).
pub fn form_from_admin_ui_model<T: AdminUiModel>() -> FormConfig {
    let fields = T::fields().into_iter().map(field_config_from).collect();
    FormConfig {
        title: format!("Edit {}", T::model_name()),
        subtitle: String::new(),
        fields,
        submitted: false,
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
    /// Equivalent to `FormBuilder { form: form_from_admin_ui_model::<T>() }`,
    /// but reads naturally at the call site.
    pub fn from_admin_ui_model<T: AdminUiModel>() -> Self {
        Self {
            form: form_from_admin_ui_model::<T>(),
        }
    }
}
