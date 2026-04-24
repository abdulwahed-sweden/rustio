# Blog — RustIO example

End-to-end demo of the RustIO 1.0 stack: Postgres ORM, Meilisearch
full-text search, RBAC with groups, CSRF + rate-limit + gzip, and
the auto-generated admin UI.

## Requirements

- Docker (for Postgres + Meilisearch)
- Rust 1.75+

## Run

From the repository root:

```sh
make up         # start postgres + meilisearch, wait until healthy
make migrate    # apply examples/blog/migrations to the blog database
make run        # cargo run -p blog
```

Then open <http://127.0.0.1:8000/admin>.

Default login, seeded on first boot:

    admin@example.com / admin

## Smoke test

With the server running:

```sh
./scripts/smoke-test.sh
```

Exercises the full login → CSRF → create → search loop.

## Reset

```sh
make clean      # cargo clean + docker compose down -v (wipes the db)
```
