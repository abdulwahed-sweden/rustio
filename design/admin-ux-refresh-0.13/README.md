# Admin UX Refresh 0.13 — design contract

Frozen design package for the RustIO admin. This directory is the **visual and
UX source of truth** for the refresh. It contains no production code and changes
no behaviour.

Baseline: `main` at `5f43014aef6dfbd69ac350012d2329b5f4e99352`.

## What is here

| File | Holds |
|---|---|
| `TOKENS.md` | Every colour, type and geometry value, and the rules governing them |
| `SPEC.md` | Per-screen structure, components, interaction contract, responsive contract, and the product-scope classification for every proposal |
| `ACCEPTANCE.md` | Screenshot-verifiable criteria, per screen and global |
| `boards/` | Eight standalone HTML design references |

Open `boards/index.html` in a browser. Every board is a single self-contained
file — no build step, no dependency, no shared stylesheet. Copy one anywhere and
it still renders.

## The two sources of truth

**Behaviour is defined by the production code and its tests.** This package is
the visual and UX contract only. Where the two could disagree, the code wins and
the disagreement is documented rather than resolved by changing the code.

This package does **not** define, and must not be read as redefining:

- routes, including `/admin/<model>/new` as the canonical form route and
  `/admin/<model>/create` as the compatibility alias
- authentication, sessions, CSRF
- RBAC and its status codes
- database behaviour, migrations, PRG, CRUD semantics
- unknown-record 404 behaviour on both GET and POST
- ViewSpec semantics: the six roles, the per-layout role sets, the
  never-empty-row fallback, and the declared-relation identity derivation
- the fixed page-size contract
- template override seams

See `SPEC.md` → *Behavioural boundary* for the full list.

## Product-scope classification

Every proposal in this package carries exactly one status.

| Status | Meaning |
|---|---|
| **EXISTING BEHAVIOUR** | Already true of the shipped admin. The design depends on it and does not change it. |
| **VISUAL / UX CHANGE ONLY** | Presentation only. No handler, context, route or query change. |
| **REQUIRES EXISTING BACKEND WIRING** | Uses a capability the runtime already has; needs the context or template wired to it. |
| **FUTURE PRODUCT FEATURE** | Not part of this implementation. Appears only in clearly separated exploration blocks, never on an approved board. |

A future feature never appears as a current implementation target. Where one is
valuable enough to keep in the design record, it is drawn dashed and
desaturated inside an *Exploration — not approved* block and labelled as such.

## Scope of change

Only `design/admin-ux-refresh-0.13/**` is touched by this package. No file under
`rustio-core/`, `rustio-cli/`, `examples/`, `migrations/`, `tests/`, `.github/`
or any Cargo manifest is modified.

## Reading order

1. `TOKENS.md` — the values everything else assumes
2. `boards/index.html` — what the screens look like
3. `SPEC.md` — why, and what each change costs
4. `ACCEPTANCE.md` — how to tell it was implemented correctly
