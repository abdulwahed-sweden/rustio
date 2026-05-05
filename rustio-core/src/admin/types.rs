//! The admin's data vocabulary. Kept separate from rendering and
//! handlers so changes here ripple out predictably.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::Result;
use crate::http::FormData;
use crate::orm::{Db, Value};

type CreateResult<'a> =
    Pin<Box<dyn Future<Output = Result<std::result::Result<i64, Vec<String>>>> + Send + 'a>>;

type UpdateResult<'a> =
    Pin<Box<dyn Future<Output = Result<std::result::Result<(), Vec<String>>>> + Send + 'a>>;

// ---------------------------------------------------------------------------
// Phase 10/c — User profile extension API
// ---------------------------------------------------------------------------

/// One labeled section rendered in the project-extension area of the
/// built-in user profile page (admin/user_view.html — `{% block
/// project_user_fields %}`). A project's extension closure returns
/// `Vec<UserProfileSection>` so it can contribute multiple disjoint
/// areas (e.g. "Halal certification" + "Restaurant assignments") in
/// a single registration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserProfileSection {
    pub label: String,
    pub rows: Vec<UserProfileRow>,
}

/// One key-value row inside a [`UserProfileSection`]. Both fields are
/// `String` so projects can format whatever shape they need (numbers,
/// dates, comma-joined lists). Rendered escaped — pass plain text;
/// for arbitrary HTML, projects override the template block instead.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UserProfileRow {
    pub label: String,
    pub value: String,
}

/// The boxed-closure shape stored on `Admin`. `pub(crate)` because
/// projects use the generic [`Admin::user_profile_extension`] builder
/// method and never have to name this directly.
pub(crate) type UserProfileExtensionFn = Arc<
    dyn Fn(Db, crate::auth::UserProfile) -> UserProfileExtensionFuture
        + Send
        + Sync
        + 'static,
>;

pub(crate) type UserProfileExtensionFuture =
    Pin<Box<dyn Future<Output = Result<Vec<UserProfileSection>>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldType {
    I32,
    I64,
    Bool,
    String,
    DateTime,
    OptionalI64,
    OptionalString,
    /// Phase 2: a nullable timestamp. Required so the schema exporter can
    /// faithfully describe fields like `published_at: Option<DateTime<Utc>>`
    /// that OLD's API expressed as `(DateTime, nullable=true)`.
    OptionalDateTime,
}

impl FieldType {
    pub fn widget(&self) -> &'static str {
        match self {
            FieldType::Bool => "checkbox",
            FieldType::DateTime | FieldType::OptionalDateTime => "datetime",
            FieldType::I32 | FieldType::I64 | FieldType::OptionalI64 => "number",
            FieldType::String | FieldType::OptionalString => "text",
        }
    }

    pub fn nullable(&self) -> bool {
        matches!(
            self,
            FieldType::OptionalI64 | FieldType::OptionalString | FieldType::OptionalDateTime
        )
    }
}

#[derive(Debug, Clone)]
pub struct AdminField {
    pub name: &'static str,
    pub label: &'static str,
    pub field_type: FieldType,
    pub editable: bool,
    pub relation: Option<AdminRelation>,
    /// Phase 5/d — closed list of allowed string values for this
    /// field. When `Some`, the form layer renders a `<select>` with
    /// one option per entry. The values double as labels (raw, not
    /// humanised) per the "no invented content" rule. Hand-populated
    /// for now; a future macro pass will accept
    /// `#[rustio(choices = […])]` to derive this automatically.
    pub choices: Option<&'static [&'static str]>,
}

#[derive(Debug, Clone)]
pub struct AdminRelation {
    pub target_model: &'static str,
    pub display_field: Option<&'static str>,
    /// Phase 5/d — `true` for many-to-many relations (form renders
    /// `<select multiple>`), `false` for the default belongs-to
    /// (single `<select>`). Macro emits `false`; consumers that want
    /// M2M behaviour must hand-set this until the macro learns a
    /// `#[rustio(many_to_many)]` attribute.
    pub multi: bool,
}

