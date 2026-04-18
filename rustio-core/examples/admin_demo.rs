use std::net::SocketAddr;

use rustio_core::admin;
use rustio_core::auth::{self, authenticate};
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Error, Model, Router, Row, RustioAdmin, Server, Value};

// `User` is the built-in core auth model; the demo ships a different
// admin model to avoid colliding with it.
#[derive(Debug, RustioAdmin)]
struct Member {
    id: i64,
    name: String,
    is_active: bool,
}

impl Model for Member {
    const TABLE: &'static str = "members";
    const COLUMNS: &'static [&'static str] = &["id", "name", "is_active"];
    const INSERT_COLUMNS: &'static [&'static str] = &["name", "is_active"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            is_active: row.get_bool("is_active")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![self.name.clone().into(), self.is_active.into()]
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db = Db::memory().await.expect("db connect");
    auth::ensure_core_tables(&db)
        .await
        .expect("create auth tables");
    db.execute(
        "CREATE TABLE members (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            is_active INTEGER NOT NULL
        )",
    )
    .await
    .expect("create schema");

    auth::user::create(&db, "admin@example.com", "admin", "admin")
        .await
        .expect("seed admin user");

    Member {
        id: 0,
        name: "Alice".into(),
        is_active: true,
    }
    .create(&db)
    .await
    .expect("seed alice");

    let router = with_defaults(Router::new()).wrap(authenticate(db.clone()));
    let router = admin::register::<Member>(router, &db);

    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    eprintln!("admin demo: open http://{addr}/admin and sign in as admin@example.com / admin");
    Server::bind(addr).serve_router(router).await
}
