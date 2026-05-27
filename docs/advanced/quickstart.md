> **Advanced docs.** This file goes deep — APIs, internals, gotchas.
> If you'''re new to RustIO, start at the [main README](../../README.md) first.
> It walks you from zero to a running admin in 5 minutes.

# Quickstart — 15 minutes to a running admin

## What you'll see at the end

A running admin UI at `http://127.0.0.1:8000/admin` with:

- a **list page** for one model, showing a clean set of default columns (not every field — RustIO picks the most useful ones)
- a **toolbar** with full-text search, a primary dropdown, a `Sort` control, and a **More filters** button that opens a secondary panel
- a **Columns** menu to hide or reveal columns instantly with no page reload
- **filter chips** under the toolbar for every active filter, each with an `×` that removes it and rewrites the URL
- a **chevron in the first column** of each row; clicking it reveals the fields that didn't fit in the default columns as an inline read-only panel
- working **Create / Edit / Delete** forms, login at `/admin`, and CSRF — all wired by the framework

No hand-written HTML. No template engine. Just a Rust struct and a derive.

---

## 1. Install

```bash
cargo install rustio-cli
```

Requires **Rust 1.75+** and a C toolchain for `sqlx` / SQLite. That's it — no Node, no Docker, no database server.

## 2. Create a project

```bash
rustio init myblog --preset blog
cd myblog
```

The `blog` preset scaffolds a project with one app called `posts`, already registered in `apps/mod.rs`. The `basic` preset gives you an empty project; the `api` preset scaffolds an `items` app instead.

## 3. Look at the model RustIO generated

Open `apps/posts/models.rs`:

```rust
use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub is_active: bool,
    pub priority: i32,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] = &["id", "title", "is_active", "priority"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "is_active", "priority"];

    fn id(&self) -> i64 { self.id }
    fn from_row(row: Row<'_>) -> Result<Self, Error> { /* generated */ }
    fn insert_values(&self) -> Vec<Value> { /* generated */ }
}
```

`#[derive(RustioAdmin)]` is what produces every admin page for this struct. The `Model` impl handles the DB round-trip; the CLI wrote both for you. The matching migration is already at `migrations/0001_create_posts.sql`.

Add a field later? Either edit the struct + migration by hand, or use `rustio ai plan "..." --save p.json && rustio ai apply p.json --yes`.

## 4. Apply migrations and create an admin user

```bash
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
```

First command creates the `posts` table plus the `rustio_users` / `rustio_sessions` tables auth needs. Second command gives you a login.

⚠️ **Common issues**

- *"not inside a RustIO project"* — you didn't `cd myblog`.
- *"no such table: rustio_users"* — you skipped `rustio migrate apply`.
- *"email already exists"* — you already ran `user create`; just use that login.

## 5. Run the server and open the admin

```bash
rustio run
```

You should see `serving on http://127.0.0.1:8000` in the terminal. Open <http://127.0.0.1:8000/admin>, sign in, and click into **Posts**. Add a couple of records, then try:

- typing in the search box and pressing **Search** → a chip appears under the toolbar
- clicking the chip's `×` → the filter is removed, URL updates, sort is preserved
- clicking **Columns** → uncheck `priority`, watch it vanish without a reload
- clicking the **chevron** on any row → hidden fields show inline (becomes visible once you add a few more fields to the model)

⚠️ **Common issues**

- *Page loads but I can't log in* — you created the user before running migrations; sessions table didn't exist yet. Recreate with `rustio user create`.
- *`/admin` is blank / 404* — you edited `apps/mod.rs` and removed the scaffolded app. The markers (`// -- modules --` etc.) must stay.
- *Port already in use* — another `rustio run` is still open, or port 8000 is taken. Kill it or set `RUSTIO_DATABASE_URL` + run on a different port via your own `main.rs`.

---

## What RustIO generated for you

From **one struct + one derive**:

| route | page |
|---|---|
| `GET /admin` | dashboard listing every registered model |
| `GET /admin/posts` | list with search, filters, chips, columns toggle, row expansion, sort, bulk delete |
| `GET /admin/posts/create` · `POST /admin/posts/create` | create form |
| `GET /admin/posts/:id` | read-only detail page |
| `GET /admin/posts/:id/edit` · `POST /admin/posts/:id/edit` | edit form |
| `GET /admin/posts/:id/delete` · `POST /admin/posts/:id/delete` | confirmed delete |
| `POST /admin/posts/bulk_action` | multi-row delete |
| `GET /admin/login` · `POST /admin/login` · `POST /admin/logout` | auth |

Also generated: `rustio.schema.json` (run `rustio schema`) — the contract every AI tool reads.

Next: **`build-school-app.md`** walks through a real four-model system with relations.