/// What the `#[derive(RustioAdmin)]` macro produces for each struct.
pub trait AdminModel: Send + Sync + 'static {
    const ADMIN_NAME: &'static str;
    const DISPLAY_NAME: &'static str;
    const SINGULAR_NAME: &'static str;
    const FIELDS: &'static [AdminField];

    /// Render one row for the list page (column → display string).
    fn display_values(&self) -> Vec<(String, String)>;

    /// Populate a new instance from an HTTP form. Returns a list of
    /// validation errors if anything was wrong.
    fn from_form(form: &FormData) -> std::result::Result<Self, Vec<String>>
    where
        Self: Sized;

    /// A stable label for one instance (used on the delete confirm page).
    fn object_label(&self) -> String;

    fn id(&self) -> i64;

    fn values_to_update(&self) -> Vec<(&'static str, Value)>;
}

/// Runtime metadata about one admin-registered model.
pub struct AdminEntry {
    pub admin_name: &'static str,
    pub display_name: &'static str,
    pub singular_name: &'static str,
    /// SQL table name. For user-registered models this is `<M as Model>::TABLE`;
    /// for the synthetic core User entry it's `"rustio_users"`.
    pub table: &'static str,
    pub fields: &'static [AdminField],
    /// `true` only for framework-owned entries (currently just `User`).
    /// External tools key off this to refuse destructive plans against
    /// framework infrastructure.
    pub core: bool,
    pub(crate) ops: Arc<dyn AdminOps>,
    pub(crate) search_hook: Option<Arc<dyn SearchHook>>,
}

/// A callback invoked after create/update/delete on a model, to
/// keep an external search index in sync. The router invokes it
/// fire-and-forget so a laggy search backend doesn't block admin writes.
pub(crate) trait SearchHook: Send + Sync {
    fn on_upsert<'a>(
        &'a self,
        db: &'a crate::orm::Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    fn on_delete(&self, id: i64);
}

/// Type-erased CRUD operations. The `Admin::model::<M>()` call captures
/// a concrete `M: AdminModel + Model` and hides it behind this trait so
/// the router can treat every model uniformly.
pub(crate) trait AdminOps: Send + Sync {
    fn list<'a>(
        &'a self,
        db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>>;

    fn find_row<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>>;

    fn create<'a>(&'a self, db: &'a Db, form: &'a FormData) -> CreateResult<'a>;

    fn update<'a>(&'a self, db: &'a Db, id: i64, form: &'a FormData) -> UpdateResult<'a>;

    fn delete<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn object_label<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>>;
}

/// A row as shown on the list page.
#[derive(Debug)]
pub struct ListRow {
    pub id: i64,
    pub cells: Vec<String>,
}

/// The raw field values used to pre-fill the edit form.
#[derive(Debug)]
pub struct EditRow {
    // Exposed on the public API for templates that want to build
    // canonical object URLs; not read by the default renderer.
    #[allow(dead_code)]
    pub id: i64,
    pub values: Vec<(String, String)>,
}

/// Per-project admin branding. Defaults are RustIO-flavoured;
/// projects override via [`Admin::site_branding`].
///
/// - `site_title` — used in `<title>` tags.
/// - `site_header` — header bar text (and the login card's brand).
/// - `index_title` — dashboard h1.
/// - `footer_copyright` — single line at the bottom of every page.
/// - `domain` — DNS-shape string used to mint demo email addresses
///   (`<role>@<domain>`). Phase 7a/0.5/c. Not surfaced in any
///   template — it's strictly a backend identifier for the demo
///   bootstrap flow.
#[derive(Clone, Debug)]
pub struct SiteBranding {
    pub site_title: String,
    pub site_header: String,
    pub index_title: String,
    pub footer_copyright: String,
    pub domain: String,
}

impl Default for SiteBranding {
    fn default() -> Self {
        Self {
            site_title: "RustIO administration".into(),
            site_header: "RustIO administration".into(),
            index_title: "Site administration".into(),
            footer_copyright: format!("RustIO {}", env!("CARGO_PKG_VERSION")),
            domain: "rustio.local".into(),
        }
    }
}

