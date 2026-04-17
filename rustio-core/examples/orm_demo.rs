use rustio_core::{Db, Error, Model, Row, Value};

#[derive(Debug)]
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
async fn main() -> Result<(), Error> {
    let db = Db::memory().await?;
    db.execute(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            is_admin INTEGER NOT NULL
        )",
    )
    .await?;

    let alice_id = User { id: 0, name: "Alice".into(), is_admin: false }
        .create(&db)
        .await?;
    let bob_id = User { id: 0, name: "Bob".into(), is_admin: true }
        .create(&db)
        .await?;
    println!("created ids: alice={alice_id} bob={bob_id}");

    let alice = User::find(&db, alice_id).await?.expect("alice");
    println!("find alice: {alice:?}");

    let all = User::all(&db).await?;
    println!("all: {all:?}");

    let renamed = User { id: alice_id, name: "Alicia".into(), is_admin: false };
    renamed.update(&db).await?;
    let after_update = User::find(&db, alice_id).await?.unwrap();
    println!("after update: {after_update:?}");

    User::delete(&db, bob_id).await?;
    println!("remaining after delete bob: {:?}", User::all(&db).await?);

    Ok(())
}
