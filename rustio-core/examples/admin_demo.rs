use std::net::SocketAddr;

use rustio_core::admin;
use rustio_core::auth::authenticate;
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Error, Model, Router, Row, RustioAdmin, Server, Value};

#[derive(Debug, RustioAdmin)]
struct User {
    id: i64,
    name: String,
    is_admin: bool,
}

impl Model for User {
    const TABLE: &'static str = "users";
    const COLUMNS: &'static [&'static str] = &["id", "name", "is_admin"];
    const INSERT_COLUMNS: &'static [&'static str] = &["name", "is_admin"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
            is_admin: row.get_bool("is_admin")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![self.name.clone().into(), self.is_admin.into()]
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let db = Db::memory().await.expect("db connect");
    db.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            is_admin INTEGER NOT NULL
        )",
    )
    .await
    .expect("create schema");

    User {
        id: 0,
        name: "Alice".into(),
        is_admin: false,
    }
    .create(&db)
    .await
    .expect("seed alice");
    User {
        id: 0,
        name: "Bob".into(),
        is_admin: true,
    }
    .create(&db)
    .await
    .expect("seed bob");

    let router = with_defaults(Router::new()).wrap(authenticate);
    let router = admin::register::<User>(router, &db);

    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
    eprintln!("admin demo: hit /admin/users with `Authorization: Bearer dev-admin` header");
    Server::bind(addr).serve_router(router).await
}