/// 1.8.2 — full admin chrome palette. Each field maps onto one of the
/// framework's `--rio-*` design tokens defined in `admin/base.html`,
/// so overriding these values via `Admin::theme(...)` re-skins the
/// entire admin shell (topbar, sidebar, body, cards, headings, hairlines)
/// without touching CSS or rebuilding Tailwind.
///
/// Defaults match the framework's current chrome so a project that
/// doesn't call `.theme(...)` renders unchanged. Operators typically
/// override 1–3 fields and let the rest default:
///
/// ```ignore
/// admin.theme(AdminTheme {
///     accent:  "#1e6ba8".into(),
///     topbar:  "#1e3a5f".into(),
///     bg:      "#f0f5fa".into(),
///     ..AdminTheme::default()
/// })
/// ```
///
/// Hex form (`#rrggbb` or `rrggbb`); leading `#` is auto-normalised
/// at render time. Malformed values fall back to framework defaults
/// rather than panic — the admin path never breaks over a config typo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminTheme {
    /// Primary brand colour. Drives `--rio-accent` (logo mark, focus
    /// rings, primary buttons, badges) AND the `--ds-color-accent`
    /// CSS variable (for Tailwind utilities that bake the accent
    /// with opacity, like `bg-accent/10`).
    pub accent: String,
    /// Page background. Drives `--rio-bg` — the canvas behind the
    /// shell on every admin page.
    pub bg: String,
    /// Card / topbar / sidebar surface. Drives `--rio-bg-surface-1` —
    /// the colour of the topbar background and any elevated surface.
    pub surface: String,
    /// Body text colour. Drives `--rio-text` — body copy AND the
    /// "skip-to-content" link AND the topbar brand title.
    pub text: String,
    /// Secondary / supporting text. Drives `--rio-text-muted` — the
    /// "signed in as …" line, table hints, and most metadata labels.
    pub text_muted: String,
    /// Border / hairline colour. Drives `--rio-border` — every visible
    /// hairline on cards, the topbar bottom-border, and table dividers.
    pub border: String,
}

impl Default for AdminTheme {
    fn default() -> Self {
        // 1.8.3 — defaults migrated to the Cobalt Blue palette from
        // `docs/design-system.json` themes.light. Framework-default
        // projects (those that don't call Admin::accent_color or
        // Admin::theme) now inherit cobalt automatically — no per-
        // project boilerplate required. Operators that opt into a
        // custom palette via .theme(...) keep their override; this
        // change only shifts the *unset* baseline.
        //
        // Mirror these values exactly with the JSON light theme. If
        // either drifts, `make css-check` will keep input.css in
        // sync but this struct must be hand-edited.
        Self {
            accent:     "#2563EB".into(), // Cobalt Blue (light theme accent)
            bg:         "#F4F6FB".into(), // light theme background
            surface:    "#FFFFFF".into(), // light theme surface
            text:       "#111827".into(), // light theme body text
            text_muted: "#4B5563".into(), // light theme muted text
            border:     "#D1D5DB".into(), // light theme hairlines
        }
    }
}

/// Builder for the admin. Register models with `.model::<M>()`, then
/// hand it to the router via `register_admin_routes`.
pub struct Admin {
    pub(crate) entries: Vec<AdminEntry>,
    pub(crate) site_branding: SiteBranding,
    /// Phase 10/c — optional project-supplied closure that contributes
    /// extra sections to the built-in user profile page. `None` for the
    /// zero-config baseline.
    pub(crate) user_profile_ext: Option<UserProfileExtensionFn>,
    /// 1.8.2 — full admin chrome palette. See `AdminTheme` for the
    /// fields and how each one flows to a `--rio-*` CSS token.
    /// `Admin::accent_color(...)` sets `theme.accent` only and leaves
    /// the rest defaulted; `Admin::theme(...)` replaces the whole
    /// palette at once.
    pub(crate) theme: AdminTheme,
}

