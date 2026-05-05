//! Phase 14, commit 5 — bridge from `ModelSchema` to admin metadata.
//!
//! This module is the framework's first real consumer of the
//! Phase 14 schema contract. Given a `&ModelSchema` produced by
//! `#[derive(RustioModel)]`, it emits the per-column admin
//! metadata required by the existing admin UI without a hand-
//! written `AdminModel` impl.
//!
//! # What stays untouched
//!
//! Existing manual admin paths (the `#[derive(RustioAdmin)]`
//! macro and projects that hand-build an `AdminModel`) are not
//! affected. This module is **additive** — it produces values
//! that consumers can plug into the existing `AdminEntry`
//! constructor; it never modifies, replaces, or shadows any
//! existing admin type.
//!
//! # Mapping rules (Phase 14, commit 5 spec)
//!
//! For each `ModelColumn`:
//!
//! | Contract field      | Bridge output                              |
//! |---------------------|--------------------------------------------|
//! | `name`              | `AdminField.name` (verbatim)               |
//! | `admin_label`       | `AdminField.label` (fallback = `name`)     |
//! | `admin_widget`      | `BridgedField.widget` (preserved through)  |
//! | `flags.searchable`  | `BridgedField.searchable`                  |
//! | `flags.filterable`  | `BridgedField.filterable`                  |
//! | `flags.sortable`    | `BridgedField.sortable`                    |
//! | `flags.readonly`    | `BridgedField.readonly` + `editable=!ro`   |
//! | `primary_key`       | `BridgedField.primary_key`                 |
//!
//! `AdminField` (the existing type) only models `editable`. The
//! remaining flag bits and the widget hint live on
//! `BridgedField` — a side-channel struct so consumers (search
//! indexer, list/sort UI, future renderer changes) can read them
//! without breaking `AdminField`'s shape.
//!
//! # Static lifetimes via `Box::leak`
//!
//! `AdminField` requires `&'static str` and an `&'static
//! [AdminField]` slice (the existing macro emits compile-time
//! constants). When bridging at runtime, we promote owned data
//! to static via `Box::leak`. This is a one-time setup cost
//! equivalent to a `static`: schemas are registered at process
//! startup and live for the program's lifetime, so leaked memory
//! is never reclaimed but never grows either.
//!
//! # No DB, no reflection, no new deps
//!
//! Pure CPU. No async, no database access, no `unsafe`, no new
//! `Cargo.toml` entries.

use crate::admin::types::{AdminField, FieldType};
use crate::contract::{ModelColumn, ModelSchema, RustType};

// ---------------------------------------------------------------------------
// FieldType mapping
// ---------------------------------------------------------------------------

/// Map a contract column's `(RustType, nullable)` pair to the
/// admin's `FieldType` vocabulary.
///
/// Variants the admin layer doesn't model natively (`F64`,
/// `Decimal`, `JsonValue`, `Uuid`) fall through to
/// `String` / `OptionalString` — admin renders them as text
/// inputs, which preserves their values without inventing
/// widgets that don't exist yet. Future commits may extend
/// `FieldType` with dedicated variants; until then text input
/// is the safe minimum.
pub fn field_type_for(col: &ModelColumn) -> FieldType {
    // Match exhaustively — `RustType` is `#[non_exhaustive]` only
    // cross-crate, but inside `rustio-core` it's exhaustive. Keeping
    // the match tight means adding a future variant fails compilation
    // here until the bridge gets an explicit mapping; a wildcard
    // would silently fall back to text input and mask the gap.
    use RustType::*;
    match col.rust_type {
        // The admin's `FieldType` has no `OptionalI32` variant; a
        // nullable `i32` column collapses to `OptionalI64` since
        // both render as the same numeric input. Non-nullable
        // `i32` keeps its dedicated variant.
        I32 if col.nullable => FieldType::OptionalI64,
        I32 => FieldType::I32,
        I64 if col.nullable => FieldType::OptionalI64,
        I64 => FieldType::I64,
        // `Bool` has no nullable variant in the admin layer; a
        // tri-state checkbox isn't part of the existing UI, so
        // nullable bools render as the same checkbox (NULL is
        // treated as `false` at form-submission time).
        Bool => FieldType::Bool,
        String if col.nullable => FieldType::OptionalString,
        String => FieldType::String,
        DateTimeUtc if col.nullable => FieldType::OptionalDateTime,
        DateTimeUtc => FieldType::DateTime,
        // Variants the admin layer doesn't model natively — `F64`,
        // `Decimal`, `JsonValue`, `Uuid` — collapse to text inputs
        // (`String` / `OptionalString`). Documented behaviour;
        // a future commit may extend `FieldType` with dedicated
        // variants and tighten these.
        F64 | Decimal | JsonValue | Uuid if col.nullable => FieldType::OptionalString,
        F64 | Decimal | JsonValue | Uuid => FieldType::String,
    }
}

