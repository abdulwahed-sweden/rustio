//! Auto form generation — derive a [`FormConfig`] from a Rust type
//! that implements [`FormModel`], no manual field-by-field wiring.
//!
//! Callers describe their model once (as a `Vec<AutoField>` in
//! `form_fields`), and [`form_from_model`] or the fluent
//! [`FormBuilder`] produce the same [`FormConfig`] the manual
//! engine consumes. The existing [`crate::admin::form`] renderer is
//! untouched — this module is purely additive.
//!
//! No macros, no runtime reflection, no DB introspection. Pure typed
//! metadata supplied by the caller.

use crate::admin::form::{infer_field_type, FieldConfig, FieldType, FormConfig};

// ---------------------------------------------------------------
// Model-side description
// ---------------------------------------------------------------

/// Declarative description of one field on a model. The caller fills
/// this in once per column; the converter turns it into a
/// [`FieldConfig`].
///
/// `field_type = None` asks the engine to infer the type from the
/// field's `name` (via [`infer_field_type`]). `is_foreign_key = true`
/// always wins — even if `field_type` or inference says otherwise,
/// the final widget is a `<select>` populated from `options`.
#[derive(Debug, Clone)]
pub struct AutoField {
    pub name: &'static str,
    pub label: &'static str,

    /// If `None`, [`infer_field_type`] runs against `name`.
    pub field_type: Option<FieldType>,

    pub required: bool,

    /// Forces [`FieldType::ForeignKey`], overriding both `field_type`
    /// and inference. The FK is always rendered as a `<select>` with
    /// `options` supplying human-readable labels.
    pub is_foreign_key: bool,

    /// `(value, label)` pairs consumed by Select / ForeignKey.
    pub options: Vec<(String, String)>,
}

/// Types that can describe their own form shape.
///
/// Invoked through the turbofish: `FormBuilder::from_model::<User>()`
/// — no instance needed, so this composes with unit structs used
/// purely as type tags.
pub trait FormModel {
    fn form_fields() -> Vec<AutoField>;
    fn form_title() -> &'static str;
}

// ---------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------

/// Build a [`FormConfig`] from a model's [`FormModel`] impl. The
/// resulting form has no `value`s and no `help` — overrides populate
/// those via [`FormBuilder::override_field`].
pub fn form_from_model<T: FormModel>() -> FormConfig {
    let fields = T::form_fields()
        .into_iter()
        .map(field_config_from)
        .collect();

    FormConfig {
        title: T::form_title().to_string(),
        // Subtitle isn't part of the FormModel contract today; a
        // future extension could derive it from a type name or an
        // extra trait method. Empty string renders as an empty
        // `.drawer-subtitle` div, which the approved CSS handles.
        subtitle: String::new(),
        fields,
    }
}

fn field_config_from(f: AutoField) -> FieldConfig {
    // 1. Start from the caller's declared type, falling back to name
    //    inference when the caller left it blank.
    let mut ty = match f.field_type {
        Some(t) => t,
        None => infer_field_type(f.name),
    };
    // 2. `is_foreign_key` is the hard override — it always wins,
    //    even against a caller-declared `field_type`. Matches the
    //    invariant "FK columns render as dropdowns, never raw ids".
    if f.is_foreign_key {
        ty = FieldType::ForeignKey;
    }
    FieldConfig {
        name: f.name.to_string(),
        label: f.label.to_string(),
        field_type: ty,
        required: f.required,
        readonly: false,
        placeholder: None,
        help: None,
        value: None,
        options: f.options,
        error: None,
    }
}

// ---------------------------------------------------------------
// Override / builder
// ---------------------------------------------------------------

/// Patch applied to a single field after auto-generation. Every
/// sub-field is `Option` — `None` means "leave as the generated
/// value".
#[derive(Debug, Clone, Default)]
pub struct FieldOverride {
    pub field_type: Option<FieldType>,
    pub label: Option<String>,
    pub help: Option<String>,
}

/// Fluent builder that starts from an auto-generated [`FormConfig`]
/// and lets the caller patch individual fields. An
/// `override_field(name, ...)` call whose `name` doesn't match any
/// generated field is a silent no-op — the builder intentionally
/// doesn't fail on typos so it composes with optional overrides from
/// config files.
pub struct FormBuilder {
    pub form: FormConfig,
}

impl FormBuilder {
    pub fn from_model<T: FormModel>() -> Self {
        Self {
            form: form_from_model::<T>(),
        }
    }

    pub fn override_field(mut self, name: &str, patch: FieldOverride) -> Self {
        if let Some(field) = self.form.fields.iter_mut().find(|f| f.name == name) {
            if let Some(ty) = patch.field_type {
                field.field_type = ty;
            }
            if let Some(label) = patch.label {
                field.label = label;
            }
            if let Some(help) = patch.help {
                field.help = Some(help);
            }
        }
        self
    }

    pub fn build(self) -> FormConfig {
        self.form
    }
}