impl Default for Admin {
    fn default() -> Self {
        Self::new()
    }
}

impl Admin {
    /// Constructs a new `Admin` with the framework's core entries
    /// pre-seeded. As of Phase 2 the only core entry is `User`, which
    /// the schema exporter must always describe so external tooling
    /// can reason about authentication tables. Project models are
    /// added on top via [`Self::model`] / [`Self::model_with_search`].
    pub fn new() -> Self {
        Self {
            entries: vec![core_user_entry()],
            site_branding: SiteBranding::default(),
            user_profile_ext: None,
            theme: AdminTheme::default(),
        }
    }

    /// Override the default RustIO branding. Project-facing API; the
    /// builder pattern matches `.model()` / `.model_with_search()` so
    /// chains read naturally.
    pub fn site_branding(mut self, branding: SiteBranding) -> Self {
        self.site_branding = branding;
        self
    }

    /// Read-only access to the active branding — handlers and context
    /// builders use this to thread brand strings into templates.
    pub fn branding(&self) -> &SiteBranding {
        &self.site_branding
    }

    /// 1.8.2 — set the admin chrome's accent colour (one-line shortcut
    /// for `theme(AdminTheme { accent: ..., ..Default::default() })`).
    /// Hex form, with or without the leading `#` (`"#1e6ba8"` and
    /// `"1e6ba8"` both work).
    ///
    /// Drives a runtime `<style>` block in `admin/base.html` that
    /// overrides every Tailwind utility baking the framework's default
    /// teal as a literal RGB value (`bg-brand-600`, `text-brand-700`,
    /// link colour, selection colour, focus rings, the `--ds-color-accent`
    /// CSS variable). No Tailwind rebuild required.
    ///
    /// Malformed input (wrong length, non-hex characters) is accepted
    /// silently — the renderer falls back to the framework default at
    /// hex-to-RGB conversion time. This avoids panicking the admin path
    /// over a config typo; future versions may add a `Result`-returning
    /// builder for stricter operators.
    pub fn accent_color(mut self, color: impl Into<String>) -> Self {
        self.theme.accent = normalise_hex(color);
        self
    }

    /// 1.8.2 — set the entire admin chrome palette in one call.
    /// Re-skins the topbar, sidebar, body background, surface cards,
    /// body text, muted text, and hairlines by overriding the
    /// framework's `--rio-*` design tokens at render time. See
    /// [`AdminTheme`] for the field-by-field contract.
    pub fn theme(mut self, theme: AdminTheme) -> Self {
        self.theme = theme;
        self
    }

    /// 1.8.2 — read-only access to the configured accent colour
    /// (`#rrggbb`). Used by `BaseContext` to populate the render
    /// context; projects rarely need to call this directly.
    pub fn accent(&self) -> &str {
        &self.theme.accent
    }

    /// 1.8.2 — read-only access to the active full theme. Used by
    /// `BaseContext` to populate the override `<style>` block.
    pub fn active_theme(&self) -> &AdminTheme {
        &self.theme
    }

    pub fn model<M>(mut self) -> Self
    where
        M: AdminModel + crate::orm::Model,
    {
        let ops: Arc<dyn AdminOps> = Arc::new(ConcreteOps::<M>::new());
        self.entries.push(AdminEntry {
            admin_name: M::ADMIN_NAME,
            display_name: M::DISPLAY_NAME,
            singular_name: M::SINGULAR_NAME,
            table: <M as crate::orm::Model>::TABLE,
            fields: M::FIELDS,
            core: false,
            ops,
            search_hook: None,
        });
        self
    }

    /// Register a model and wire it into an async search indexer.
    /// Every create/update/delete pushes the row into the indexer's
    /// queue — the actual HTTP round-trip to Meilisearch happens in
    /// the background.
    pub fn model_with_search<M>(mut self, indexer: crate::search::Indexer) -> Self
    where
        M: AdminModel + crate::orm::Model + crate::search::Searchable,
    {
        let ops: Arc<dyn AdminOps> = Arc::new(ConcreteOps::<M>::new());
        let hook: Arc<dyn SearchHook> = Arc::new(ConcreteSearchHook::<M>::new(indexer));
        self.entries.push(AdminEntry {
            admin_name: M::ADMIN_NAME,
            display_name: M::DISPLAY_NAME,
            singular_name: M::SINGULAR_NAME,
            table: <M as crate::orm::Model>::TABLE,
            fields: M::FIELDS,
            core: false,
            ops,
            search_hook: Some(hook),
        });
        self
    }