// ---------------------------------------------------------------------------
// Label resolution
// ---------------------------------------------------------------------------

/// Resolved admin label per the spec's fallback rule:
/// `admin_label` when set, otherwise the column's name verbatim.
///
/// Both branches return `&'static str` so the result drops
/// directly into `AdminField.label` without a leak — explicit
/// labels are already `&'static` (compile-time constants from
/// the macro), and `col.name` is `&'static` by `ModelColumn`
/// definition.
pub fn label_for(col: &ModelColumn) -> &'static str {
    col.admin_label.unwrap_or(col.name)
}

// ---------------------------------------------------------------------------
// BridgedField
// ---------------------------------------------------------------------------

/// One column in its bridge form: the existing `AdminField`
/// (consumed verbatim by the admin UI) plus the column-level
/// flags `AdminField` doesn't model.
///
/// Consumers:
/// - The admin renderer plucks `.field` out for `AdminEntry`.
/// - A search-index sync layer (commit 6) reads `.searchable`.
/// - Future filter/sort UI reads `.filterable` / `.sortable`.
/// - The `.primary_key` bit identifies the row-id column for
///   any code that needs it without re-scanning the schema.
#[derive(Debug, Clone)]
pub struct BridgedField {
    /// The existing-shape admin field. Plug this directly into
    /// `AdminEntry.fields` (after `Box::leak`-ing the slice).
    pub field: AdminField,
    /// `true` when the source column has `primary_key = true`.
    /// Mirrors `ModelColumn.primary_key`.
    pub primary_key: bool,
    /// `flags.searchable` from the source column.
    pub searchable: bool,
    /// `flags.filterable` from the source column.
    pub filterable: bool,
    /// `flags.sortable` from the source column.
    pub sortable: bool,
    /// `flags.readonly` from the source column. Also drives
    /// `field.editable = !readonly`.
    pub readonly: bool,
    /// `admin_widget` from the source column, preserved
    /// verbatim. `AdminField` doesn't carry a widget override
    /// today; the existing renderer derives the widget from
    /// `FieldType.widget()`. Holding the override here lets
    /// future renderer code consult it without altering
    /// `AdminField`'s shape.
    pub widget: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Public bridge API
// ---------------------------------------------------------------------------

/// Bridge every column in declaration order. Order is
/// preserved 1:1 with `schema.columns` — the admin UI lists
/// columns in the order the model declared them, and skipping
/// or reordering would silently change rendered forms.
pub fn bridged_fields_from_schema(schema: &ModelSchema) -> Vec<BridgedField> {
    schema
        .columns
        .iter()
        .map(|col| BridgedField {
            field: AdminField {
                name: col.name,
                label: label_for(col),
                field_type: field_type_for(col),
                editable: !col.flags.readonly,
                relation: None,
                choices: None,
            },
            primary_key: col.primary_key,
            searchable: col.flags.searchable,
            filterable: col.flags.filterable,
            sortable: col.flags.sortable,
            readonly: col.flags.readonly,
            widget: col.admin_widget,
        })
        .collect()
}

/// Static-leaked `&'static [AdminField]` for direct use as
/// `AdminEntry.fields`. Equivalent to a `static` array — the
/// memory is allocated once and lives the program's lifetime.
pub fn admin_fields_from_schema(schema: &ModelSchema) -> &'static [AdminField] {
    let fields: Vec<AdminField> = bridged_fields_from_schema(schema)
        .into_iter()
        .map(|b| b.field)
        .collect();
    Box::leak(fields.into_boxed_slice())
}

