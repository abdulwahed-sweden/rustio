# Phase 5a — Port `admin/suggestions.rs` from OLD

Built on top of Phase 4 (commit `fc94878`).

## Commits shipped

```
HEAD     phase 5a/2: port suggestions_tests.rs with AdminEntry::for_testing stub + Phase 4 fixture patches   ← this commit
7457bf3  phase 5a/1: port suggestions.rs core functions (stub *_from_entries)
fc94878  phase 4b: port intelligence_tests.rs with NEW's AdminField shape
```

Split so the library port (5a/1) reviews independently of the test + infra commit (5a/2) that introduces `AdminEntry::for_testing`.

## Per-item port manifest

| # | Item | Lines | Returns / does | Verdict | How it landed |
|---|---|---:|---|---|---|
| enum | `Confidence` (+ `as_str`, `pill_class`) | 22 | `&'static str` | PORT | verbatim |
| struct | `Suggestion` (+ `url_path`) | 39 | type / `String` | PORT | verbatim |
| fn | `derive_suggestions(&[AdminEntry], Option<&ContextConfig>)` | 50 | `Vec<Suggestion>` | PORT | verbatim |
| fn | `find_suggestion(…)` | 10 | `Option<Suggestion>` | PORT | verbatim |
| fn | `derive_suggestions_from_entries(&[DynamicAdminEntry], …)` | 41 | `Vec<Suggestion>` | **STUBBED** | body replaced with `Vec::new()`; TODO comment; signature unchanged |
| fn | `find_suggestion_from_entries(…)` | 10 | `Option<Suggestion>` | **STUBBED** | body replaced with `None`; TODO comment; signature unchanged |
| fn | `derive_relation_suggestions(&Schema)` | 58 | `Vec<Suggestion>` | PORT | verbatim |
| fn | `find_relation_suggestion(&Schema, …)` | 9 | `Option<Suggestion>` | PORT | verbatim |

**8 items: 6 PORT verbatim, 2 STUBBED, 0 DROP.**

## Two mechanical changes applied during the port

### 1. Phase 4-style `AdminField` fixture patch in the test file

OLD's test file constructs 9 `AdminField {}` literals across three `const` slices (`APPLICANT_FIELDS`, `FULLY_COVERED_FIELDS`, `WIDGET_FIELDS`). Each literal was rewritten:

- `ty:` → `field_type:`
- `nullable: false,` removed
- `label: "<name>",` added (mirrors the `name` string — `intelligence.rs`/`suggestions.rs` never read `AdminField.label`, so the content is semantically inert)

The 8 `SchemaField` literals and the 1 `DynamicAdminField` literal in the same file were left untouched — `SchemaField` is unchanged between OLD and NEW, and `DynamicAdminField` is defined locally in the placeholder module with OLD's exact shape.

### 2. `AdminEntry::for_testing` + `PanicOps` infrastructure

NEW's `AdminEntry` has two extra `pub(crate)` fields not present in OLD:

| Field | OLD `admin.rs:152` | NEW `admin/types.rs:92` |
|---|---|---|
| `ops: Arc<dyn AdminOps>` | — | present |
| `search_hook: Option<Arc<dyn SearchHook>>` | — | present |

OLD's test helpers (`applicant_entry`, `fully_covered_entry`, `widget_entry`, `core_user_entry`) built `AdminEntry {}` literals directly — impossible in NEW because the pub(crate) fields need concrete values. Added to `rustio-core/src/admin/types.rs`:

```rust
#[cfg(test)]
impl AdminEntry {
    pub(crate) fn for_testing(
        admin_name: &'static str, display_name: &'static str,
        singular_name: &'static str, table: &'static str,
        fields: &'static [AdminField], core: bool,
    ) -> Self {
        Self { admin_name, display_name, singular_name, table, fields, core,
               ops: Arc::new(PanicOps), search_hook: None }
    }
}

#[cfg(test)]
struct PanicOps;

#[cfg(test)]
const PANIC_MSG: &str =
    "PanicOps is test-only; if you hit this, a test is using AdminEntry for CRUD, which is wrong — use a real Model";

#[cfg(test)]
impl AdminOps for PanicOps {
    // 6 trait methods, each: Box::pin(async { unreachable!("{PANIC_MSG}") })
}
```

**LOC of new test-only infrastructure**: 88 lines in `types.rs` (1 `impl AdminEntry` block, 1 struct, 1 const, 1 `impl AdminOps` block with 6 methods). All `#[cfg(test)]`-gated — zero production impact. Release builds are byte-identical to Phase 4.

The 4 fixture helpers in `suggestions_tests.rs` were rewritten from struct literals to `AdminEntry::for_testing(...)` calls — one substitution per helper.

## Placeholder module: `admin/entry_builder.rs`

Created at `rustio-core/src/admin/entry_builder.rs` (42 LOC). Stubs:

- `pub struct DynamicAdminEntry` with 6 fields matching OLD
- `pub struct DynamicAdminField` with 4 fields matching OLD (`ty: FieldType`, `nullable: bool` kept — `DynamicAdminField` is a separate type from `AdminField` and its shape isn't coupled to NEW's `AdminField` divergence)
- `pub fn build_admin_entries(_schema: &Schema) -> Vec<DynamicAdminEntry>` — returns empty

Gated with module-level `#![allow(dead_code)]` — placeholder has no real callers until the full port lands, so rustc would otherwise flag every symbol. The attribute is deliberately scoped tight: only this one placeholder module is affected; the rest of the crate still runs under `-D warnings`.