    /// Phase 14, commit 8 — register a model whose admin
    /// behaviour is derived entirely from its `ModelSchema`. No
    /// `AdminModel` impl required.
    ///
    /// The bridge in [`crate::admin::from_schema`] produces an
    /// `AdminEntry` whose:
    /// - field list comes from
    ///   [`crate::admin::from_schema::admin_fields_from_schema`]
    /// - CRUD goes through a generic `SchemaOps` that builds
    ///   SQL from the schema's column list
    /// - admin / display / singular names derive from
    ///   `schema.table` (humanised + naive singular)
    ///
    /// Coexists with [`Self::model`] / [`Self::model_with_search`]
    /// — projects can mix manual `AdminModel` impls with
    /// schema-derived entries on the same `Admin`.
    pub fn from_schema<T>(self) -> Self
    where
        T: crate::contract::HasSchema,
    {
        let entry = crate::admin::from_schema::admin_entry_from_type::<T>();
        self.push_entry(entry)
    }

    /// Phase 14, commit 8 — bulk variant: register every
    /// `ModelSchema` in the slice as a schema-derived entry.
    /// Equivalent to calling [`Self::from_schema`] once per
    /// element, but takes the schemas as values (clone-on-pass)
    /// so a caller can pass `&freelance::all_schemas()` directly.
    pub fn from_schemas(mut self, schemas: &[crate::contract::ModelSchema]) -> Self {
        for schema in schemas {
            let entry = crate::admin::from_schema::admin_entry_from_schema(schema.clone());
            self.entries.push(entry);
        }
        self
    }

    fn push_entry(mut self, entry: AdminEntry) -> Self {
        self.entries.push(entry);
        self
    }

    pub fn entries(&self) -> &[AdminEntry] {
        &self.entries
    }

    /// Phase 10/c — register a project-specific extension that contributes
    /// extra sections to the built-in user profile page. The closure is
    /// invoked on every render of `GET /admin/users/:id` (Overview tab);
    /// it receives the `Db` handle and the loaded [`crate::auth::UserProfile`]
    /// (no `password_hash`) and returns a `Vec<UserProfileSection>`.
    /// Sections render in the order returned, immediately after the core
    /// profile show-grid.
    ///
    /// Zero-config baseline: don't call this method, and the extension area
    /// stays empty. Projects that need richer layout than key-value rows
    /// override the `{% block project_user_fields %}` template block in
    /// `templates/admin/user_view.html` (extending `admin/base.html`).
    ///
    /// ```ignore
    /// let admin = Admin::new()
    ///     .user_profile_extension(|_db, user| Box::pin(async move {
    ///         Ok(vec![rustio_core::admin::UserProfileSection {
    ///             label: "Account".into(),
    ///             rows: vec![rustio_core::admin::UserProfileRow {
    ///                 label: "Display name".into(),
    ///                 value: user.full_name.unwrap_or(user.email),
    ///             }],
    ///         }])
    ///     }));
    /// ```
    pub fn user_profile_extension<F, Fut>(mut self, ext: F) -> Self
    where
        F: Fn(Db, crate::auth::UserProfile) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<UserProfileSection>>> + Send + 'static,
    {
        self.user_profile_ext = Some(Arc::new(move |db, user| Box::pin(ext(db, user))));
        self
    }

    /// Internal accessor — handlers fetch the registered extension
    /// closure (if any) here.
    pub(crate) fn user_profile_ext(&self) -> Option<&UserProfileExtensionFn> {
        self.user_profile_ext.as_ref()
    }

