# Phase 3 — Port `admin/relations.rs` logic from OLD

Built on top of Phase 2e (commit `77fc396`).

## Function classification

OLD's `admin/relations.rs` (418 LOC) and `admin/relations_tests.rs` (416 LOC) are pure data + lookup code. Per the file's own doc-comment:

> Rendering. The admin decides how a resolved relation looks on the page; the registry only tells it which target to look up.

Every function returns a data structure; nothing returns a `String` of HTML. Classification:

| # | Item | Lines | Returns | Verdict |
|---|---|---:|---|---|
| const | `RELATION_FILTER_DROPDOWN_CAP` | 1 | `usize` | PORT |
| struct | `ResolvedRelation` | 19 | type | PORT |
| struct | `InverseRelation` | 16 | type | PORT |
| enum  | `RegistryError` (+ Display + Error impl) | 27 | type | PORT |
| struct | `RelationRegistry` | 9 | type | PORT |
| fn | `RelationRegistry::empty()` | 3 | `Self` | PORT |
| fn | `RelationRegistry::from_schema(&Schema)` | 82 | `Self` | PORT |
| fn | `RelationRegistry::belongs_to(model, field)` | 3 | `Option<&ResolvedRelation>` | PORT |
| fn | `RelationRegistry::belongs_to_of(model)` | 6 | `&[ResolvedRelation]` | PORT |
| fn | `RelationRegistry::has_many(model)` | 6 | `&[InverseRelation]` | PORT |
| fn | `RelationRegistry::is_empty()` | 3 | `bool` | PORT |
| fn | `RelationRegistry::validate(&Schema)` | 33 | `Vec<RegistryError>` | PORT |
| fn | `RelationRegistry::iter_belongs_to()` | 8 | `impl Iterator<…>` | PORT |

**13 items, all PORT, 0 DROP.**

## LOC moved

| | Lines | Source | Destination |
|---|---:|---|---|
| `relations.rs` | 418 | `OLD/rustio-core/src/admin/relations.rs` | `NEW/rustio-core/src/admin/relations.rs` |
| `relations_tests.rs` | 416 | `OLD/rustio-core/src/admin/relations_tests.rs` | `NEW/rustio-core/src/admin/relations_tests.rs` |
| `admin/mod.rs` | +13 | — | `mod relations;` + `#[cfg(test)] mod relations_tests;` + 5-symbol re-export block |

**834 LOC ported, 0 LOC dropped, 13 LOC added in `mod.rs`.** No file body was edited — both source files are byte-identical to OLD.

## Tests ported

All 12 tests from `OLD/admin/relations_tests.rs`:

```
test admin::relations_tests::relation_metadata_round_trips_through_json    ... ok
test admin::relations_tests::relation_without_display_field_round_trips    ... ok
test admin::relations_tests::registry_indexes_belongs_to_entries           ... ok
test admin::relations_tests::registry_inverts_every_stored_belongs_to      ... ok
test admin::relations_tests::dangling_target_is_skipped_at_build_and_reported_by_validate         ... ok
test admin::relations_tests::unknown_display_field_is_skipped_at_build_and_reported_by_validate   ... ok
test admin::relations_tests::empty_schema_produces_empty_registry          ... ok
test admin::relations_tests::empty_registry_is_safe_default                ... ok
test admin::relations_tests::relation_filter_dropdown_cap_is_500           ... ok
test admin::relations_tests::belongs_to_of_lists_every_fk_on_a_model       ... ok
test admin::relations_tests::iter_belongs_to_is_deterministic              ... ok
test admin::relations_tests::resolved_relation_carries_admin_slug_and_table ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 221 filtered out
```

## Types now used in NEW that weren't before

None — every type the ported file references was already present from earlier phases:

- `crate::schema::Schema`, `SchemaModel`, `SchemaField`, `Relation`, `RelationKind` — copied verbatim from OLD in **Phase 1**
- `SchemaModel.table` + `SchemaModel.core` — added in **Phase 2a** (the same change that unignored the four schema-export tests)

The compatibility was load-bearing on Phase 2a's work: `from_schema` reads `target.table` and `target.admin_name` directly. If `AdminEntry`/`SchemaModel` had still lacked `table` (Phase 1's Path B fudge), the port would have either failed or produced wrong table names in `ResolvedRelation`. Phase 2a's surgical addition unblocked this port without anyone noticing.

## Verification (paste-proof)

```
$ cargo check --workspace --all-targets         → clean
$ cargo clippy --workspace --all-targets -- -D warnings  → clean
$ cargo test --workspace -p rustio-core --lib relations  → 12 passed, 0 failed
$ cargo test --workspace                        → 225 passed, 0 failed, 8 ignored
```

The 8 ignored are the same Phase-2 PG integration tests gated behind `RUSTIO_TEST_DB=1`. Going from 213 → 225 = exactly the +12 ported tests landing.

## Is the relations module complete?

**For a runtime data layer, yes.** Every lookup the admin needs (`belongs_to`, `belongs_to_of`, `has_many`, `is_empty`, `validate`, `iter_belongs_to`) is in place, with deterministic ordering and lenient build-time behavior (dangling targets are recorded by `validate` rather than blanking out the registry). The `RELATION_FILTER_DROPDOWN_CAP = 500` constant is exported for future filter-rendering callers.

**What's deliberately not here:** any HTML, any SQL, any Db handle. The doc-comments name three extension points the next iterations are expected to grow into:

- **FK dropdown rendering** in `<select>` form — deferred to Phase 6 when the templating layer lands.
- **Inverse-panel preview rows** (top-N latest related rows) — needs a `preview_query` helper on `ResolvedRelation` plus a `render_related_preview` template hook; deferred.
- **Relation-aware search** for `?q=…` — needs the list query builder in `admin.rs` to support projection aliases first; deferred.

For Phase 3's stated scope ("port the relation-tracking logic"), the module is **complete and ready** for the next phase to consume.
