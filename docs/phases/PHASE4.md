# Phase 4 — Port `admin/intelligence.rs` logic from OLD

Built on top of Phase 3 (commit `16a692f`).

## Commits shipped

```
HEAD     phase 4b: port intelligence_tests.rs with NEW's AdminField shape    ← this commit
99a2c75  phase 4a: rename AdminField.ty → field_type in intelligence.rs
16a692f  phase 3: port admin/relations.rs logic from OLD (drop HTML)
```

Split into two commits so the library rename (4a) reviews independently of the test-fixture adaptation (4b).

## Function classification

OLD's `admin/intelligence.rs` (591 LOC) is a pure classification/rendering-hint layer. Per the file's own doc-comment:

> Nothing in this module touches the filesystem, the database, or produces HTML — it returns structured data that the admin renderer consumes.

(See the `context_global()` note at the bottom for the one item that violates this claim.)

Every function either returns a data structure (`FieldRole`, `FieldUI`, `Vec<FilterDef>`, `SearchIntent`) or a plain string (`format_relation_cell`, `mask_pii`, `humanise`) — no HTML. Classification:

| # | Item | Lines | Returns | Verdict |
|---|---|---:|---|---|
| enum | `FieldRole` | 14 | type | PORT |
| enum | `FilterKind` | 6 | type | PORT |
| struct | `FilterDef` | 4 | type | PORT |
| struct | `FieldUI` | 8 | type | PORT |
| enum  | `SearchIntent` (+ `label()`) | 20 | type | PORT |
| fn | `context_global()` | 9 | `Option<&'static ContextConfig>` | PORT (flagged — see footnote) |
| fn | `classify_field(&AdminField, Option<&ContextConfig>)` | 75 | `FieldRole` | PORT |
| fn | `field_ui_metadata(…)` | 72 | `FieldUI` | PORT |
| fn | `field_ui_metadata_with_relation(…)` | 22 | `FieldUI` | PORT |
| fn | `format_relation_cell(i64, Option<&str>)` | 14 | `String` (plain text, not HTML) | PORT |
| fn | `infer_filters(&[AdminField], Option<&ContextConfig>)` | 12 | `Vec<FilterDef>` | PORT |
| fn | `infer_filters_with_relations<F>(…)` | 51 | `Vec<FilterDef>` | PORT |
| fn | `classify_search_for_field(&str, Option<&str>)` | 17 | `SearchIntent` | PORT |
| fn | `classify_search(&str)` | 22 | `SearchIntent` | PORT |
| fn | `looks_like_email(&str)` | 20 | `bool` (private helper) | PORT |
| fn | `looks_like_personnummer(&str)` | 28 | `bool` (private helper) | PORT |
| fn | `mask_pii(&str)` | 27 | `String` | PORT |
| fn | `humanise(&str)` | 15 | `String` (private helper) | PORT |

**18 items, all PORT, 0 DROP.**

## Two mechanical changes applied during the port

The verbatim copy didn't compile against NEW because `AdminField` has diverged:

| Field | OLD `src/admin.rs:93` | NEW `src/admin/types.rs:53` |
|---|---|---|
| `name` | `&'static str` | `&'static str` |
| `label` | — | `&'static str` (new in NEW) |
| type field | `ty: FieldType` | `field_type: FieldType` (renamed) |
| `editable` | `bool` | `bool` |
| `nullable` | `bool` (gone in NEW) | — |
| `relation` | `Option<AdminRelation>` | `Option<AdminRelation>` |

### 4a — library rename (`intelligence.rs`)

Four `f.ty` → `f.field_type` renames in `classify_field` (lines 262, 265, 276, 279 of the ported file). No logic edits, no new branches, no helper added. `replace_all` against `matches!(f.ty,` → `matches!(f.field_type,`.

### 4b — test fixture adaptation (`admin_intelligence_tests.rs`)

Four fixture factories (`text`, `bigint`, `boolean`, `datetime`) each construct one `AdminField { … }` literal. Per-fixture changes:

