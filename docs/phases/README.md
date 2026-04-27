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
| `PHASE7a-2.md` | 7a/2 | Admin redesign — Tailwind build pipeline + sidebar + Inter + lucide icons (5 commits) |
| `PHASE1-ux.md` | 1 (UX) | Admin UX stabilization — auto timestamps, form polish, empty states |

The next chronological block — Phases 2 (design system foundation),
3 (token migration sweep, six sub-phases), 4 (UI consistency polish),
5 (a/c/d — dynamic list + smart field-to-UI mapping + enum/relation
selects), 6 (layout intelligence), 6.2 (unify bespoke forms onto
`FormField`), 7 (audit fixes), 7.1 (real FK / M2M data), 7.2
(searchable + truncated FK selects), 7.3 (remote-search endpoint) —
ships **without** a separate `PHASE*.md` per sub-phase. Each
sub-phase's commit message is the authoritative report; `git log
--oneline` is the index. `CHANGELOG.md`'s Unreleased section rolls
the headlines up.

Read the latest dedicated report (`PHASE7a-2.md`) plus the Unreleased
CHANGELOG entry for the current state of the framework. Earlier
reports stay accurate as a record of how the codebase got to where
it is, not as live documentation of how it behaves now.
