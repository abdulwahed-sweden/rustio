//! A blog example demonstrating the full v1.0 stack:
//! - Postgres-backed models
//! - Meilisearch full-text indexing of posts
//! - Groups (`editors`) with targeted permissions
//! - CSRF + rate limiting + gzip + security headers
//!
//! Prereqs:
//!     docker run --rm -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:16
//!     docker run --rm -p 7700:7700 getmeili/meilisearch:v1.10
//!
//!     export DATABASE_URL=postgres://postgres:dev@localhost/rustio_dev
//!     createdb rustio_dev
//!     cargo run
//!
//! Then open http://127.0.0.1:8000/admin
//! Default login: admin@example.com / admin

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustio_core::admin::{register_admin_routes, Admin};
use rustio_core::auth::{self, Role};
use rustio_core::middleware::{self, RateLimiter};
use rustio_core::migrations;
use rustio_core::orm::Db;
use rustio_core::router::Router;
use rustio_core::search::{Indexer, MeiliClient};
use rustio_core::server::Server;
use rustio_core::templates::Templates;
use rustio_core::{background, http::Response};

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:dev@localhost/rustio_dev".into());
    let meili_url = std::env::var("MEILI_URL")
        .unwrap_or_else(|_| "http://localhost:7700".into());
    let meili_key = std::env::var("MEILI_MASTER_KEY").ok();

    // Database: 30 connections, 1s acquire timeout, 2048-entry read cache.
    let db = Db::connect(&db_url).await?;
    auth::init_tables(&db).await?;
    // Resolve relative to the crate source tree so `cargo run -p blog`
    // works no matter what the caller's CWD is. Override with
    // `MIGRATIONS_DIR` if you're running a packaged binary.
    let migrations_dir = std::env::var("MIGRATIONS_DIR")
        .unwrap_or_else(|_| format!("{}/migrations", env!("CARGO_MANIFEST_DIR")));
    migrations::apply(&db, &migrations_dir).await?;
    background::spawn_housekeeping(db.clone());

    // Seed a default admin user if the table is empty.
    seed_initial_admin(&db).await?;

    // Search: connect to Meili, build the async indexer, configure the index.
    let meili = Arc::new(MeiliClient::new(&meili_url, meili_key)?);
    if let Err(e) = meili.health().await {
        log::warn!("meilisearch not reachable ({e}) — search features will be unavailable");
    } else {
        use rustio_core::search::Searchable;
        use apps::posts::Post;
        meili
            .configure_index(
                Post::INDEX_NAME,
                Post::SEARCHABLE_ATTRIBUTES,
                Post::FILTERABLE_ATTRIBUTES,
                Post::SORTABLE_ATTRIBUTES,
            )
            .await
            .ok();
    }
    let indexer = Indexer::spawn(meili.clone(), 1024);

    // Templates: pick up overrides from ./templates/ if present.
    let templates = Templates::new(Some("templates".into()))?;

    // Admin — register models with search wired in, then materialise their permissions.
    let admin = Admin::new().model_with_search::<apps::posts::Post>(indexer.clone());
    admin.seed_permissions(&db).await?;

    // Create an "editors" group as a convenience on first boot.
    seed_editors_group(&db).await?;

    // Build the router with the standard middleware stack. Order
    // matters: rate-limit first so an attacker can't get expensive
    // downstream middleware to run for free.
    let rate_limiter = RateLimiter::new(240, Duration::from_secs(60));
    let router = Router::new()
        .middleware(middleware::rate_limit(rate_limiter))
        .middleware(middleware::logger)
        .middleware(middleware::security_headers)
        .middleware(middleware::gzip)
        .middleware(middleware::csrf_protect);

    // Home + search.
    let db_for_search = db.clone();
    let templates_for_search = templates.clone();
    let router = router
        .get("/", |_req| async move { Ok(Response::redirect("/admin")) })
        .get("/search", move |req| {
            let db = db_for_search.clone();
            let meili = meili.clone();
            let templates = templates_for_search.clone();
            async move {
                if req.query().get("format") == Some("json") {
                    apps::posts::search_json(&db, &meili, req).await
                } else {
                    apps::posts::search_html(&meili, &templates, req).await
                }
            }
        });

    // Admin.
    let router = register_admin_routes(router, admin, db.clone(), templates);

    let addr: SocketAddr = "127.0.0.1:8000".parse()?;
    Server::new(router, addr).run().await?;
    Ok(())
}

async fn seed_initial_admin(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    if auth::find_user_by_email(db, "admin@example.com").await?.is_none() {
        auth::create_user(db, "admin@example.com", "admin", Role::Admin).await?;
        log::info!("seeded default admin: admin@example.com / admin");
    }
    Ok(())
}

async fn seed_editors_group(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    let existing = sqlx::query_scalar::<_, i64>("SELECT id FROM rustio_groups WHERE name = $1")
        .bind("editors")
        .fetch_optional(db.pool())
        .await?;
    if existing.is_some() {
        return Ok(());
    }
    let gid = auth::create_group(db, "editors", "Can create and edit posts").await?;
    for perm in ["posts.add_post", "posts.change_post", "posts.view_post"] {
        auth::grant_to_group(db, gid, perm).await?;
    }
    log::info!("seeded editors group with post permissions");
    Ok(())
}