/// The schema's primary-key column, located by the
/// `primary_key = true` flag. Returns `None` when no column
/// is flagged (a malformed schema; the validator in commit 3
/// surfaces this as `WrongPrimaryKey`).
pub fn primary_key_column(schema: &ModelSchema) -> Option<&ModelColumn> {
    schema.columns.iter().find(|c| c.primary_key)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::SchemaFlags;

    // ----- Test fixture -----------------------------------------------------

    /// A schema covering every mapping rule the bridge must
    /// honour: primary key, label override, widget override,
    /// every flag bit, every commonly-used `RustType`, both
    /// nullable and non-nullable. One static fixture so the
    /// individual tests don't drift from each other.
    fn fixture_schema() -> ModelSchema {
        static COLS: &[ModelColumn] = &[
            // Primary key, readonly (auto-managed).
            ModelColumn {
                name: "id",
                sql_decl: "BIGSERIAL PRIMARY KEY",
                rust_type: RustType::I64,
                nullable: false,
                primary_key: true,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: false,
                    sortable: true,
                    readonly: true,
                },
                admin_label: None,
                admin_widget: None,
            },
            // Searchable + filterable + explicit label.
            ModelColumn {
                name: "title",
                sql_decl: "TEXT NOT NULL",
                rust_type: RustType::String,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: true,
                    filterable: true,
                    sortable: false,
                    readonly: false,
                },
                admin_label: Some("Headline"),
                admin_widget: None,
            },
            // Nullable string + widget override.
            ModelColumn {
                name: "body",
                sql_decl: "TEXT",
                rust_type: RustType::String,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: true,
                    filterable: false,
                    sortable: false,
                    readonly: false,
                },
                admin_label: None,
                admin_widget: Some("textarea"),
            },
            // Nullable timestamp + sortable.
            ModelColumn {
                name: "published_at",
                sql_decl: "TIMESTAMPTZ",
                rust_type: RustType::DateTimeUtc,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: true,
                    sortable: true,
                    readonly: false,
                },
                admin_label: None,
                admin_widget: None,
            },
            // Bool, no flags set.
            ModelColumn {
                name: "is_pinned",
                sql_decl: "BOOLEAN NOT NULL",
                rust_type: RustType::Bool,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            },
        ];
        ModelSchema {
            table: "posts",
            columns: COLS,
            primary_key: "id",
            search_index: Some("posts"),
        }
    }

    // ----- Required by spec -------------------------------------------------

    /// Spec gate: "fields generated from schema". One
    /// `BridgedField` per column, none dropped, none added.
    #[test]
    fn fields_generated_from_schema_one_per_column() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        assert_eq!(
            bridged.len(),
            schema.columns.len(),
            "every ModelColumn must produce exactly one BridgedField"
        );
    }

    /// Spec gate: "ordering preserved". Bridge output order
    /// matches the schema's declaration order column-for-column.
    /// Reordering would silently re-arrange admin forms.
    #[test]
    fn ordering_preserved_matches_schema_columns() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        let bridged_names: Vec<&str> = bridged.iter().map(|b| b.field.name).collect();
        let schema_names: Vec<&str> = schema.columns.iter().map(|c| c.name).collect();
        assert_eq!(
            bridged_names, schema_names,
            "BridgedField order must mirror ModelSchema.columns order"
        );
    }

    /// Spec gate: "flags correctly mapped". Every flag bit
    /// flows through to the matching `BridgedField` field.
    #[test]
    fn flags_correctly_mapped_per_column() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // `id`: sortable + readonly only.
        let id = &bridged[0];
        assert!(!id.searchable);
        assert!(!id.filterable);
        assert!(id.sortable);
        assert!(id.readonly);
        assert!(!id.field.editable, "readonly => editable=false");

        // `title`: searchable + filterable.
        let title = &bridged[1];
        assert!(title.searchable);
        assert!(title.filterable);
        assert!(!title.sortable);
        assert!(!title.readonly);
        assert!(title.field.editable);

        // `body`: searchable only.
        let body = &bridged[2];
        assert!(body.searchable);
        assert!(!body.filterable);
        assert!(!body.sortable);
        assert!(!body.readonly);

        // `published_at`: filterable + sortable.
        let pa = &bridged[3];
        assert!(!pa.searchable);
        assert!(pa.filterable);
        assert!(pa.sortable);

        // `is_pinned`: all flags off.
        let pin = &bridged[4];
        assert!(!pin.searchable);
        assert!(!pin.filterable);
        assert!(!pin.sortable);
        assert!(!pin.readonly);
    }

    /// Spec gate: "label fallback works". When `admin_label`
    /// is set, the bridge uses it verbatim; when `None`, the
    /// fallback is the column's name string (per spec table:
    /// "fallback = name").
    #[test]
    fn label_fallback_uses_column_name_when_no_override() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // Override case: `title` had `admin_label = Some("Headline")`.
        assert_eq!(bridged[1].field.label, "Headline");

        // Fallback case: `id`, `body`, `published_at`, `is_pinned`
        // all had `admin_label = None`.
        assert_eq!(bridged[0].field.label, "id");
        assert_eq!(bridged[2].field.label, "body");
        assert_eq!(bridged[3].field.label, "published_at");
        assert_eq!(bridged[4].field.label, "is_pinned");
    }

    /// Spec gate: "widget override works". `admin_widget`
    /// from the source column is preserved on `BridgedField.widget`
    /// verbatim. `None` stays `None`.
    #[test]
    fn widget_override_preserved_through_bridge() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        assert_eq!(bridged[2].widget, Some("textarea"), "body's textarea override must survive");
        assert!(bridged[0].widget.is_none(), "id had no widget override");
        assert!(bridged[1].widget.is_none(), "title had no widget override");
        assert!(bridged[3].widget.is_none(), "published_at had no widget override");
        assert!(bridged[4].widget.is_none(), "is_pinned had no widget override");
    }

    /// Spec gate: "primary key detected". The bridge surfaces
    /// the same column flagged in the contract; helper picks
    /// it out by reference.
    #[test]
    fn primary_key_detected_from_schema() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);

        // Exactly one column flagged primary_key.
        let pk_count = bridged.iter().filter(|b| b.primary_key).count();
        assert_eq!(pk_count, 1, "fixture has exactly one primary-key column");
        assert!(bridged[0].primary_key, "the `id` column is the PK");
        assert!(!bridged[1].primary_key);

        // Helper resolves the same column.
        let pk = primary_key_column(&schema).expect("PK exists in fixture");
        assert_eq!(pk.name, "id");
    }

    /// `primary_key_column` returns `None` when no column is
    /// flagged. The validator in commit 3 surfaces this as a
    /// `WrongPrimaryKey` issue; the bridge just reports honestly.
    #[test]
    fn primary_key_column_returns_none_when_unflagged() {
        static COLS: &[ModelColumn] = &[ModelColumn {
            name: "value",
            sql_decl: "TEXT NOT NULL",
            rust_type: RustType::String,
            nullable: false,
            primary_key: false,
            flags: SchemaFlags::empty(),
            admin_label: None,
            admin_widget: None,
        }];
        let schema = ModelSchema {
            table: "scratch",
            columns: COLS,
            primary_key: "value",
            search_index: None,
        };
        assert!(primary_key_column(&schema).is_none());
    }

    // ----- FieldType mapping -----------------------------------------------

    /// Every `RustType` variant the admin natively models has
    /// the documented `(nullable, non-nullable)` pair.
    #[test]
    fn field_type_mapping_covers_native_variants() {
        // Helper: build a stub column for type-mapping checks.
        fn col(rust_type: RustType, nullable: bool) -> ModelColumn {
            ModelColumn {
                name: "f",
                sql_decl: "",
                rust_type,
                nullable,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            }
        }

        assert_eq!(field_type_for(&col(RustType::I32, false)), FieldType::I32);
        // No OptionalI32 variant — nullable I32 collapses into OptionalI64.
        assert_eq!(field_type_for(&col(RustType::I32, true)), FieldType::OptionalI64);

        assert_eq!(field_type_for(&col(RustType::I64, false)), FieldType::I64);
        assert_eq!(field_type_for(&col(RustType::I64, true)), FieldType::OptionalI64);

        assert_eq!(field_type_for(&col(RustType::Bool, false)), FieldType::Bool);
        assert_eq!(field_type_for(&col(RustType::Bool, true)), FieldType::Bool);

        assert_eq!(field_type_for(&col(RustType::String, false)), FieldType::String);
        assert_eq!(field_type_for(&col(RustType::String, true)), FieldType::OptionalString);

        assert_eq!(field_type_for(&col(RustType::DateTimeUtc, false)), FieldType::DateTime);
        assert_eq!(field_type_for(&col(RustType::DateTimeUtc, true)), FieldType::OptionalDateTime);
    }

    /// Variants the admin doesn't natively model (`F64`,
    /// `Decimal`, `JsonValue`, `Uuid`) collapse to text inputs.
    /// Documented behaviour; a future commit may extend
    /// `FieldType` and tighten these.
    #[test]
    fn field_type_mapping_falls_back_to_string_for_unmodelled_variants() {
        fn col(rust_type: RustType, nullable: bool) -> ModelColumn {
            ModelColumn {
                name: "f",
                sql_decl: "",
                rust_type,
                nullable,
                primary_key: false,
                flags: SchemaFlags::empty(),
                admin_label: None,
                admin_widget: None,
            }
        }

        for rt in [RustType::F64, RustType::Decimal, RustType::JsonValue, RustType::Uuid] {
            assert_eq!(field_type_for(&col(rt, false)), FieldType::String, "{:?} -> String", rt);
            assert_eq!(field_type_for(&col(rt, true)), FieldType::OptionalString, "{:?} -> OptionalString", rt);
        }
    }

    // ----- AdminField slice helper -----------------------------------------

    /// `admin_fields_from_schema` returns a slice the same
    /// length and order as `bridged_fields_from_schema`, with
    /// each `AdminField` matching the bridge output verbatim.
    #[test]
    fn admin_fields_slice_matches_bridge_output() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        let slice = admin_fields_from_schema(&schema);

        assert_eq!(slice.len(), bridged.len());
        for (i, f) in slice.iter().enumerate() {
            assert_eq!(f.name, bridged[i].field.name, "name @{}", i);
            assert_eq!(f.label, bridged[i].field.label, "label @{}", i);
            assert_eq!(f.field_type, bridged[i].field.field_type, "field_type @{}", i);
            assert_eq!(f.editable, bridged[i].field.editable, "editable @{}", i);
        }
    }

    /// The slice satisfies the `&'static [AdminField]` shape
    /// `AdminEntry.fields` requires — it can be used in places
    /// where a `'static` lifetime is mandatory. Compile-time
    /// gate: this won't compile if the helper returns a non-
    /// static reference.
    #[test]
    fn admin_fields_slice_is_static_lifetime() {
        fn assert_static(_x: &'static [AdminField]) {}
        let schema = fixture_schema();
        let slice = admin_fields_from_schema(&schema);
        assert_static(slice);
    }

    /// A column with `flags.readonly = true` produces
    /// `AdminField.editable = false`. The inverse
    /// (readonly = false → editable = true) is also covered.
    #[test]
    fn editable_is_inverse_of_readonly() {
        let schema = fixture_schema();
        let bridged = bridged_fields_from_schema(&schema);
        for b in &bridged {
            assert_eq!(
                b.field.editable, !b.readonly,
                "editable must always equal !readonly for `{}`",
                b.field.name
            );
        }
    }

    /// Empty schema → empty bridge output. A schema with zero
    /// columns is malformed but the bridge shouldn't panic.
    #[test]
    fn empty_schema_produces_empty_bridge_output() {
        static COLS: &[ModelColumn] = &[];
        let schema = ModelSchema {
            table: "empty",
            columns: COLS,
            primary_key: "id",
            search_index: None,
        };
        assert_eq!(bridged_fields_from_schema(&schema).len(), 0);
        assert_eq!(admin_fields_from_schema(&schema).len(), 0);
        assert!(primary_key_column(&schema).is_none());
    }
}
