//! The Schema Contract System (Phase 14 — Commit 1, types only).
//!
//! Single-source-of-truth metadata describing a model's columns, their
//! Rust types, their expected SQL DDL, and the admin/search flags that
//! flow to the rest of the framework.
//!
//! # The five type rules
//!
//! These are the *non-negotiable* defaults. The validator enforces
//! them at runtime; the macro layer enforces them at compile time.
//! Both layers consult `RustType::is_compatible_with` to decide what
//! "matches" means.
//!
//! 1. **IDs** — `i64` ↔ `BIGINT` / `BIGSERIAL`. Never `i32`.
//! 2. **Timestamps** — `DateTime<Utc>` ↔ `TIMESTAMPTZ`. Never `NaiveDateTime`.
//! 3. **Money** — `Decimal` (preferred) or `i64` cents ↔ `NUMERIC`.
//!    Never `f64`. The `RustType::F64` variant exists for non-money
//!    decimals (percentages, scientific data) and is *deliberately
//!    not* compatible with `numeric`.
//! 4. **JSON** — `serde_json::Value` ↔ `JSONB`. Never `JSON`.
//! 5. **Strings** — `String` ↔ `TEXT` (default). `VARCHAR(n)` only
//!    when strictly required (and matches `String` too).
//!
//! See `docs/types.md` for the full mapping table. This module's job
//! is only to *encode* the rules; enforcement is the validator and
//! macro's job in later commits.
//!
//! # Stability contract
//!
//! Adding new variants to `RustType` is a non-breaking minor-version
//! addition (the enum is `#[non_exhaustive]`). Removing or renaming
//! a variant is breaking. Adding fields to `ModelColumn` /
//! `ModelSchema` / `SchemaFlags` is non-breaking when the field has
//! a `Default` (the structs are `#[non_exhaustive]`); removing or
//! retyping a field is breaking.
//!
//! # Phase scope
//!
//! Commit 1 ships only the types and the compatibility helpers. The
//! macro that *generates* a `ModelSchema` from a struct
//! (`#[derive(RustioModel)]`) ships in commit 2; the runtime validator
//! that introspects PostgreSQL and compares against `ModelSchema`
//! ships in commit 3. Nothing in `admin/`, `search/`, `migrations/`,
//! or `examples/` references this module yet.

// ---------------------------------------------------------------------------
// RustType
// ---------------------------------------------------------------------------

/// The Rust types the contract system knows about. One variant per
/// scalar shape; nullability is represented separately via
/// `ModelColumn::nullable` to avoid doubling the variant count.
///
/// Compatibility with PostgreSQL types is given by
/// [`Self::pg_compatible`] / [`Self::is_compatible_with`]. The lists
/// hold both the long form (`information_schema.columns.data_type`,
/// e.g. `"timestamp with time zone"`) and the short `udt_name` form
/// (e.g. `"timestamptz"`) that PostgreSQL exposes for the same type.
/// Comparison is case-insensitive.
///
/// `#[non_exhaustive]` so future variants (`Bytes`, `Inet`, …) are
/// minor-version additions, not breaking changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RustType {
    /// 32-bit signed integer. Maps to PG `integer` / `int4`.
    /// **Not** compatible with `bigint` / `bigserial`; using `i32`
    /// for an ID column violates Type Rule #1 and the validator will
    /// reject it.
    I32,
    /// 64-bit signed integer. Maps to PG `bigint` / `int8` /
    /// `bigserial`. The default for ID columns.
    I64,
    /// 64-bit floating point. Maps to PG `double precision` /
    /// `float8`. **Not** compatible with `numeric` / `decimal` —
    /// money columns must use `Decimal` (Type Rule #3). This variant
    /// exists only for non-money real-valued columns (percentages,
    /// scientific measurements, etc.).
    F64,
    /// Boolean. Maps to PG `boolean` / `bool`.
    Bool,
    /// UTF-8 string. Maps to PG `text` / `varchar` /
    /// `character varying`. Default for text columns; `VARCHAR(n)`
    /// is accepted but the schema rule prefers `TEXT`.
    String,
    /// Timezone-aware UTC timestamp (`chrono::DateTime<Utc>`). Maps
    /// to PG `timestamp with time zone` / `timestamptz` only —
    /// `timestamp without time zone` is **not** compatible (Type
    /// Rule #2 forbids naive timestamps for the framework's internal
    /// reasoning about audit trails).
    DateTimeUtc,
    /// Arbitrary-precision decimal (`rust_decimal::Decimal`). Maps
    /// to PG `numeric` / `decimal`. Money columns use this; using
    /// `f64` for money is forbidden (Type Rule #3).
    Decimal,
    /// JSON document (`serde_json::Value`). Maps to PG `jsonb` only.
    /// `json` (without the `b`) is **not** compatible — the
    /// framework only supports the binary form (Type Rule #4).
    JsonValue,
    /// UUID. Maps to PG `uuid`.
    Uuid,
}

