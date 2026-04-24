//! Full-text search via Meilisearch.
//!
//! Why Meilisearch and not Elasticsearch: Meili is written in Rust,
//! runs in <100MB of RAM for millions of docs, has sub-millisecond
//! queries by default, and the typo tolerance + faceting story is
//! simpler than Elastic's. For a framework that values "small and
//! fast" as a core constraint, it's the right fit.
//!
//! Layout:
//! - `client.rs` — the thin REST client (reqwest-backed).
//! - `indexer.rs` — the async write pipeline: models pushed into a
//!   channel, a background task drains and bulk-indexes.
//! - `traits.rs`  — `Searchable` trait that models implement to opt in.

mod client;
mod indexer;
mod traits;

pub use client::{MeiliClient, SearchHit, SearchOptions, SearchResults};
pub use indexer::{IndexJob, Indexer};
pub use traits::Searchable;
