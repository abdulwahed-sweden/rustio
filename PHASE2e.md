# Phase 2e — Clean rebuild of Docker stack with RustIO-specific names

Built on top of Phase 2 (commit `66c40fd`).

## Commits shipped

```
HEAD     phase 2e/3: update docs for new docker stack names    ← this commit
5e969f2  phase 2e/2: switch all references from blog database to rustio_dev
77de5fe  phase 2e/1: rename docker stack to rustio-* with rustio_dev database
```

## Final `docker compose ps`

```
NAME                 IMAGE                        SERVICE       STATUS
rustio-postgres      postgres:16                  postgres      Up (healthy)   0.0.0.0:5432->5432/tcp
rustio-meilisearch   getmeili/meilisearch:v1.10   meilisearch   Up (healthy)   0.0.0.0:7700->7700/tcp
```

Container names are now stable (`rustio-postgres`, not the auto-generated `rustio-postgres-1`).

## Connection probe — both sides

From inside the postgres container:

```
$ docker compose exec postgres psql -U postgres -d rustio_dev \
    -c "SELECT current_database(), current_user, version();"
 current_database | current_user |          version
------------------+--------------+----------------
 rustio_dev       | postgres     | PostgreSQL 16.13 …
```

From host (sandbox in this case — confirms TCP + auth from outside the container):

```
$ PGPASSWORD=dev psql -h localhost -p 5432 -U postgres -d rustio_dev \
    -c "SELECT 'ok' AS reachable_from_host;"
 reachable_from_host
---------------------
 ok
```

## Test counts in both modes

```
=== sandbox mode (cargo test --workspace) ===
test result: ok. 213 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out

=== host mode (RUSTIO_TEST_DB=1 RUSTIO_TEST_DATABASE_URL=postgres://postgres:dev@localhost:5432/rustio_dev
                cargo test --workspace -- --ignored) ===
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 213 filtered out
```

(Both modes also produce four `0 passed; 0 failed; 0 ignored` lines from the empty test binaries — `rustio-cli` bin, `rustio-macros` lib, `blog` bin — they just have no `#[test]`s.)

## Every place "blog" was replaced

Database-name replacements (`blog` → `rustio_dev`) — in commit `5e969f2`:

| File | Line(s) | Before | After |
|---|---|---|---|
| `docker-compose.yml` | line 8 (POSTGRES_DB) | `blog` | `rustio_dev` |
| `docker-compose.yml` | line 14 (healthcheck) | `pg_isready -U postgres -d blog` | `pg_isready -U postgres -d rustio_dev` |
| `Makefile` | line 1 (DB_URL) | `…/blog` | `…/rustio_dev` |
| `Makefile` | line 23 (db-setup probe) | `datname='blog'` | `datname='rustio_dev'` |
| `Makefile` | line 24 (db-setup create) | `CREATE DATABASE blog` | `CREATE DATABASE rustio_dev` |
| `README.md` | line 32 (createdb) | `createdb … blog` | `createdb … rustio_dev` |
| `README.md` | line 33 (DATABASE_URL) | `…/blog` | `…/rustio_dev` |
| `examples/blog/src/main.rs` | line 11 (docstring URL) | `…/blog` | `…/rustio_dev` |
| `examples/blog/src/main.rs` | line 12 (docstring `createdb`) | `createdb blog` | `createdb rustio_dev` |
| `examples/blog/src/main.rs` | line 40 (runtime fallback URL) | `…/blog` | `…/rustio_dev` |
| `examples/blog/README.md` | line 18 (`make migrate` comment) | `the blog database` | `the rustio_dev database` |
| `rustio-core/src/ai/executor_pg_tests.rs` | line 14 (doc URL) | `…/blog` | `…/rustio_dev` |
| `rustio-core/src/ai/executor_pg_tests.rs` | line 47 (`default_dev_url`) | `…/blog` | `…/rustio_dev` |
| `PHASE2.md` | lines 55, 129 (documentation) | `…/blog` | `…/rustio_dev` |

Docker stack rename (commit `77de5fe`) — in `docker-compose.yml`:

| What | Before | After |
|---|---|---|
| PG container name | (auto) `rustio-postgres-1` | explicit `container_name: rustio-postgres` |
| Meili container name | (auto) `rustio-meilisearch-1` | explicit `container_name: rustio-meilisearch` |
| PG volume | `rustio_pg` (alias) → on disk `rustio_rustio_pg` | `rustio_pg_data` (alias) → on disk `rustio_rustio_pg_data` |
| Meili volume | `rustio_meili` → `rustio_rustio_meili` | `rustio_meili_data` → `rustio_rustio_meili_data` |

Docs update (commit /3 — this commit) — `README.md` quick-start switched to `docker compose up -d`, and a "Running the test suite" subsection added explaining the two modes (default vs `RUSTIO_TEST_DB=1 -- --ignored`).

## What was NOT touched

Per the spec — the example crate name `blog` is a different thing from the `blog` database. Every `blog` reference that means "the example crate" stays:

- `Cargo.toml:7` — workspace member `"examples/blog"`
- `examples/blog/Cargo.toml:2` — `name = "blog"`
- `examples/blog/` directory tree (path is the crate name)
- Every `cargo run -p blog` invocation in Makefile, README, PROGRESS.md
- Every `examples/blog/migrations`, `cd examples/blog`, `examples/blog/src/...` path
- Doc-comment narrative ("A blog example", "Blog — RustIO example")
- `rustio-macros/src/lib.rs:357` — `// "blog_posts" → "Blog posts"` (humanise() example)
- Historical PROGRESS.md and STATUS.md hits

The host's Postgres `ajaweed` database (the user's unrelated project) was never touched — only the Docker stack was wiped + recreated.

## Verified

- `cargo check --workspace --all-targets` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- Sandbox mode: 213 passed / 8 ignored (the gated PG suite)
- Host mode: 8 passed (all PG integration tests against `rustio_dev`)
- Both Docker containers report `(healthy)` after compose up
- PG reachable both from inside the container and from the host
