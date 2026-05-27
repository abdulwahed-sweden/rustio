# Advanced Docs

> If you're new to RustIO, start at the [main README](../../README.md) first.
> It walks you through your first project in about 5 minutes.

These docs assume you're already comfortable with a basic RustIO project
(`rustio init`, models, migrations, the admin) and want to go deeper.

## What's here

- **[Quickstart (long form)](quickstart.md)** — a 15-minute, opinion-heavy walkthrough that builds the same project the main README covers, but with more detail on every step (why each line is there, what flags exist, where the bodies are buried).
- **[Build a hospital management system](build-hospital-app.md)** — a four-model CRUD example (Departments · Doctors · Patients · Appointments) that exercises relations, FK delete-guards, RBAC, audit logging, and the AI pipeline end-to-end. Treat it as a worked example of "what does a real RustIO system look like."
- **[Healthcare stress test](stress-test-healthcare.md)** — a deliberate stress test of the admin under realistic relational complexity. Useful when you need to understand the framework's limits or design around them.

## What's *not* here yet

- A full API reference for `rustio-core`. Use `cargo doc --open -p rustio-core` for now.
- A migration cookbook. The patterns are stable; what's in `CHANGELOG.md` § 0.9.x is the closest thing.
- Deployment guides. RustIO ships as a regular Rust binary — host it like any other Rust binary.

## See also

- [`README.md`](../../README.md) — the beginner entry point.
- [`docs/glossary.md`](../glossary.md) — plain-English definitions of every framework term.
- [`ROADMAP.md`](../../ROADMAP.md) — the three phases (Foundation / Intelligence / Systems) and where each release fits.
- [`CHANGELOG.md`](../../CHANGELOG.md) — every visible change, version by version.
