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

Phases 2 → 9.1 ship **without** a separate `PHASE*.md` per
sub-phase. Each sub-phase's commit message is the authoritative
report; `git log --oneline` is the index. `CHANGELOG.md` rolls the
headlines up under release tags. The chronology:

| Block | Phases | Commits | Release |
|---|---|---|---|
| Design system foundation | 2 (foundation, fonts, theme alias) | 3 | v1.0-admin |
| Token migration sweep | 3 (a / b-0 / b / c / d / e / f) | 7 | v1.0-admin |
| UI consistency polish | 4 | 1 | v1.0-admin |
| Dynamic UI | 5 (a / c / d) | 3 | v1.0-admin |
| Layout intelligence | 6, 6.2 | 2 | v1.0-admin |
| Audit fixes + FK data | 7, 7.1, 7.2, 7.3 | 4 | v1.0-admin |
| Inline errors + a11y | 7.5 (Path A folds 7.4 plumbing) | 1 | v1.0-admin |
| Production hardening | 7.6 | 1 | v1.0-admin |
| AI generator | 8.0 | 1 | v1.1-ai |
| AI updater | 8.1 | 1 | v1.1-ai |
| AI analyzer | 8.2 | 1 | v1.1-ai |
| Analyze ↔ update bridge | 8.3, 8.3.1, 8.4 | 3 | v1.1-ai |
| AI safety hardening | 9.1 | 1 | v1.1.1 |

Phase 9.0 was a real-world validation run (no commit; produced the
findings that drove 9.1). Phase 7.4 was paused at the audit stage;
its plumbing folded into 7.5/Path A.

Read the latest dedicated report (`PHASE7a-2.md`) plus the
appropriate `CHANGELOG.md` release section for the current state of
the framework. Earlier reports stay accurate as a record of how the
codebase got to where it is, not as live documentation of how it
behaves now.
