# RustIO

A production-grade, strict-by-construction web framework for Rust.

Write a model struct, derive `RustioAdmin`, and you get the admin UI,
HTTP server, Postgres ORM, migrations, full-text search, sessions, and
granular permissions — without writing the glue.

## What's in 1.0

| Layer | Implementation |
|---|---|
| HTTP | hyper 1.x, HTTP/1.1 + keep-alive, graceful shutdown |
| Database | PostgreSQL via sqlx, 30-conn pool, prepared-statement cache |
| Search | Meilisearch (REST), async batch indexer with backpressure |
| Templates | minijinja, embedded defaults + on-disk overrides |
| Auth | argon2id passwords, DB-backed sessions, HttpOnly cookies |
| Authorization | Users + Groups + per-action permissions, 60s LRU cache |
| Caching | In-process LRU query cache with prefix invalidation |
| Middleware | logger, csrf, rate_limit, gzip, security_headers |

## Quick start

Prereqs: PostgreSQL 14+, Meilisearch 1.10+.

```bash
# Spin up the dev backends
docker run --rm -d -p 5432:5432 -e POSTGRES_PASSWORD=dev postgres:16
docker run --rm -d -p 7700:7700 getmeili/meilisearch:v1.10

# Set up
createdb -h localhost -U postgres blog
export DATABASE_URL=postgres://postgres:dev@localhost/blog
export MEILI_URL=http://localhost:7700

# Run the example
cd examples/blog
cargo run

# Open http://127.0.0.1:8000/admin
# Log in with admin@example.com / admin
```

## A model end-to-end

```rust
use chrono::{DateTime, Utc};
use rustio_core::{Error, Model, Row, RustioAdmin, Searchable, Value};
use serde_json::json;

#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub published: bool,
    pub created_at: DateTime<Utc>,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] =
        &["id", "title", "body", "published", "created_at"];
    const INSERT_COLUMNS: &'static [&'static str] =
        &["title", "body", "published", "created_at"];

    fn id(&self) -> i64 { self.id }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            body: row.get_string("body")?,
            published: row.get_bool("published")?,
            created_at: row.get_datetime("created_at")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.body.clone().into(),
            self.published.into(),
            self.created_at.into(),
        ]
    }
}

impl Searchable for Post {
    const INDEX_NAME: &'static str = "posts";
    const SEARCHABLE_ATTRIBUTES: &'static [&'static str] = &["title", "body"];
    const FILTERABLE_ATTRIBUTES: &'static [&'static str] = &["published"];

    fn to_search_doc(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "title": self.title,
            "body": self.body,
            "published": self.published,
        })
    }
}
```

Wire it up:

```rust
let admin = Admin::new()
    .model_with_search::<Post>(indexer.clone());
admin.seed_permissions(&db).await?;
```

You now have:
- `/admin/posts` — list with edit/delete
- `/admin/posts/new` — add form (CSRF-protected)
- `/admin/posts/:id/edit` — edit form
- `/admin/posts/:id/delete` — confirm-and-delete
- Auto-emitted permissions: `posts.add_post`, `posts.change_post`,
  `posts.delete_post`, `posts.view_post`
- Auto-indexing into Meilisearch on every write

## Users, groups, permissions

Three roles:
- **Admin** — superuser. Bypasses every permission check.
- **Staff** — can access admin, but only does what permissions allow.
- **User** — no admin access.

Granting permissions, two paths:

```bash
# Direct grant (rare)
rustio perm grant-user --email alice@x.com --permission posts.change_post

# Via group (preferred)
rustio group create editors --description "Can manage posts"
rustio group grant --group editors --permission posts.add_post
rustio group grant --group editors --permission posts.change_post
rustio user add-to-group --email alice@x.com --group editors
```

## CLI reference

```
rustio new project <name>             scaffold a new project
rustio new app <name>                 scaffold an app inside one

rustio migrate apply                  apply pending migrations
rustio migrate generate <name>        create a new empty migration file
rustio migrate status                 show which migrations are applied

rustio user create --email --role     create a user (prompts for password)
rustio user set-password --email      reset a password
rustio user add-to-group --email --group

rustio group create <name>            create a group
rustio group grant --group --permission

rustio perm list                      list every registered permission
rustio perm grant-user --email --permission

rustio ai plan "<prompt>"             rule-based plan from prose
rustio ai review <plan.json>          score risk against current schema
rustio ai apply <plan.json>           write migration files
```

Every command that talks to the DB takes `--db` or reads `DATABASE_URL`.

## Performance notes

- **Connection pool: 30** by default. SQLx prepares statements per
  connection and caches them; first query on a connection pays the
  prepare cost, subsequent queries don't.
- **Read cache: 2048 entries.** Tune with `DbOptions { cache_capacity }`.
  Invalidates by table prefix on every write.
- **Permission cache: 60s TTL.** Hot admin endpoints don't touch the
  permission tables on every request.
- **Search batching: 100ms / 500 docs.** Bulk indexing is ~10× faster
  than per-doc round-trips. Backpressure via a 1024-entry channel.
- **Session reads: O(1) DB hit + an out-of-band `last_seen` UPDATE
  fired on a tokio task** — request doesn't wait for it.
- **gzip kicks in at 1KB.** Below that, compression overhead beats
  the savings.

## Architecture

```
rustio-core/                          ~7,000 LOC, 30+ tests
├── src/
│   ├── admin/        list/edit/delete UI + built-in users/groups
│   ├── ai/           planner, reviewer, executor (deterministic)
│   ├── auth/         users, sessions, permissions (split files)
│   ├── middleware/   logger, csrf, rate_limit, gzip, security
│   ├── search/       Meilisearch client + async indexer
│   ├── orm.rs        Postgres pool, Model trait, CRUD helpers
│   ├── cache.rs      LRU query cache
│   ├── server.rs     hyper glue + graceful shutdown
│   └── ...
└── assets/
    ├── templates/    12 default Jinja templates (admin)
    └── static/       hand-written CSS, ~280 lines

rustio-macros/                        ~400 LOC
└── src/lib.rs        #[derive(RustioAdmin)]

rustio-cli/                           ~470 LOC
└── src/main.rs       the `rustio` binary

examples/blog/                        ~150 LOC
└── full Postgres + search + auth example
```

See `docs/architecture.md` for the longer version.

## Design principles (unchanged from 0.9)

1. **Single binary.** Default templates and CSS are baked in via
   `include_str!`. Override anything by dropping a file in
   `templates/` or `static/`.
2. **No magic.** The macro emits readable code (`cargo expand`).
3. **Plain HTML + plain CSS.** No Bootstrap, no Tailwind, no React.
4. **Deterministic AI.** The planner is rule-based. No LLM at runtime.
5. **Strict by construction.** The AI's `Primitive` enum is
   `#[non_exhaustive]` + `deny_unknown_fields`. Destructive
   operations refuse without `--yes`.

## License

MIT