- `ty: FieldType::…` → `field_type: FieldType::…` (rename to match NEW)
- `nullable: false,` removed entirely (field gone in NEW; `intelligence.rs` doesn't read nullability, so classification behavior is preserved)
- `label: name,` added (field required by NEW; `intelligence.rs` doesn't read `f.label`, confirmed by `grep -n "\.label" intelligence.rs` → zero hits, so label content is semantically inert and the fixture value just has to type-check)

Every `AdminField` in the 586-LOC test file is constructed via these four helpers — no inline struct literals to patch elsewhere. No assertion was touched. All 43 tests passed on first run after the fixture patch.

## Re-exports in `admin/mod.rs`

Per instruction, public types re-exported; internal helpers kept private. Because `mod intelligence` is a private module, **all `pub fn`s had to be re-exported too** — otherwise rustc flags them as dead (no callers in NEW until admin.rs is ported, Phase 5+). That would have failed `clippy -D warnings`. Re-exporting the public surface lets rustc treat them as library API reachable from the crate root.

```rust
pub use intelligence::{
    classify_field, classify_search, classify_search_for_field, context_global,
    field_ui_metadata, field_ui_metadata_with_relation, format_relation_cell, infer_filters,
    infer_filters_with_relations, mask_pii, FieldRole, FieldUI, FilterDef, FilterKind,
    SearchIntent,
};
```

**Not** re-exported (stayed module-private per instruction): `looks_like_email`, `looks_like_personnummer`, `humanise`. These are `fn` (not `pub fn`) in OLD and remain so.

## LOC moved

| | Lines | Source | Destination |
|---|---:|---|---|
| `intelligence.rs` | 591 | `OLD/rustio-core/src/admin/intelligence.rs` | `NEW/rustio-core/src/admin/intelligence.rs` (+4 mechanical renames) |
| `admin_intelligence_tests.rs` | 586 | `OLD/rustio-core/src/admin/admin_intelligence_tests.rs` | `NEW/rustio-core/src/admin/admin_intelligence_tests.rs` (4 fixture literals patched) |
| `admin/mod.rs` | +5 | — | `mod intelligence;` + `#[cfg(test)] mod admin_intelligence_tests;` + 15-symbol re-export |

**1,177 LOC ported, 0 LOC dropped, 5 LOC added in `mod.rs`.** OLD's test file uses the exact name `admin_intelligence_tests.rs` (not `intelligence_tests.rs`) — mirrored verbatim in NEW per instruction.

Confirmation: **the body of `intelligence.rs` received only 4 renames, no logic edits.** Every `if`, every `matches!`, every ordering, every branch is byte-identical to OLD aside from the `ty`→`field_type` token swap. The test file received only the 4 fixture struct-literal patches; no assertion, no test body, no helper logic was changed.

## Tests ported

All 43 tests from `OLD/admin/admin_intelligence_tests.rs` pass against NEW on first run:

```
$ cargo test --workspace -p rustio-core --lib intelligence 2>&1 | tail
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 233 filtered out; finished in 0.00s
```

## Test count in the full suite

| | Tests passing (sandbox) | Ignored (PG-gated) | Delta |
|---|---:|---:|---|
| Phase 3 baseline (`16a692f`) | 225 | 8 | — |
| **Phase 4 (`fc94878`)** | **268** | **8** | **+43** |

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 268 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## Verification (paste-proof)

```
$ cargo check --workspace --all-targets              → clean, 0 warnings
$ cargo test --workspace -p rustio-core --lib intelligence  → 43 passed, 0 failed
$ cargo clippy --workspace --all-targets -- -D warnings     → clean
$ cargo test --workspace                             → 268 passed, 0 failed, 8 ignored
```

## Footnote: `context_global()` is a filesystem read in disguise

File `rustio-core/src/admin/intelligence.rs:51–59`:

```rust
pub fn context_global() -> Option<&'static ContextConfig> {
    static CTX: OnceLock<Option<ContextConfig>> = OnceLock::new();
    CTX.get_or_init(|| {
        let text = std::fs::read_to_string("rustio.context.json").ok()?;
        serde_json::from_str(&text).ok()
    })
    .as_ref()
}
```

This contradicts the module's own top-of-file doc-comment: *"Nothing in this module touches the filesystem, the database, or produces HTML."* The function reads `rustio.context.json` from the current working directory, caches it in a `OnceLock` for the process lifetime, and returns `Option<&'static ContextConfig>`. Both failure modes (file missing, JSON malformed) collapse to `None` silently — no log, no error path.

**Why it's here (reading OLD):** callers of `classify_field`/`field_ui_metadata`/`infer_filters` take `Option<&ContextConfig>`. `context_global()` is the convenience constructor that says "just pick up whatever `rustio.context.json` the project ships." In OLD's `admin.rs` it was called once per request handler to thread the context through.

**Not blocking Phase 4.** The tests use explicit `ContextConfig` fixtures, never `context_global()` — so zero test coverage depends on the filesystem, and the contradiction only shows up for a runtime caller that passes `context_global()` as the context argument. NEW has no such runtime caller yet (admin.rs handlers not ported). The function is ported as-is to preserve the `OLD::admin::intelligence::context_global` API contract.

**Future cleanup (not this phase):** when admin.rs is ported (Phase 5+), move the file-read out to where it belongs — either into `ContextConfig::load(path)` + constructor injection in the handler, or into a once-at-startup step during `Admin::new`/`register_admin_routes`. The intelligence module should then take only pre-resolved `Option<&ContextConfig>` with no fallback, and the module's doc-comment becomes honest again.

## Is the intelligence module complete?

**For a classification/rendering-hint layer, yes.** Every decision the future admin renderer needs is in place:
- Role classification (`classify_field`) — 13 `FieldRole` variants covering email, phone, URL, image, PII, status, FK, timestamp, bool, numeric, long/short text, personnummer.
- UI metadata (`field_ui_metadata`) — humanized labels, placeholder hints, masking rules.
- Filter inference (`infer_filters`, `infer_filters_with_relations`) — bool toggles, enum dropdowns (capped at `RELATION_FILTER_DROPDOWN_CAP = 500`), date-range pickers, FK selectors resolved via the registry.
- Search-intent classification (`classify_search_for_field`, `classify_search`) — email, phone, personnummer, free-text, relation-target.
- PII masking (`mask_pii`) — applied to email local-parts, phone numbers, personnummer before display.

**What's deliberately not here:** any HTML emission. The renderer (Phase 5/6) calls these to get *structured* hints, then decides how to HTML-render them. The `format_relation_cell` helper returns plain text (`"#42 — display-or-fallback"`), not a link — the link is the renderer's job.

For Phase 4's stated scope ("port the intelligence logic"), the module is **complete and ready** for the next phase to consume.
