# Phase reports

Each `PHASE*.md` is the long-form report for one chronological phase
of the OLD → NEW port. Reports are written at the end of a phase and
land in that phase's final commit (see `CLAUDE.md` for the hygiene
rule).

| File | Phase | Topic |
|---|---|---|
| `PHASE1.md` | 1 | Port `schema.rs` + `ai/` from OLD |
| `PHASE2.md` | 2 | Postgres SQL rewrite (sqlx, ORM) |
| `PHASE2e.md` | 2e | Postgres bring-up errata + smoke fixes |
| `PHASE3.md` | 3 | Auth surface (sessions, password hash) |
| `PHASE4.md` | 4 | Admin module split (types/render/handlers/routes/builtin) |
| `PHASE5a.md` | 5a | Search index + `Searchable` trait + indexer |
| `PHASE5b.md` | 5b | Search UI (HTMX-free vanilla JS, facets, highlights) |
| `PHASE6a.md` | 6a | Built-in users admin pages |
| `PHASE6b.md` | 6b | Built-in groups admin pages + permission inheritance |
| `PHASE7a-0.md` | 7a/0 | `SiteBranding` API |
| `PHASE7a-0_5.md` | 7a/0.5 | Authorization, demo users, view-first navigation (12 commits) |

Read the latest report (`PHASE7a-0_5.md`) for the current state of
the framework. Earlier reports stay accurate as a record of how the
codebase got to where it is, not as live documentation of how it
behaves now.
