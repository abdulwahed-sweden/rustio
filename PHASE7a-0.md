# Phase 7a/0 — `SiteBranding` API in rustio-core

Built on top of Phase 6b (commit `99a9a9e`) — the user/group creation flows.

## Commits shipped

```
TBD-final   phase 7a/0: SiteBranding API report                                                       ← this commit
b014522     phase 7a/0d: blog example calls .site_branding() explicitly to demo the API
f549fff     phase 7a/0c: templates render SiteBranding fields instead of hardcoded strings
9812e9e     phase 7a/0b: thread SiteBranding into BaseContext (4 fields, 14 call sites)
dff82a6     phase 7a/0a: add SiteBranding struct + Admin::site_branding() builder
```

## Step 1 audit (recap)

Five hardcoded admin-branding sites in `rustio-core`:

| Location | Was | Becomes |
|---|---|---|
| `src/admin/render.rs:47` (BaseContext::new default) | `"RustIO administration"` | `branding.site_title` |
| `src/admin/render.rs:48` (BaseContext::new default) | `"RustIO administration"` | `branding.site_header` |
| `src/admin/render.rs:194` (`dashboard_ctx`) | `"Site administration"` | `branding.index_title` |
| `assets/templates/admin/base.html:50` (footer) | `"RustIO administration — single binary, plain HTML, system fonts."` | `{{ footer_copyright }}` (default `RustIO {VERSION}`) |
| `assets/templates/admin/login.html:15` (card h2) | `"RustIO administration"` | `{{ site_header }}` |

The "single binary, plain HTML, system fonts" tagline was RustIO-specific marketing — dropped per spec. Default footer is now `RustIO {CARGO_PKG_VERSION}`.

## API surface added

```rust
// rustio_core::admin
pub struct SiteBranding {
    pub site_title: String,
    pub site_header: String,
    pub index_title: String,
    pub footer_copyright: String,
}

impl Default for SiteBranding {
    fn default() -> Self {
        Self {
            site_title: "RustIO administration".into(),
            site_header: "RustIO administration".into(),
            index_title: "Site administration".into(),
            footer_copyright: format!("RustIO {}", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl Admin {
    pub fn site_branding(mut self, branding: SiteBranding) -> Self { ... }
    pub fn branding(&self) -> &SiteBranding { ... }
}
```

Re-exported from `rustio_core::admin::*` so projects can `use rustio_core::admin::SiteBranding`.

## Internal refactor — `BaseContext::new` signature change

**Old:**
```rust
BaseContext::new(identity: Option<&Identity>, csrf_token: String) -> Self
// hardcoded "RustIO administration" defaults inside
```

**New:**
```rust
BaseContext::new(identity: Option<&Identity>, csrf_token: String, admin: &Admin) -> Self
// reads admin.branding() internally
```

Plus `BaseContext` gained two fields: `index_title: String` and `footer_copyright: String`. `site_title` / `site_header` upgraded from `&'static str` to `String`.

**14 call sites updated**:

- `handlers.rs` (8): login×2, dashboard, list_model, show_new_form×2, show_edit_form×2, show_delete_confirm, show_object_history, show_log_entries, show_password_change×2
- `builtin.rs` (8): list_users, show_user_edit, show_new_user×2, list_groups, show_group_edit, show_new_group×2

Three context-builder fns also updated to take `&Admin` instead of `&[AdminEntry]`:
- `dashboard_ctx(identity, &Admin, recent, csrf)` — was `&[AdminEntry]`
- `list_ctx(identity, &Admin, entry, ...)` — was `&[AdminEntry]`
- `form_ctx(identity, &Admin, entry, ...)` — was `&[AdminEntry]`
- `confirm_delete_ctx(identity, &Admin, entry, ...)` — was `&[AdminEntry]`

These now do `admin.entries().iter().map(SidebarEntry::from)` internally — same data, less arg threading. The `&Admin` arg also gets the constructor branding for free.

## Future-proofing rationale

Taking `&Admin` (not just `&SiteBranding`) means Phase 8/9 can pull more from `Admin` into `BaseContext` (locale, theme tokens, feature flags) **without** re-touching the 14 call sites. The signature is the future-proof choice.

## Test count

| | Tests passing | Ignored | Failed |
|---|---:|---:|---:|
| Phase 6b baseline (`99a9a9e`) | 291 | 21 | 0 |
| **Phase 7a/0 (`b014522`)** | **291** | **21** | **0** |

No regressions. No new tests added — the change is mechanical refactoring with browser-verified output.

## Verification

```
$ cargo test --workspace 2>&1 | grep "^test result"
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 291 passed; 0 failed; 21 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored
test result: ok. 0 passed; 0 failed; 0 ignored

$ cargo clippy --workspace --all-targets -- -D warnings    → clean
$ cargo check --workspace --all-targets                    → clean
```

### Browser smoke (running blog with `RUSTIO_TEMPLATE_DIR=/tmp/none`)

| Surface | Rendered string |
|---|---|
| `<title>` on all pages | `RustIO administration \| RustIO administration` |
| `#header h1` (header brand) | `RustIO administration` |
| `/admin/login` card `<h2>` | `RustIO administration` |
| `/admin` dashboard `<h1>` | `Site administration` (the `index_title`) |
| Footer `<small>` | `RustIO 1.0.0` |

All four branded fields render from the default `SiteBranding::default()`. Custom values flow through identically — Phase 7a/1 (tolkhuset) will set its own.

### Blog example demo

```rust
use rustio_core::admin::{register_admin_routes, Admin, SiteBranding};

let admin = Admin::new()
    .site_branding(SiteBranding::default())   // explicit default = same as omitting
    .model_with_search::<Post>(indexer.clone());
```

The explicit-default call is redundant but visible — projects copy this line and pass their own values:

```rust
let admin = Admin::new()
    .site_branding(SiteBranding {
        site_title: "Tolkhuset administration".into(),
        site_header: "Tolkhuset administration".into(),
        index_title: "Tolkhuset interpreter management".into(),
        footer_copyright: "© 2026 Tolkhuset AB. Powered by RustIO.".into(),
    })
    .model::<…>();
```

## Out of scope

- **Search example templates** (`assets/templates/base.html`, `assets/templates/search.html`) retain hardcoded `RustIO` brand strings. Future phase may introduce `SearchBranding` or extend `SiteBranding` to cover non-admin surfaces.
- **CSS palette overrides per project** — Phase 8/9 if any project wants to swap the rust accent for their own.
- **Logo URL** — Phase 8/9 if a project wants an `<img>` instead of text.
- **i18n strings** — defaults are ASCII English, projects supply their own. No locale switching today.

## Confirmation

- Builder API consistent with `.model()` / `.model_with_search()` — chaining stays natural.
- `String` over `&'static str` — runtime override needs ownership, premature optimization avoided.
- No changes to AI / audit / suggestions / intelligence / relations / migrations.
- `examples/blog/Cargo.toml` unchanged — only `main.rs` touched (1 import line + 4-line builder change).
- Sandbox suite green, clippy `-D warnings` clean.

## Phase 7a/1 onward

The next commit starts the tolkhuset crate skeleton. It will:

1. Use `Admin::new().site_branding(SiteBranding { ... custom Swedish/Tolkhuset values ... })`.
2. Define its first model (likely `Translator` or `Booking`) with `#[derive(RustioAdmin)]`.
3. Provide a `tolkhuset` binary alongside `blog` in the workspace.

The Phase 7a/0 API (this report) is the foundation that makes (1) a single line for tolkhuset.