## Re-exports in `admin/mod.rs`

```rust
pub use suggestions::{
    derive_relation_suggestions, derive_suggestions, derive_suggestions_from_entries,
    find_relation_suggestion, find_suggestion, find_suggestion_from_entries, Confidence, Suggestion,
};
```

All 6 `pub fn`s + 2 public types. `entry_builder` module itself is not re-exported from `admin/mod.rs` — it's accessed only through `super::entry_builder::*` in `suggestions_tests.rs`. Keeping it out of the crate-public surface avoids committing the placeholder types to the API; they're free to change when the real port lands.

## LOC moved

| | Lines | Source | Destination |
|---|---:|---|---|
| `suggestions.rs` | 316 | `OLD/rustio-core/src/admin/suggestions.rs` | `NEW/rustio-core/src/admin/suggestions.rs` (-47 lines from stubbing 2 fn bodies; +16 lines of TODO comments = net -31) |
| `suggestions_tests.rs` | 589 | `OLD/rustio-core/src/admin/suggestions_tests.rs` | `NEW/rustio-core/src/admin/suggestions_tests.rs` (+18 lines of `#[ignore]` attributes × 5 tests + 5 `= "..."` messages; 9 AdminField literal patches; 4 AdminEntry helpers rewritten) |
| `entry_builder.rs` | 0 → 42 | (new placeholder) | `NEW/rustio-core/src/admin/entry_builder.rs` |
| `types.rs` | +88 | — | `PanicOps` + `AdminEntry::for_testing` |
| `admin/mod.rs` | +10 | — | `mod entry_builder;` + `mod suggestions;` + `#[cfg(test)] mod suggestions_tests;` + 8-symbol re-export |

**905 LOC ported, 0 LOC dropped from OLD's two files.** 130 LOC added net (placeholder + `for_testing` + mod wiring). No logic from OLD was changed — only the 2 stubs (body replaced, signature identical) and the fixture shape patches.

## Test count in the full suite

| | Tests passing (sandbox) | Ignored | Delta |
|---|---:|---:|---|
| Phase 4 baseline (`fc94878`) | 268 | 8 | — |
| **Phase 5a (`d6b38b1`)** | **286** | **13** | **+18 active, +5 ignored** |

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 286 passed; 0 failed; 13 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
```

Phase plan predicted 285 + 14 ignored (17 active + 6 ignored additions). Actual split is 18 active + 5 ignored — my inspection-phase count was the right one (23 tests; 10 AdminEntry-based + 8 Schema-only relation = 18 portable; 5 DynamicAdminEntry-dependent = ignored).

## Verification (paste-proof)

```
$ cargo check --workspace --all-targets              → clean, 0 warnings
$ cargo test --workspace -p rustio-core --lib suggestions
  → 18 passed, 0 failed, 5 ignored
$ cargo clippy --workspace --all-targets -- -D warnings  → clean
$ cargo test --workspace 2>&1 | grep "^test result"
  → 286 passed, 0 failed, 13 ignored
```

## Why the 5 `#[ignore]`d tests stay ignored

All 5 depend on `build_admin_entries(&Schema) -> Vec<DynamicAdminEntry>`, whose real body reads `SchemaModel` / `SchemaField` and constructs runtime `DynamicAdminEntry` values. The placeholder returns empty, so:

| Test | Would fail because |
|---|---|
| `schema_driven_suggestion_fires_for_missing_field` | asserts `ss.len() == 1`; stub gives 0 |
| `schema_driven_suggestion_disappears_when_field_present` | asserts `before.len() == 1`; stub gives 0 |
| `schema_driven_and_compile_time_derivations_agree_when_shapes_match` | compares `dyn_ss[0]` against `compile_ss[0]`; stub produces empty `dyn_ss`, index panic |
| `schema_driven_find_rejects_crafted_urls` | asserts `find_suggestion_from_entries(..., "annual_income").is_some()`; stub returns `None` |
| `schema_driven_skips_core_models` | constructs `DynamicAdminEntry { .. }` literal + calls stubbed function; would trivially pass (empty == empty) but the test's intent is inverted — passing on the stub means nothing. Kept ignored for honesty. |

## Future unblocking

| Landing this | Re-enables |
|---|---|
| Port full `entry_builder.rs` body from OLD (real `build_admin_entries`, `DynamicAdminEntry::from_schema_model`) | All 5 `#[ignore]`d tests + body un-stub of `derive_suggestions_from_entries` + `find_suggestion_from_entries` |
| Same phase: drop `#![allow(dead_code)]` from `entry_builder.rs` once real callers exist | dead-code safety returns |

Scope is a single self-contained phase (call it Phase 5c or similar) once Phase 5b (audit.rs) lands. No cross-file ripple expected — the stubs' signatures already match OLD's.

## Confirmation

- **No logic from OLD was rewritten.** The 6 PORT items are byte-identical to OLD. The 2 STUBBED items have their bodies replaced with `Vec::new()` / `None` + TODO comments; signatures are unchanged.
- **No HTML, no SQL, no filesystem, no DB** — suggestions.rs is pure.
- **The `PanicOps` / `AdminEntry::for_testing` additions are `#[cfg(test)]`-gated.** Release builds remain identical to Phase 4. `cargo clippy --all-targets -- -D warnings` stays clean.