impl RustType {
    /// PostgreSQL types this Rust type is compatible with.
    ///
    /// Names are lowercase. Both the `data_type` long form (e.g.
    /// `"timestamp with time zone"`) and the `udt_name` short form
    /// (e.g. `"timestamptz"`) are listed when PG exposes both. The
    /// validator normalises whatever it reads from
    /// `information_schema.columns` to lowercase before consulting
    /// this list.
    pub fn pg_compatible(&self) -> &'static [&'static str] {
        match self {
            // Type Rule #1: i32 maps ONLY to integer/int4. No
            // bigint, no bigserial — using i32 for an ID column is
            // explicitly rejected.
            RustType::I32 => &["integer", "int4"],
            // Type Rule #1: i64 is the only RustType compatible with
            // bigint / bigserial. The id column on every model goes
            // here.
            RustType::I64 => &["bigint", "int8", "bigserial", "serial8"],
            // Type Rule #3 (negative side): F64 is for `double
            // precision` and friends, NOT for money. The list
            // deliberately excludes "numeric" / "decimal".
            RustType::F64 => &["double precision", "float8", "real", "float4"],
            RustType::Bool => &["boolean", "bool"],
            // Type Rule #5: TEXT is the default; VARCHAR(n) and the
            // long form `character varying` are accepted equivalents.
            // The macro will warn if a `String` field has
            // `sql = "VARCHAR(...)"`, but the validator considers
            // them compatible — the warning is about *style*, not
            // type-system correctness.
            RustType::String => &["text", "varchar", "character varying"],
            // Type Rule #2: TIMESTAMPTZ only. Plain `timestamp` /
            // `timestamp without time zone` is NOT in this list — a
            // `DateTime<Utc>` field over a naive PG timestamp is a
            // validator error.
            RustType::DateTimeUtc => &["timestamp with time zone", "timestamptz"],
            // Type Rule #3 (positive side): Decimal is the only
            // RustType compatible with numeric / decimal. F64 is
            // explicitly not listed here.
            RustType::Decimal => &["numeric", "decimal"],
            // Type Rule #4: JSONB only. Plain `json` is NOT
            // compatible — using JSON without B in PG forfeits
            // indexing and querying performance, and the framework
            // standardises on JSONB.
            RustType::JsonValue => &["jsonb"],
            RustType::Uuid => &["uuid"],
        }
    }

    /// Case-insensitive compatibility check. Pass any form
    /// PostgreSQL might return — the long `data_type`, the short
    /// `udt_name`, mixed case — this method lowercases before
    /// comparing.
    pub fn is_compatible_with(&self, pg_type: &str) -> bool {
        let needle = pg_type.trim().to_lowercase();
        self.pg_compatible().contains(&needle.as_str())
    }
}

// ---------------------------------------------------------------------------
// SchemaFlags — column-level flags
// ---------------------------------------------------------------------------

/// Per-column flags consumed by the admin UI and the search indexer.
///
/// Defaulted to all-`false` so a column with no flags is the safe
/// minimum: editable, not searchable, not filterable, not sortable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SchemaFlags {
    /// Indexed for full-text search (Meili `searchable_attributes`).
    /// Compile-time invariant in the macro layer (commit 2): only
    /// `RustType::String` (and `Optional<String>` via
    /// `ModelColumn::nullable = true`) may set this.
    pub searchable: bool,
    /// Available for `filter=` queries (Meili
    /// `filterable_attributes`).
    pub filterable: bool,
    /// Available for `sort=` queries (Meili `sortable_attributes`).
    pub sortable: bool,
    /// Admin form treats this as readonly (no `<input>`, just a
    /// rendered value). Auto-managed columns (`created_at`,
    /// generated columns) typically set this.
    pub readonly: bool,
}