    pub fn find(&self, admin_name: &str) -> Option<&AdminEntry> {
        self.entries.iter().find(|e| e.admin_name == admin_name)
    }

    /// Register the canonical (add/change/delete/view) permissions for
    /// every model. Call during startup after `init_tables`.
    pub async fn seed_permissions(&self, db: &crate::orm::Db) -> crate::error::Result<()> {
        for entry in &self.entries {
            let singular = entry.singular_name.to_ascii_lowercase();
            crate::auth::register_model_permissions(db, entry.admin_name, &singular).await?;
        }
        Ok(())
    }
}

// Concrete implementation of AdminOps for a given M.
struct ConcreteOps<M> {
    _marker: std::marker::PhantomData<M>,
}

impl<M> ConcreteOps<M> {
    fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<M> AdminOps for ConcreteOps<M>
where
    M: AdminModel + crate::orm::Model,
{
    fn list<'a>(
        &'a self,
        db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>> {
        Box::pin(async move {
            let rows = crate::orm::all::<M>(db).await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let id = AdminModel::id(&r);
                    let cells = r.display_values().into_iter().map(|(_, v)| v).collect();
                    ListRow { id, cells }
                })
                .collect())
        })
    }

    fn find_row<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>> {
        Box::pin(async move {
            let found = crate::orm::find::<M>(db, id).await?;
            Ok(found.map(|m| EditRow {
                id: AdminModel::id(&m),
                values: m.display_values(),
            }))
        })
    }

    fn create<'a>(&'a self, db: &'a Db, form: &'a FormData) -> CreateResult<'a> {
        Box::pin(async move {
            match M::from_form(form) {
                Ok(model) => match crate::orm::create(db, &model).await {
                    Ok(id) => Ok(Ok(id)),
                    // Phase 7.6 — `From<sqlx::Error>` (in error.rs)
                    // routes Postgres constraint violations to
                    // `Error::Conflict`. Catch it here so the user
                    // sees a re-rendered form with an inline error
                    // instead of a 500. The string is intentionally
                    // generic — projects that want a per-field
                    // message can validate before submit.
                    Err(crate::error::Error::Conflict(msg)) => {
                        log::warn!("create rejected by DB constraint: {msg}");
                        Ok(Err(vec![
                            "Invalid value or constraint violation. \
                             Please check the highlighted fields and try again."
                                .into(),
                        ]))
                    }
                    Err(other) => Err(other),
                },
                Err(errs) => Ok(Err(errs)),
            }
        })
    }

    fn update<'a>(&'a self, db: &'a Db, id: i64, form: &'a FormData) -> UpdateResult<'a> {
        Box::pin(async move {
            match M::from_form(form) {
                Ok(model) => match crate::orm::update(db, id, &model).await {
                    Ok(()) => Ok(Ok(())),
                    Err(crate::error::Error::Conflict(msg)) => {
                        log::warn!("update rejected by DB constraint: {msg}");
                        Ok(Err(vec![
                            "Invalid value or constraint violation. \
                             Please check the highlighted fields and try again."
                                .into(),
                        ]))
                    }
                    Err(other) => Err(other),
                },
                Err(errs) => Ok(Err(errs)),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move { crate::orm::delete::<M>(db, id).await })
    }

    fn object_label<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>> {
        Box::pin(async move {
            let found = crate::orm::find::<M>(db, id).await?;
            Ok(found.map(|m| m.object_label()))
        })
    }
}

// ---- SearchHook impl -----------------------------------------------------

struct ConcreteSearchHook<M> {
    indexer: crate::search::Indexer,
    _marker: std::marker::PhantomData<M>,
}

