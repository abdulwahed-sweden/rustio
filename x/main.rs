use rustio_core::auth::authenticate;
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Router, Schema, Server};

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `rustio schema` invokes this binary with --dump-schema. We emit
    // rustio.schema.json from the in-memory admin registry and exit
    // before doing any I/O — no DB connect, no bound port.
    if std::env::args().any(|a| a == "--dump-schema") {
        let admin = apps::build_admin();
        let schema = Schema::from_admin(&admin);
        schema.write_to(std::path::Path::new("rustio.schema.json"))?;
        eprintln!(
            "wrote rustio.schema.json ({} model{})",
            schema.models.len(),
            if schema.models.len() == 1 { "" } else { "s" },
        );
        return Ok(());
    }

    // Schema is managed by `rustio migrate apply`, which also creates
    // the `rustio_users` / `rustio_sessions` tables auth depends on.
    // Override the database URL with RUSTIO_DATABASE_URL if needed.
    let url = std::env::var("RUSTIO_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());
    let db = Db::connect(&url).await?;

    // `authenticate(db)` returns a middleware that reads the session
    // cookie on every request, validates it against `rustio_sessions`,
    // and attaches `Identity` to the context when valid.
    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    let router = apps::register_all(router, &db);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8000));
    eprintln!("serving on http://{addr}");
    Server::bind(addr).serve_router(router).await?;
    Ok(())
}