impl SchemaFlags {
    /// All-`false` flags. Equivalent to `SchemaFlags::default()`,
    /// but `const fn` so it composes inside other `const fn`
    /// constructors. Use this in `static` declarations the macro
    /// emits in commit 2 — `Default::default()` is not const.
    pub const fn empty() -> Self {
        Self {
            searchable: false,
            filterable: false,
            sortable: false,
            readonly: false,
        }
    }

    /// Convenience constructor for the most common case: a fully
    /// indexed text column ("title", "body" on a content model).
    pub const fn searchable() -> Self {
        Self {
            searchable: true,
            filterable: false,
            sortable: false,
            readonly: false,
        }
    }
}

// ---------------------------------------------------------------------------
// ModelColumn
// ---------------------------------------------------------------------------

/// One column in a model's contract.
///
/// All fields are `&'static` references because the contract is built
/// at compile time by `#[derive(RustioModel)]` (commit 2) and lives
/// in static memory. No allocations on the hot path.
///
/// `#[non_exhaustive]` so future fields (e.g. `references` for FKs,
/// `default` for column defaults) can be added without breaking.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelColumn {
    /// Column name as it appears in SQL.
    pub name: &'static str,
    /// Verbatim DDL fragment from the operator's
    /// `#[rustio(sql = "...")]` attribute. The validator parses this
    /// when comparing against `information_schema.columns`. Examples:
    /// `"BIGSERIAL PRIMARY KEY"`, `"TEXT NOT NULL"`,
    /// `"NUMERIC(12,2) NOT NULL DEFAULT 0"`.
    pub sql_decl: &'static str,
    /// Rust type kind. The compatibility check goes through
    /// [`RustType::is_compatible_with`].
    pub rust_type: RustType,
    /// Whether the Rust field is `Option<T>`. When `true`, the
    /// validator expects PG to allow NULL on this column; mismatch
    /// is a validator error.
    pub nullable: bool,
    /// Whether this column is the table's primary key. Exactly one
    /// column per `ModelSchema` should set this to `true`. Validation
    /// of that invariant is the validator's job (commit 3).
    pub primary_key: bool,
    /// Admin / search flags.
    pub flags: SchemaFlags,
    /// Optional admin display label override. `None` means use the
    /// humanised column name (`"full_name"` → `"Full name"`).
    pub admin_label: Option<&'static str>,
    /// Optional admin form widget hint (`"textarea"`, `"email"`,
    /// `"tel"`, …). `None` means use the default for the
    /// `RustType` — `<input type="text">` for `String`,
    /// `<input type="number">` for `I64`, etc.
    pub admin_widget: Option<&'static str>,
}

impl ModelColumn {
    /// Minimal column constructor — name, SQL DDL, Rust type. All
    /// flags default to `false` / `None`. Builder-style setters
    /// below opt into the rest.
    ///
    /// `const fn` so the macro in commit 2 can use this directly
    /// inside `static SCHEMA: ModelSchema = ...` initialisers,
    /// and so external callers (tests, the future
    /// `examples/freelance/` crate) can construct columns
    /// despite the `#[non_exhaustive]` attribute.
    ///
    /// ```
    /// # use rustio_core::contract::{ModelColumn, RustType};
    /// const ID: ModelColumn =
    ///     ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64)
    ///         .primary_key();
    /// ```
    pub const fn new(
        name: &'static str,
        sql_decl: &'static str,
        rust_type: RustType,
    ) -> Self {
        Self {
            name,
            sql_decl,
            rust_type,
            nullable: false,
            primary_key: false,
            flags: SchemaFlags::empty(),
            admin_label: None,
            admin_widget: None,
        }
    }