impl<M> ConcreteSearchHook<M> {
    fn new(indexer: crate::search::Indexer) -> Self {
        Self {
            indexer,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<M> SearchHook for ConcreteSearchHook<M>
where
    M: AdminModel + crate::orm::Model + crate::search::Searchable,
{
    fn on_upsert<'a>(
        &'a self,
        db: &'a Db,
        id: i64,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let indexer = self.indexer.clone();
        Box::pin(async move {
            match crate::orm::find::<M>(db, id).await {
                Ok(Some(model)) => {
                    indexer.queue_detached(crate::search::IndexJob::Upsert {
                        index: M::INDEX_NAME.to_string(),
                        primary_key: M::PRIMARY_KEY.to_string(),
                        document: model.to_search_doc(),
                    });
                }
                Ok(None) => {}
                Err(e) => log::warn!("search hook: could not reload {}:{id}: {e}", M::INDEX_NAME),
            }
        })
    }

    fn on_delete(&self, id: i64) {
        self.indexer.queue_detached(crate::search::IndexJob::Delete {
            index: M::INDEX_NAME.to_string(),
            id: id.to_string(),
        });
    }
}

// -------------------------------------------------------------------------
// Core User entry — synthetic, schema-only
// -------------------------------------------------------------------------
//
// Every project's exported schema must describe the auth tables so
// external tooling (planner, dashboards) can reason about them. The
// `User` entry is built directly here rather than implementing
// `AdminModel` on a placeholder struct: the auth subsystem already
// owns the live `/admin/users` page with its own logic, so we don't
// want a second route to spawn.
//
// Field order in `CORE_USER_FIELDS` matches the `rustio_users`
// migration (id, email, password_hash, role, is_active, created_at).
// `Schema::from_admin` re-sorts alphabetically before exporting.

const CORE_USER_FIELDS: &[AdminField] = &[
    AdminField {
        name: "id",
        label: "id",
        field_type: FieldType::I64,
        editable: false,
        relation: None,
        choices: None,
    },
    AdminField {
        name: "email",
        label: "email",
        field_type: FieldType::String,
        editable: true,
        relation: None,
        choices: None,
    },
    AdminField {
        name: "password_hash",
        label: "password_hash",
        field_type: FieldType::String,
        editable: false,
        relation: None,
        choices: None,
    },
    AdminField {
        name: "role",
        label: "role",
        field_type: FieldType::String,
        editable: true,
        relation: None,
        choices: None,
    },
    AdminField {
        name: "is_active",
        label: "is_active",
        field_type: FieldType::Bool,
        editable: true,
        relation: None,
        choices: None,
    },
    AdminField {
        name: "created_at",
        label: "created_at",
        field_type: FieldType::DateTime,
        editable: false,
        relation: None,
        choices: None,
    },
];

/// 1.8.2 — normalise a user-supplied colour string to `#rrggbb` form.
/// Accepts both `"#1e6ba8"` and `"1e6ba8"`; trims whitespace; does NOT
/// validate that the body is hex (that's the renderer's job, where
/// invalid values fall back to the framework default rather than
/// panic). The `format!()` adds back exactly one leading `#`.
pub(crate) fn normalise_hex(input: impl Into<String>) -> String {
    let raw = input.into();
    let trimmed = raw.trim().trim_start_matches('#');
    format!("#{trimmed}")
}

fn core_user_entry() -> AdminEntry {
    AdminEntry {
        admin_name: "users",
        display_name: "Users",
        singular_name: "User",
        table: "rustio_users",
        fields: CORE_USER_FIELDS,
        core: true,
        ops: Arc::new(CoreUserOps),
        search_hook: None,
    }
}

/// Schema-only ops stub for the synthetic User entry. The live
/// `/admin/users` page is wired separately by `admin::builtin`, so
/// every method here returns a dedicated error rather than silently
/// half-working. If the generic admin ever routes to this, the error
/// makes the misuse obvious.
struct CoreUserOps;

fn core_user_route_error() -> crate::error::Error {
    crate::error::Error::Internal(
        "the core User entry is schema-only — use the dedicated /admin/users page".into(),
    )
}

impl AdminOps for CoreUserOps {
    fn list<'a>(
        &'a self,
        _db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>> {
        Box::pin(async { Err(core_user_route_error()) })
    }

    fn find_row<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>> {
        Box::pin(async { Err(core_user_route_error()) })
    }

    fn create<'a>(&'a self, _db: &'a Db, _form: &'a FormData) -> CreateResult<'a> {
        Box::pin(async { Err(core_user_route_error()) })
    }

