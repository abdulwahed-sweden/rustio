# medflow — RustIO healthcare stress-test example

A deliberately uncomfortable test of RustIO's admin under realistic relational
complexity. **Six related models, ~300 seeded rows, six hand-written migrations.**

Long-form write-up: [`docs/stress-test-healthcare.md`](../../docs/stress-test-healthcare.md).
Read that first if you want to understand what this example exists to prove.

## Quick start (from `examples/medflow/`)

```
rustio migrate apply
sqlite3 app.db < seed.sql
rustio user create --email admin@medflow.local --password medflow123 --role admin
rustio run
```

Then open <http://127.0.0.1:8000/admin> and sign in.

If you're running against the in-tree `rustio-core` (not a crates.io version),
prefix every command with
`RUSTIO_CORE_PATH=$(pwd)/../../rustio-core cargo run --manifest-path ../../rustio-cli/Cargo.toml --`.

## What's in here

| Models | Apps | Migrations | Rows seeded |
|---|---|---|---|
| `Department · Doctor · Patient` | `people` | `0001`, `0002`, `0003` | 8 + 10 + 40 |
| `Appointment · Prescription` | `care` | `0004`, `0005` | 120 + 60 |
| `Invoice` | `billing` | `0006` | 40 |

## Re-seeding

`seed.sql` is not idempotent — running it twice hits unique constraints.
Rebuild clean:

```
rm app.db app.db-shm app.db-wal
rustio migrate apply
sqlite3 app.db < seed.sql
rustio user create --email admin@medflow.local --password medflow123 --role admin
```

## Known limitations

See §7 of [`docs/stress-test-healthcare.md`](../../docs/stress-test-healthcare.md) —
the entire section is a list of things this example was built to expose.