    /// Mark this column as nullable (the Rust field is `Option<T>`).
    /// The validator will require PG to allow NULL on the column.
    pub const fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }

    /// Mark this column as the table's primary key. Exactly one
    /// column per `ModelSchema` should set this. The validator
    /// checks the invariant in commit 3.
    pub const fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self
    }

    /// Replace the column's flags wholesale. Pair with
    /// `SchemaFlags::searchable()` / `SchemaFlags::empty()` /
    /// a struct literal (within the crate).
    pub const fn with_flags(mut self, flags: SchemaFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Override the admin display label. `None` (the default) means
    /// the chrome humanises the column name (`full_name` →
    /// `Full name`).
    pub const fn with_label(mut self, label: &'static str) -> Self {
        self.admin_label = Some(label);
        self
    }

    /// Override the admin form widget. Useful for upgrading a
    /// `String` column to `<textarea>` or to a typed input
    /// (`"email"`, `"tel"`).
    pub const fn with_widget(mut self, widget: &'static str) -> Self {
        self.admin_widget = Some(widget);
        self
    }
}

// ---------------------------------------------------------------------------
// ModelSchema
// ---------------------------------------------------------------------------

/// The full schema contract for one model.
///
/// Generated by `#[derive(RustioModel)]` in commit 2 and exposed via
/// `rustio_core::contract::HasSchema` (also commit 2). The validator
/// in commit 3 consumes a `&'static ModelSchema` and produces a
/// `SchemaReport`.
///
/// Lives entirely in static memory. Cloning copies the slice fat
/// pointer, not the contents.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelSchema {
    /// SQL table name. Conventionally plural snake_case
    /// (`projects`, `invoices`).
    pub table: &'static str,
    /// All columns in declaration order. Validator uses this both
    /// for forward checks (every Rust column must be in PG) and
    /// reverse checks (every PG column not in Rust → warning).
    pub columns: &'static [ModelColumn],
    /// Name of the primary-key column. Conventionally `"id"`. Must
    /// match exactly one entry in `columns` with `primary_key: true`.
    pub primary_key: &'static str,
    /// Meili index name when the model is searchable, `None` when
    /// it isn't. Conventionally equal to `table`.
    pub search_index: Option<&'static str>,
}

impl ModelSchema {
    /// Construct a schema from its required parts. `search_index`
    /// defaults to `None`; opt in via [`Self::with_search_index`].
    ///
    /// `const fn` so the macro can emit
    /// `static SCHEMA: ModelSchema = ModelSchema::new(...)` directly,
    /// and so external code (tests, examples) can construct schemas
    /// despite the `#[non_exhaustive]` attribute.
    ///
    /// ```
    /// # use rustio_core::contract::{ModelColumn, ModelSchema, RustType};
    /// static COLS: &[ModelColumn] = &[
    ///     ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64).primary_key(),
    /// ];
    /// const POSTS: ModelSchema = ModelSchema::new("posts", COLS, "id");
    /// ```
    pub const fn new(
        table: &'static str,
        columns: &'static [ModelColumn],
        primary_key: &'static str,
    ) -> Self {
        Self {
            table,
            columns,
            primary_key,
            search_index: None,
        }
    }

    /// Set the Meili index name. Conventionally equal to `table`.
    /// Setting this signals the model is searchable; the search
    /// pipeline in commit 6 keys off this field.
    pub const fn with_search_index(mut self, index: &'static str) -> Self {
        self.search_index = Some(index);
        self
    }

    /// Find a column by name. `O(n)` linear scan — column counts
    /// are small (typically < 20 per model) and this is not on a
    /// hot path.
    pub fn column(&self, name: &str) -> Option<&ModelColumn> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// All columns flagged as searchable. The macro layer in
    /// commit 2 guarantees these are all `RustType::String`.
    pub fn searchable_columns(&self) -> impl Iterator<Item = &ModelColumn> {
        self.columns.iter().filter(|c| c.flags.searchable)
    }
}

