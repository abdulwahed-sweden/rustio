# Bring-up log — initial port (Phases 1–5)

> **Historical document.** Captures the build-readiness checklist
> from the early days of the port: `compile → clippy → tests →
> end-to-end → smoke`. The numbering here ("Phase 1 — compile",
> "Phase 2 — clippy", …) is **not** the same as the chronological
> port phases in `docs/phases/PHASE*.md` — this is a parallel
> build-pipeline taxonomy from before the phase reports started.
>
> Kept for context on how the workspace first became green. For
> current architecture and behaviour, see `docs/architecture.md`
> and `CHANGELOG.md`.

## Files modified

### Phase 1 — compile
- `Cargo.toml` — added `examples/blog` to workspace members
- `rustio-core/src/router.rs` — fixed `for<'a>` HRTB lifetime binding
- `rustio-core/src/admin/types.rs` — disambiguated `AdminModel::id` vs `Model::id`
- `rustio-core/src/ai/review.rs` — added `Ord` / `PartialOrd` on `Risk`
- `rustio-core/src/auth/permissions.rs` — dropped unused imports; kept `Role` under `cfg(test)`
- `rustio-core/src/ai/planner.rs` — dropped unused `mut`
- `rustio-cli/Cargo.toml` — added `sqlx`, enabled `clap/env`
- `examples/blog/Cargo.toml` — added `sqlx`

### Phase 2 — clippy
- `rustio-core/src/admin/types.rs` — introduced `CreateResult`/`UpdateResult` aliases; `#[allow(dead_code)]` on `EditRow.id`
- `rustio-core/src/ai/executor.rs` — `ApplyOptions` now `#[derive(Default)]`
- `rustio-core/src/auth/mod.rs` — fixed doc-list indent
- `rustio-core/src/auth/permissions.rs` — made `invalidate_group_cache` return `()` and drop the `let _ =`
- `rustio-core/src/auth/users.rs` — renamed `Role::from_str` → `Role::parse` (avoid `FromStr` conflict)
- `rustio-core/src/auth/sessions.rs`, `rustio-core/src/admin/builtin.rs`, `rustio-cli/src/main.rs` — followed the rename
- `rustio-core/src/router.rs` — `&route.method == method` → `route.method == *method`
- `rustio-core/src/search/mod.rs` — fixed doc-list indent
- `rustio-core/src/search/client.rs` — `as_ref().map(|s| s.as_slice())` → `as_deref()`
- `rustio-cli/src/main.rs` — `&PathBuf` → `&Path` on four helpers

### Phase 3 — tests
- `rustio-core/src/ai/mod.rs` — rewrote the module-doc list so the arrow characters no longer get parsed as a doctest

### Phase 4 — end-to-end
- `docker-compose.yml` — Postgres 16 + Meilisearch 1.10 with healthchecks
- `Makefile` — `up`, `down`, `db-setup`, `migrate`, `run`, `check`, `clean`
- `examples/blog/README.md` — setup / smoke-test / reset
- `rustio-core/src/background.rs` — added startup log line so `spawn_session_sweeper` is visible on boot

### Phase 5 — smoke
- `scripts/smoke-test.sh` — login → CSRF → create → search loop

