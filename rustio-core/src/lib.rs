//! RustIO — a production-grade, strict-by-construction web framework
//! for Rust.
//!
//! Write a model struct, derive `RustioAdmin`, and the framework
//! provides the admin UI, HTTP/2 server, Postgres ORM, migrations,
//! full-text search (Meilisearch), sessions, and granular RBAC.

// Phase 7.3 — admin render-test fixtures hand-build large
// `serde_json::json!` literals (FormField has ~16 fields × multiple
// fields per fixture). The default recursion limit (128) is too low
// for those macro expansions; 256 is the conventional bump.
#![recursion_limit = "256"]

pub mod admin;
pub mod ai;
pub mod ai_gen;
pub mod auth;
pub mod background;
pub mod cache;
pub mod error;
pub mod http;
pub mod middleware;
pub mod migrations;
pub mod orm;
pub mod router;
pub mod schema;
pub mod search;
pub mod server;
pub mod templates;

// Common vocabulary at the crate root.
pub use crate::admin::{Admin, AdminField, AdminModel, FieldType};
pub use crate::auth::{Identity, Role};
pub use crate::error::{Error, Result};
pub use crate::http::{FormData, Request, Response};
pub use crate::orm::{Db, DbOptions, Model, Row, Value};
pub use crate::router::{Next, Router};
pub use crate::search::{Indexer, MeiliClient, Searchable};
pub use crate::server::Server;

pub use rustio_macros::RustioAdmin;

// `RustioAdmin` emits `::rustio_core::*` paths in its expansion. That
// resolves cleanly for downstream consumers, but inside this crate's
// own compilation unit `rustio_core` isn't a known extern. Aliasing
// the crate to itself under `cfg(test)` lets the macro be exercised by
// `admin::macro_tests` without changing any non-test build.
#[cfg(test)]
extern crate self as rustio_core;
