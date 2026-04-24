//! The `Searchable` trait. Models implement this to opt into the
//! search index. The trait is deliberately tiny: we want to serialise
//! into a Meilisearch document, name the index, and declare which
//! fields are filterable/sortable.

use serde_json::Value;

pub trait Searchable {
    /// Index name — typically the same as the admin name ("posts").
    const INDEX_NAME: &'static str;

    /// Primary key field in the serialised document. Meilisearch
    /// requires one unique key per doc; for us it's almost always "id".
    const PRIMARY_KEY: &'static str = "id";

    /// Which attributes the engine should actually tokenise for
    /// full-text queries. Fewer is faster; only include what users
    /// would actually type to find this model.
    const SEARCHABLE_ATTRIBUTES: &'static [&'static str];

    /// Fields available for `filter=…` queries (e.g. `published=true`).
    const FILTERABLE_ATTRIBUTES: &'static [&'static str] = &[];

    /// Fields available for `sort=…`.
    const SORTABLE_ATTRIBUTES: &'static [&'static str] = &[];

    /// Subset of filterable attributes to request facet counts for.
    /// A UI with a "by author" sidebar wants author here; a raw
    /// `created_at` timestamp that's filterable-but-not-faceted does not.
    const FACETABLE_ATTRIBUTES: &'static [&'static str] = &[];

    /// Serialise `self` into the JSON document shape Meili indexes.
    fn to_search_doc(&self) -> Value;
}