// ---------------------------------------------------------------------------
// Tests — Type Rule enforcement
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Type Rule #1 — IDs ----------------------------------------------

    /// Type Rule #1: `i64` is the canonical ID type. The validator
    /// must accept it for `BIGINT` / `BIGSERIAL` columns (and the
    /// long-form synonyms). `bigserial` is an auto-incrementing
    /// `bigint` in PG; the same Rust type covers both.
    #[test]
    fn i64_is_compatible_with_bigint_and_bigserial() {
        for pg in &["bigint", "BIGINT", "int8", "INT8", "bigserial", "BIGSERIAL", "serial8"] {
            assert!(
                RustType::I64.is_compatible_with(pg),
                "i64 must be compatible with `{pg}` (Type Rule #1)"
            );
        }
    }

    /// Type Rule #1 (negative): `i32` MUST NOT satisfy a BIGINT /
    /// BIGSERIAL column. Using `i32` for an ID is the most common
    /// instance of this rule being violated; the validator will
    /// reject it. `i32` remains valid for *non-id* `INTEGER`
    /// columns — this test asserts the asymmetry directly.
    #[test]
    fn i32_does_not_satisfy_id_constraint() {
        for pg in &["bigint", "int8", "bigserial", "serial8"] {
            assert!(
                !RustType::I32.is_compatible_with(pg),
                "i32 must NOT match `{pg}` — using i32 for IDs violates Type Rule #1"
            );
        }
        // Sanity: i32 still works for plain INTEGER columns.
        assert!(RustType::I32.is_compatible_with("integer"));
        assert!(RustType::I32.is_compatible_with("int4"));
    }

    // ----- Type Rule #2 — Timestamps ---------------------------------------

    /// Type Rule #2: `DateTime<Utc>` ↔ `TIMESTAMPTZ`. Both the long
    /// form (`information_schema` exposes `"timestamp with time
    /// zone"`) and the short `udt_name` form (`timestamptz`) must
    /// match. Mixed case is normalised.
    #[test]
    fn datetime_utc_is_compatible_with_timestamptz() {
        for pg in &[
            "timestamp with time zone",
            "TIMESTAMP WITH TIME ZONE",
            "Timestamp With Time Zone",
            "timestamptz",
            "TIMESTAMPTZ",
        ] {
            assert!(
                RustType::DateTimeUtc.is_compatible_with(pg),
                "DateTime<Utc> must be compatible with `{pg}` (Type Rule #2)"
            );
        }
    }

    /// Type Rule #2 (negative): naive `timestamp` / `timestamp
    /// without time zone` MUST NOT satisfy `DateTime<Utc>`. Letting
    /// this through would let projects accidentally store local-
    /// time-as-UTC and silently drift across deploys.
    #[test]
    fn datetime_utc_rejects_naive_timestamp() {
        for pg in &["timestamp", "timestamp without time zone", "timestamp(6)"] {
            assert!(
                !RustType::DateTimeUtc.is_compatible_with(pg),
                "DateTime<Utc> must NOT match `{pg}` — naive timestamps violate Type Rule #2"
            );
        }
    }

    // ----- Type Rule #3 — Money --------------------------------------------

    /// Type Rule #3: `Decimal` ↔ `NUMERIC` / `DECIMAL`. The arbitrary-
    /// precision rust_decimal type is the canonical money carrier.
    #[test]
    fn decimal_is_compatible_with_numeric() {
        for pg in &["numeric", "NUMERIC", "decimal", "DECIMAL"] {
            assert!(
                RustType::Decimal.is_compatible_with(pg),
                "Decimal must be compatible with `{pg}` (Type Rule #3)"
            );
        }
    }

    /// Type Rule #3 (negative): `f64` MUST NOT satisfy a `NUMERIC`
    /// column. Money is the canonical case — using `f64` for cents
    /// loses precision (15 significant decimal digits is fine until
    /// you hit `$1,234,567.89` × `1.05`). The asymmetry is
    /// deliberate: F64 is a real Rust type for non-money decimals
    /// (percentages, scientific), but it's *never* compatible with
    /// the SQL types money goes in.
    #[test]
    fn f64_is_not_valid_for_money_columns() {
        for pg in &["numeric", "decimal", "numeric(12,2)"] {
            assert!(
                !RustType::F64.is_compatible_with(pg),
                "f64 must NOT match `{pg}` — using f64 for money violates Type Rule #3"
            );
        }
        // Sanity: f64 still works for `double precision`.
        assert!(RustType::F64.is_compatible_with("double precision"));
        assert!(RustType::F64.is_compatible_with("float8"));
    }

    /// Type Rule #3 cross-check: `Decimal` MUST NOT satisfy
    /// `double precision` — the validator should refuse to treat a
    /// `f64`-shaped PG column as a money column even if a Decimal
    /// Rust field is declared over it. The asymmetry runs both ways.
    #[test]
    fn decimal_rejects_double_precision_columns() {
        for pg in &["double precision", "float8", "real", "float4"] {
            assert!(
                !RustType::Decimal.is_compatible_with(pg),
                "Decimal must NOT match `{pg}` — money lives in NUMERIC, not floating point"
            );
        }
    }

    // ----- Type Rule #4 — JSON ---------------------------------------------

    /// Type Rule #4: `serde_json::Value` ↔ `JSONB` only. Plain `json`
    /// is not compatible.
    #[test]
    fn json_value_is_compatible_with_jsonb_only() {
        assert!(RustType::JsonValue.is_compatible_with("jsonb"));
        assert!(RustType::JsonValue.is_compatible_with("JSONB"));
        // Type Rule #4 (negative): plain JSON is not the binary
        // form. The framework standardises on JSONB for indexing
        // and query performance.
        assert!(
            !RustType::JsonValue.is_compatible_with("json"),
            "JsonValue must NOT match plain `json` — Type Rule #4 requires JSONB"
        );
    }

    // ----- Type Rule #5 — Strings ------------------------------------------

    /// Type Rule #5: `String` ↔ `TEXT` (default), with `VARCHAR(n)`
    /// / `character varying` accepted as equivalents. The rule
    /// prefers `TEXT`; a `String` field with `sql = "VARCHAR(...)"`
    /// is structurally valid but the macro layer will warn.
    #[test]
    fn string_is_compatible_with_text_and_varchar() {
        for pg in &[
            "text",
            "TEXT",
            "varchar",
            "VARCHAR",
            "character varying",
            "Character Varying",
        ] {
            assert!(
                RustType::String.is_compatible_with(pg),
                "String must be compatible with `{pg}` (Type Rule #5)"
            );
        }
    }

    // ----- Compatibility list invariants -----------------------------------

    /// All entries in every `pg_compatible` list must be lowercase.
    /// `is_compatible_with` lowercases the input before comparing;
    /// the lists themselves must hold the canonical lowercase form
    /// or the comparison silently fails on any uppercase needle.
    #[test]
    fn pg_compatible_lists_are_lowercase() {
        for variant in [
            RustType::I32,
            RustType::I64,
            RustType::F64,
            RustType::Bool,
            RustType::String,
            RustType::DateTimeUtc,
            RustType::Decimal,
            RustType::JsonValue,
            RustType::Uuid,
        ] {
            for pg in variant.pg_compatible() {
                assert_eq!(
                    pg.to_lowercase().as_str(),
                    *pg,
                    "pg_compatible entry `{pg}` for {variant:?} must be lowercase"
                );
            }
        }
    }

    /// `Decimal` is the only variant that includes `numeric` /
    /// `decimal` in its compatibility list. This is the structural
    /// invariant that makes Type Rule #3 enforceable — a search
    /// across all variants confirms exactly one carrier for money.
    #[test]
    fn only_decimal_maps_to_numeric() {
        let mut numeric_carriers: Vec<RustType> = Vec::new();
        for variant in [
            RustType::I32,
            RustType::I64,
            RustType::F64,
            RustType::Bool,
            RustType::String,
            RustType::DateTimeUtc,
            RustType::Decimal,
            RustType::JsonValue,
            RustType::Uuid,
        ] {
            if variant.is_compatible_with("numeric") {
                numeric_carriers.push(variant);
            }
        }
        assert_eq!(
            numeric_carriers,
            vec![RustType::Decimal],
            "exactly one RustType (Decimal) may be compatible with `numeric`; \
             a second compatible variant means money columns can drift type"
        );
    }

    /// `I64` is the only variant that includes `bigint` /
    /// `bigserial`. Mirror of the previous test for ID columns —
    /// Type Rule #1's structural invariant.
    #[test]
    fn only_i64_maps_to_bigint() {
        let mut bigint_carriers: Vec<RustType> = Vec::new();
        for variant in [
            RustType::I32,
            RustType::I64,
            RustType::F64,
            RustType::Bool,
            RustType::String,
            RustType::DateTimeUtc,
            RustType::Decimal,
            RustType::JsonValue,
            RustType::Uuid,
        ] {
            if variant.is_compatible_with("bigint") {
                bigint_carriers.push(variant);
            }
        }
        assert_eq!(
            bigint_carriers,
            vec![RustType::I64],
            "exactly one RustType (I64) may be compatible with `bigint`; \
             allowing i32 here would silently violate Type Rule #1"
        );
    }

    // ----- ModelSchema accessors -------------------------------------------

    /// `ModelSchema::column` finds existing columns and returns
    /// `None` for missing names.
    #[test]
    fn model_schema_column_lookup() {
        static COLS: &[ModelColumn] = &[
            ModelColumn {
                name: "id",
                sql_decl: "BIGSERIAL PRIMARY KEY",
                rust_type: RustType::I64,
                nullable: false,
                primary_key: true,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: false,
                    sortable: false,
                    readonly: true,
                },
                admin_label: None,
                admin_widget: None,
            },
            ModelColumn {
                name: "title",
                sql_decl: "TEXT NOT NULL",
                rust_type: RustType::String,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags::searchable(),
                admin_label: None,
                admin_widget: None,
            },
        ];
        let schema = ModelSchema {
            table: "posts",
            columns: COLS,
            primary_key: "id",
            search_index: Some("posts"),
        };
        assert_eq!(schema.column("id").map(|c| c.name), Some("id"));
        assert_eq!(schema.column("title").map(|c| c.name), Some("title"));
        assert!(schema.column("missing").is_none());
    }

    /// `ModelSchema::searchable_columns` filters down to flagged
    /// columns. Used by the search-indexing layer in commit 6.
    #[test]
    fn model_schema_searchable_columns_filter() {
        static COLS: &[ModelColumn] = &[
            ModelColumn {
                name: "id",
                sql_decl: "BIGSERIAL PRIMARY KEY",
                rust_type: RustType::I64,
                nullable: false,
                primary_key: true,
                flags: SchemaFlags {
                    searchable: false,
                    filterable: false,
                    sortable: false,
                    readonly: true,
                },
                admin_label: None,
                admin_widget: None,
            },
            ModelColumn {
                name: "title",
                sql_decl: "TEXT NOT NULL",
                rust_type: RustType::String,
                nullable: false,
                primary_key: false,
                flags: SchemaFlags::searchable(),
                admin_label: None,
                admin_widget: None,
            },
            ModelColumn {
                name: "body",
                sql_decl: "TEXT",
                rust_type: RustType::String,
                nullable: true,
                primary_key: false,
                flags: SchemaFlags::searchable(),
                admin_label: None,
                admin_widget: Some("textarea"),
            },
        ];
        let schema = ModelSchema {
            table: "posts",
            columns: COLS,
            primary_key: "id",
            search_index: Some("posts"),
        };
        let names: Vec<&str> = schema.searchable_columns().map(|c| c.name).collect();
        assert_eq!(names, vec!["title", "body"]);
    }

    /// `SchemaFlags::default()` is all-false — a safe minimum that
    /// makes "I forgot to set any flags" produce an editable, non-
    /// indexed column rather than an accidental search-everything.
    #[test]
    fn schema_flags_default_is_safe_minimum() {
        let f = SchemaFlags::default();
        assert!(!f.searchable);
        assert!(!f.filterable);
        assert!(!f.sortable);
        assert!(!f.readonly);
    }

    /// `SchemaFlags::searchable()` constructor sets only the
    /// searchable bit. Convenience for the common content-column
    /// case ("title", "body", "description").
    #[test]
    fn schema_flags_searchable_constructor() {
        let f = SchemaFlags::searchable();
        assert!(f.searchable);
        assert!(!f.filterable);
        assert!(!f.sortable);
        assert!(!f.readonly);
    }

    /// `is_compatible_with` trims whitespace before comparing — PG
    /// occasionally returns padded strings.
    #[test]
    fn is_compatible_with_trims_whitespace() {
        assert!(RustType::I64.is_compatible_with("  bigint  "));
        assert!(RustType::String.is_compatible_with("\ttext\n"));
    }

    // ----- Constructor + builder tests -------------------------------------

    /// `SchemaFlags::empty()` matches `Default::default()` field-
    /// for-field. The two are intentionally synonymous; `empty()`
    /// is the `const fn` form for use inside `static` initialisers.
    #[test]
    fn schema_flags_empty_matches_default() {
        assert_eq!(SchemaFlags::empty(), SchemaFlags::default());
    }

    /// `ModelColumn::new` builds a minimal column with all flags
    /// off. Verifies every defaulted field individually so a future
    /// addition that forgets to default a new field fails this test.
    #[test]
    fn model_column_new_minimal_construction() {
        let c = ModelColumn::new("title", "TEXT NOT NULL", RustType::String);
        assert_eq!(c.name, "title");
        assert_eq!(c.sql_decl, "TEXT NOT NULL");
        assert_eq!(c.rust_type, RustType::String);
        assert!(!c.nullable);
        assert!(!c.primary_key);
        assert_eq!(c.flags, SchemaFlags::empty());
        assert!(c.admin_label.is_none());
        assert!(c.admin_widget.is_none());
    }

    /// Builder methods chain. Every setter must return `Self`; the
    /// chain must accumulate state (not overwrite).
    #[test]
    fn model_column_builder_chain_accumulates() {
        let c = ModelColumn::new("description", "TEXT", RustType::String)
            .nullable()
            .with_flags(SchemaFlags::searchable())
            .with_label("Description")
            .with_widget("textarea");
        assert!(c.nullable);
        assert!(!c.primary_key);
        assert!(c.flags.searchable);
        assert!(!c.flags.filterable);
        assert_eq!(c.admin_label, Some("Description"));
        assert_eq!(c.admin_widget, Some("textarea"));
    }

    /// A primary-key column composes the same way an `id` field
    /// will be built by the macro in commit 2. Smokes the typical
    /// pattern.
    #[test]
    fn model_column_primary_key_smoke() {
        let id = ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64)
            .primary_key();
        assert!(id.primary_key);
        assert_eq!(id.rust_type, RustType::I64);
    }

    /// `ModelSchema::new` builds a schema with `search_index = None`
    /// by default. Mirrors `ModelColumn::new`'s minimal construction.
    #[test]
    fn model_schema_new_minimal_construction() {
        static COLS: &[ModelColumn] = &[];
        let schema = ModelSchema::new("posts", COLS, "id");
        assert_eq!(schema.table, "posts");
        assert_eq!(schema.primary_key, "id");
        assert!(schema.search_index.is_none());
        assert_eq!(schema.columns.len(), 0);
    }

    /// `ModelSchema::with_search_index` sets the index name. The
    /// search pipeline (commit 6) keys off this field.
    #[test]
    fn model_schema_with_search_index_setter() {
        static COLS: &[ModelColumn] = &[];
        let schema = ModelSchema::new("posts", COLS, "id").with_search_index("posts");
        assert_eq!(schema.search_index, Some("posts"));
    }

    /// Const-context smoke test. The constructors and builders must
    /// compose at compile time so the macro in commit 2 can emit
    /// `static SCHEMA: ModelSchema = ModelSchema::new(...)`. If any
    /// part of the chain stops being `const fn`, this fails to
    /// compile, not at runtime.
    #[test]
    fn const_context_composition_compiles() {
        const COLS: &[ModelColumn] = &[
            ModelColumn::new("id", "BIGSERIAL PRIMARY KEY", RustType::I64)
                .primary_key()
                .with_flags(SchemaFlags::empty()),
            ModelColumn::new("title", "TEXT NOT NULL", RustType::String)
                .with_flags(SchemaFlags::searchable())
                .with_label("Title"),
            ModelColumn::new("body", "TEXT", RustType::String)
                .nullable()
                .with_flags(SchemaFlags::searchable())
                .with_widget("textarea"),
        ];
        const SCHEMA: ModelSchema =
            ModelSchema::new("posts", COLS, "id").with_search_index("posts");

        // Use the constants so dead-code lint doesn't strip them
        // and we still verify a few invariants.
        assert_eq!(SCHEMA.table, "posts");
        assert_eq!(SCHEMA.columns.len(), 3);
        assert_eq!(SCHEMA.search_index, Some("posts"));
        assert!(SCHEMA.column("id").unwrap().primary_key);
        assert!(SCHEMA.column("body").unwrap().nullable);
        let searchable: Vec<&str> = SCHEMA.searchable_columns().map(|c| c.name).collect();
        assert_eq!(searchable, vec!["title", "body"]);
    }
}