### Follow-up fixes
- `rustio-core/src/admin/routes.rs` — registered `GET /static/rustio.css` inside `register_admin_routes` so the embedded stylesheet is served (templates link to it; was 404'ing)
- `examples/blog/src/main.rs` — resolved the migrations dir against `CARGO_MANIFEST_DIR` so `cargo run -p blog` finds `examples/blog/migrations/` regardless of CWD (overridable via `MIGRATIONS_DIR`)

## Bugs fixed

- `router.rs`: HRTB `for<'a> Fn(…) -> BoxFuture<'a, …>` was unsatisfiable because `'a` never appears in the inputs. Switched both type aliases to `BoxFuture<'static, …>` since the returned future is always owned.
- `admin/types.rs`: `ConcreteOps::{list,find_row}` called `.id()` on a value implementing both `AdminModel` and `Model`. Disambiguated with `AdminModel::id(&x)` (same signature, just picking the trait we already imported).
- `ai/review.rs`: `std::cmp::max(max_risk, step_risk)` needs `Ord`; added `PartialOrd, Ord` to the derive.
- `auth/permissions.rs`: dead imports after the refactor; kept `Role` behind `#[cfg(test)]` for the unit test.
- `ai/planner.rs`: `let mut plan` but never mutated.
- `rustio-cli/Cargo.toml`: `sqlx` was referenced from `main.rs` but not a dep; `clap`'s `env` attribute needs the `env` feature.
- `examples/blog/Cargo.toml`: same — `seed_editors_group` uses `sqlx::query_scalar` directly.
- `Role::from_str` conflicted with the `FromStr` trait; renamed to `Role::parse` at every call site.
- `background.rs`: no boot-time log, so `make up && make run` did not visibly show the session-sweeper — added one INFO line on spawn.
- `admin/routes.rs`: the admin templates link to `/static/rustio.css` but no route served it, so the login page rendered unstyled. Registered the embedded stylesheet inside `register_admin_routes` — any app calling that function now gets styling automatically.
- `examples/blog/src/main.rs`: `migrations::apply(&db, "migrations")` silently did nothing when run from the repo root (the actual dir is `examples/blog/migrations/`). Switched to `CARGO_MANIFEST_DIR`-relative resolution with a `MIGRATIONS_DIR` override.

## Notes

- Integration tests that require a running Postgres are not wired up yet; the unit-test suite (38 tests) does not touch the network.
- `EditRow.id` is on the public API but unused by the default renderer; kept it (`#[allow(dead_code)]`) rather than breaking the surface.

## Verified

- `cargo check --workspace --all-targets` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
- `cargo test --workspace` — 38 passed, 0 failed
- `make up && make migrate && cargo run -p blog` — server logs "rustio listening", `/admin/login` returns 200, `/admin` returns 303 → `/admin/login`, session sweeper logged on boot
- `scripts/smoke-test.sh` — passes end-to-end (login → CSRF → create post → search finds it)

## Search UI

### Files touched
- `rustio-core/src/search/traits.rs` — added `FACETABLE_ATTRIBUTES` assoc const
- `rustio-core/src/search/client.rs` — `SearchOptions` gains `facets`, `highlight_pre_tag`, `highlight_post_tag`; `SearchResults` gains `facet_distribution` (`BTreeMap<String, BTreeMap<String, u64>>`); derives `Serialize` so handlers can pass it straight to templates or JSON
- `rustio-core/assets/templates/search.html` — new embedded template, extends `base.html`
- `rustio-core/src/templates.rs` — registered `search.html` in `EMBEDDED_TEMPLATES`
- `rustio-core/assets/static/js/search.js` — new 239-line vanilla-JS client (debounce, AbortController, facet sync, chips, URL replaceState, keyboard nav)
- `rustio-core/src/server.rs` — added `embedded_search_js()` helper
- `rustio-core/src/admin/routes.rs` — registered `/static/search.js` alongside `/static/rustio.css`
- `rustio-core/assets/static/css/rustio.css` — appended the full search page style block (7,815 → 13,126 bytes); highlighted `<mark>` uses `--accent-soft` / `--accent`; `.tag-published` uses `--success-soft` / `--success`
- `examples/blog/migrations/0002_add_author_to_posts.sql` — adds `author TEXT NOT NULL DEFAULT 'anonymous'` with a `posts_author_idx`
- `examples/blog/src/apps/posts/model.rs` — `Post` gains `author`; `Searchable` lists `author` as searchable/filterable/facetable
- `examples/blog/src/apps/posts/search.rs` — rewrote as `run_search` (shared) + `search_json` + `search_html`; parses q/published/author/date_range/sort/page; builds the Meili filter expression, `_formatted` highlights wrapped in `<mark>…</mark>`
- `examples/blog/src/apps/posts/mod.rs` — re-exports the two handlers
- `examples/blog/src/main.rs` — dual-mode `GET /search` branches on `?format=json`
- `examples/blog/README.md` — (unchanged) already documents `make migrate`, which now picks up 0002 automatically

### Bugs fixed during the build
- Server-side JSON initially double-escaped via `replace(...)` AND minijinja's auto-escape — JS couldn't parse the `data-initial`. Dropped the hand-roll; let minijinja escape once.
- First clippy run flagged an overindented doc list in `search.rs`; fixed to 2-space continuation.
- The `author` column needed an explicit `NOT NULL DEFAULT 'anonymous'` so the migration could backfill existing rows without a separate `UPDATE`.

### Browser checklist (the shell can't verify these — open <http://127.0.0.1:8000/search> and confirm)
- Type `rust` in the box — results appear within ~200ms without pressing Enter; `<mark>` highlights wrap matches; network tab shows a `format=json` fetch after each keystroke with the old one cancelled (Chrome DevTools shows "(canceled)")
- Click the "Published" facet checkbox — URL updates to `?published=true`, chip appears above meta bar, author counts shrink to only-published
- Click the `×` on the "Published" chip — checkbox unticks, URL loses `published`, counts restore
- Change "Sort: Relevance" to "Newest first" — results reorder, URL gains `&sort=newest`
- Focus outside the input and press `/` — search box refocuses and its text is selected
- With the input focused, press `↓ ↓ Enter` — second result card highlights (rust-orange left bar) then the browser navigates to `/admin/posts/<id>/edit`
- Press `Escape` while the input has text — text clears and results reset
- Load `/search?q=xyzblargh` directly — empty state "No results for xyzblargh. Try removing filters."; add `&published=true` and the empty state shows a "Clear all filters" link that actually works
