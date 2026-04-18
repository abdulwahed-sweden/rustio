//! RustIO — a batteries-included web framework for Rust.
//!
//! Typical usage goes through the `rustio` CLI, which scaffolds projects
//! that depend on this crate. See <https://github.com/abdulwahed-sweden/rustio>.
//!
//! The headline modules:
//!
//! - [`http`] — [`Request`], [`Response`], response builders, form/query parsing.
//! - [`router`] — path matching (`:param`) and request dispatch.
//! - [`middleware`] — around-style middleware with [`Next`].
//! - [`context`] — typed per-request storage.
//! - [`error`] — unified [`Error`] enum and safety-net conversion.
//! - [`auth`] — identity in context, `require_auth` / `require_admin` helpers.
//! - [`orm`] — SQLite-backed [`Model`] trait with `find` / `all` / `create` / `update` / `delete`.
//! - [`admin`] — auto-generated CRUD UI for structs deriving [`RustioAdmin`].
//! - [`migrations`] — versioned `.sql` files tracked in `rustio_migrations`.
//! - [`server`] — hyper-backed [`Server`] that serves a router.

pub mod admin;
pub mod ai;
pub mod auth;
pub mod context;
pub mod defaults;
pub mod error;
pub mod http;
pub mod middleware;
pub mod migrations;
pub mod orm;
pub mod router;
pub mod schema;
pub mod server;

pub use auth::Identity;
// Re-export the chrono types user models reach for. This lets generated
// and user code write `use rustio_core::{DateTime, Utc};` without adding
// chrono to their own `Cargo.toml`.
pub use chrono::{DateTime, Utc};
pub use context::Context;
pub use error::{resolve, Error};
pub use http::{html, json_raw, status_text, text, FormData, Request, Response};
pub use middleware::Next;
pub use orm::{Db, Model, Row, Value};
pub use router::{Params, Router};
pub use rustio_macros::RustioAdmin;
pub use schema::Schema;
pub use server::Server;