    fn update<'a>(&'a self, _db: &'a Db, _id: i64, _form: &'a FormData) -> UpdateResult<'a> {
        Box::pin(async { Err(core_user_route_error()) })
    }

    fn delete<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async { Err(core_user_route_error()) })
    }

    fn object_label<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>> {
        Box::pin(async { Err(core_user_route_error()) })
    }
}

#[cfg(test)]
impl AdminEntry {
    /// Build an `AdminEntry` for test fixtures. Fills `ops` with a
    /// `PanicOps` stub and `search_hook` with `None`. Any test that
    /// ends up routing CRUD through the returned entry will panic
    /// loudly at the trait method — the stub is there only to
    /// satisfy the `pub(crate)` fields on `AdminEntry`, not to
    /// stand in for a real model.
    pub(crate) fn for_testing(
        admin_name: &'static str,
        display_name: &'static str,
        singular_name: &'static str,
        table: &'static str,
        fields: &'static [AdminField],
        core: bool,
    ) -> Self {
        Self {
            admin_name,
            display_name,
            singular_name,
            table,
            fields,
            core,
            ops: Arc::new(PanicOps),
            search_hook: None,
        }
    }

    /// Phase 7.6 — variant of `for_testing` whose `ops.list()` returns
    /// an `Err`. Lets tests exercise the resilience path in
    /// `render::search_options` without spinning up Postgres.
    #[cfg(test)]
    pub(crate) fn for_testing_failing_list(
        admin_name: &'static str,
        display_name: &'static str,
        singular_name: &'static str,
        table: &'static str,
        fields: &'static [AdminField],
    ) -> Self {
        Self {
            admin_name,
            display_name,
            singular_name,
            table,
            fields,
            core: false,
            ops: Arc::new(FailingOps),
            search_hook: None,
        }
    }
}

#[cfg(test)]
struct PanicOps;

#[cfg(test)]
const PANIC_MSG: &str =
    "PanicOps is test-only; if you hit this, a test is using AdminEntry for CRUD, which is wrong — use a real Model";

#[cfg(test)]
impl AdminOps for PanicOps {
    fn list<'a>(
        &'a self,
        _db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }

    fn find_row<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }

    fn create<'a>(&'a self, _db: &'a Db, _form: &'a FormData) -> CreateResult<'a> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }

    fn update<'a>(&'a self, _db: &'a Db, _id: i64, _form: &'a FormData) -> UpdateResult<'a> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }

    fn delete<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }

    fn object_label<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>> {
        Box::pin(async { unreachable!("{PANIC_MSG}") })
    }
}

/// Phase 7.6 — test-only AdminOps whose `list()` returns a synthetic
/// DB-shaped error. Used to exercise `search_options`'s catch-and-
/// log-and-return-empty path; other methods stay unreachable since
/// search only calls `list`.
#[cfg(test)]
struct FailingOps;

#[cfg(test)]
impl AdminOps for FailingOps {
    fn list<'a>(
        &'a self,
        _db: &'a Db,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ListRow>>> + Send + 'a>> {
        Box::pin(async { Err(crate::error::Error::Internal("simulated db failure".into())) })
    }

    fn find_row<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EditRow>>> + Send + 'a>> {
        Box::pin(async { unreachable!("FailingOps only exercises list()") })
    }

    fn create<'a>(&'a self, _db: &'a Db, _form: &'a FormData) -> CreateResult<'a> {
        Box::pin(async { unreachable!("FailingOps only exercises list()") })
    }

    fn update<'a>(&'a self, _db: &'a Db, _id: i64, _form: &'a FormData) -> UpdateResult<'a> {
        Box::pin(async { unreachable!("FailingOps only exercises list()") })
    }

    fn delete<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async { unreachable!("FailingOps only exercises list()") })
    }

    fn object_label<'a>(
        &'a self,
        _db: &'a Db,
        _id: i64,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>> {
        Box::pin(async { unreachable!("FailingOps only exercises list()") })
    }
}
