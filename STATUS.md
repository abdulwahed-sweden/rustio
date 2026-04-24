# STATUS

Side-by-side snapshot of both projects. Read-only — collected `2026-04-24`.

| # | Item | OLD `~/Documents/GitHub/rustio` | NEW `~/Documents/rustio` |
|---|---|---|---|
| 1 | Cargo.toml `version` | `0.3.1` | `1.0.0` |
| 2 | Latest CHANGELOG header | `## [Unreleased]` → `### Added — 0.9.1 Destructive-op gate` | `## 1.0.0 — Production stack` |
| 2a | Latest CHANGELOG body (first ~6 bullets) | Wires `--force` on `rustio ai apply` for `remove_field` / `remove_relation`. Adds `apply_remove_field`, `apply_remove_relation`, FK-aware recreate-table, multi-model file support, `remove_model` still refused (0.9.2 scope). +5 core tests, +3 CLI tests → 519 total. Verified against medflow. | "Pivots RustIO from a single-machine, SQLite-backed admin toolkit into a production-grade web framework." Adds: PostgreSQL `Db` (sqlx pool), in-process LRU `cache::QueryCache`, Meilisearch via `MeiliClient` + background `Indexer`, full RBAC (users/groups/permissions, 60s perm cache), built-in `/admin/users` + `/admin/groups`, CSRF middleware, rate limit, gzip, security headers, background housekeeping, graceful shutdown, HTTP/1.1 keep-alive. |
| 3 | Last 5 commits (`git log --oneline -5`) | `93dd720 feat(admin): port /admin/suggestions review + apply (stage 4h-iv)`<br>`84f002e feat(admin): port /admin/actions audit timeline (stage 4h-iii)`<br>`19b5853 feat(admin): port /admin/password_change to minijinja (stage 4h-ii)`<br>`5690c8b feat(admin): port /admin/profile to minijinja (stage 4h-i)`<br>`203d442 feat(admin): port 404 page to minijinja (stage 4g')` | `fatal: not a git repository` — no `.git` directory present |
| 4 | Workspace members (root `Cargo.toml`) | `rustio-core`, `rustio-cli`, `rustio-macros` | `rustio-core`, `rustio-cli`, `rustio-macros`, `examples/blog` |
| 5 | Test summary (final lines of `cargo test --workspace`) | `test result: ok. 51 passed; 0 failed; 0 ignored` (rustio-cli)<br>`test result: ok. 461 passed; 0 failed; 0 ignored` (rustio-core lib)<br>`test result: ok. 7 passed; 0 failed; 0 ignored` (medflow)<br>**Total: 519 passing** (last run: this session) | `test result: ok. 38 passed; 0 failed; 0 ignored` (rustio-core lib)<br>plus 0-count cli/macros/blog suites<br>**Total: 38 passing** (last run: prior session, search-UI work) |
| 6 | Top 10 files by LOC in `rustio-core/src/` (lines / file) | 7310 `admin.rs`<br>3194 `ai/executor.rs`<br>3164 `admin/layout.rs`<br>1569 `auth.rs`<br>1460 `ai/executor_tests.rs`<br>1450 `ai.rs`<br>1403 `ai/planner.rs`<br>1018 `ai/review.rs`<br>991 `schema.rs`<br>929 `ai/review_tests.rs`<br>**(34,048 LOC total)** | 368 `admin/types.rs`<br>366 `admin/routes.rs`<br>366 `admin/builtin.rs`<br>330 `auth/permissions.rs`<br>302 `migrations.rs`<br>291 `orm.rs`<br>274 `router.rs`<br>260 `admin/handlers.rs`<br>244 `http.rs`<br>242 `admin/render.rs`<br>**(5,716 LOC total)** |
| 7 | `examples/` exists? Contents | yes — `medflow` | yes — `blog` |
| 8a | `rustio-core/src/ai/` | yes | yes |
| 8b | `rustio-core/src/admin/intelligence.rs` | yes | no |
| 8c | `rustio-core/src/admin/suggestions.rs` | yes | no |
| 8d | `rustio-core/src/admin/audit.rs` | yes | no |
| 8e | `rustio-core/src/schema.rs` | yes | yes |
| 8f | `rustio-core/src/search/` (Meilisearch) | no | yes |
| 8g | `docker-compose.yml` | no | yes |
| 8h | `Makefile` | no | yes |
