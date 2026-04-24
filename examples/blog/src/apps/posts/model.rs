use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Searchable, Value};
use serde_json::json;

#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub published: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] =
        &["id", "title", "body", "author", "published", "created_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["title", "body", "author", "published", "created_at"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            body: row.get_string("body")?,
            author: row.get_string("author")?,
            published: row.get_bool("published")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.body.clone().into(),
            self.author.clone().into(),
            self.published.into(),
            self.created_at.into(),
        ]
    }
}

impl Searchable for Post {
    const INDEX_NAME: &'static str = "posts";
    const SEARCHABLE_ATTRIBUTES: &'static [&'static str] = &["title", "body", "author"];
    const FILTERABLE_ATTRIBUTES: &'static [&'static str] =
        &["published", "author", "created_at"];
    const SORTABLE_ATTRIBUTES: &'static [&'static str] = &["created_at"];
    const FACETABLE_ATTRIBUTES: &'static [&'static str] = &["published", "author"];

    fn to_search_doc(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "title": self.title,
            "body": self.body,
            "author": self.author,
            "published": self.published,
            // Meilisearch sorts integers natively; use the unix timestamp.
            "created_at": self.created_at.timestamp(),
        })
    }
}
