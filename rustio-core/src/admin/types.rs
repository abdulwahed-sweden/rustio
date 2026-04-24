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
}

impl FieldType {
    pub fn widget(&self) -> &'static str {
        match self {
            FieldType::Bool => "checkbox",
            FieldType::DateTime => "datetime",
            FieldType::I32 | FieldType::I64 | FieldType::OptionalI64 => "number",
            FieldType::String | FieldType::OptionalString => "text",
        }
    }

    pub fn nullable(&self) -> bool {
        matches!(self, FieldType::OptionalI64 | FieldType::OptionalString)
    }
}

#[derive(Debug, Clone)]
pub struct AdminField {
    pub name: &'static str,
    pub label: &'static str,
    pub field_type: FieldType,
    pub editable: bool,
    pub relation: Option<AdminRelation>,
}

#[derive(Debug, Clone)]
pub struct AdminRelation {
    pub target_model: &'static str,
    pub display_field: Option<&'static str>,
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
    pub fields: &'static [AdminField],
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

/// Builder for the admin. Register models with `.model::<M>()`, then
/// hand it to the router via `register_admin_routes`.
pub struct Admin {
    pub(crate) entries: Vec<AdminEntry>,
}

impl Default for Admin {
    fn default() -> Self {
        Self::new()
    }
}

impl Admin {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
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
            fields: M::FIELDS,
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
            fields: M::FIELDS,
            ops,
            search_hook: Some(hook),
        });
        self
    }

    pub fn entries(&self) -> &[AdminEntry] {
        &self.entries
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
                Ok(model) => {
                    let id = crate::orm::create(db, &model).await?;
                    Ok(Ok(id))
                }
                Err(errs) => Ok(Err(errs)),
            }
        })
    }

    fn update<'a>(&'a self, db: &'a Db, id: i64, form: &'a FormData) -> UpdateResult<'a> {
        Box::pin(async move {
            match M::from_form(form) {
                Ok(model) => {
                    crate::orm::update(db, id, &model).await?;
                    Ok(Ok(()))
                }
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
