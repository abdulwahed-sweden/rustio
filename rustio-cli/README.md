# rustio-cli

The `rustio` binary — developer CLI for the [RustIO](https://github.com/abdulwahed-sweden/rustio) web framework.

## Install

```bash
cargo install rustio-cli
```

## Quick start

```bash
rustio init readlist
cd readlist
```

`rustio init <name>` scaffolds a Rust project and prints the next commands.
The project is empty; you add models one at a time:

```bash
rustio add model books
```

```text
✔ Created model Book

  file       models/books/models.rs
  migration  migrations/0001_create_books.sql
  admin      /admin/books

  Default fields: title (String), priority (i32), is_active (bool)
  Add more with:  rustio change "add <field> as <Type> to Book"

  → rustio migrate apply
```

Then bring the project up:

```bash
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
rustio run
```

Open <http://127.0.0.1:8000/admin> and sign in. That's the whole loop.

## Change something later

Once your project is running, describe the change in plain English:

```bash
rustio change "add a status field to tasks"
```

RustIO proposes the diff as a small blueprint, shows you the risk, and lets you pick **Apply** / **Show technical details** / **Cancel**. On Apply, it writes the model edit + a migration; you then run `rustio migrate apply` to bring the DB up to date.

The planner expresses changes inside a fixed vocabulary (add field, rename field, add relation, change type, …). If your request doesn't fit, it **refuses** rather than guessing.

## Non-interactive

Pass the project name upfront to skip the prompt:

```bash
rustio init readlist                                  # empty project
rustio init readlist --model books                    # plus one model
```

## Common commands

For a small day-one surface, run `rustio help`. The everyday loop:

| Command                          | What it does                                                         |
| -------------------------------- | -------------------------------------------------------------------- |
| `rustio init [name]`             | Scaffold an empty project                                            |
| `rustio add model <name>`        | Add one model to the current project                                 |
| `rustio run`                     | Build (cargo build) + start the server on `:8000`                    |
| `rustio change "<request>"`      | Describe a change in plain English — RustIO proposes the diff        |
| `rustio migrate apply [-v]`      | Apply pending migrations                                             |
| `rustio migrate status`          | Show applied and pending migrations                                  |
| `rustio user create [opts]`      | Create a user in the auth tables (interactive when flags omitted)    |
| `rustio doctor`                  | Health-check the current project                                     |
| `rustio explain <topic>`         | Inline docs on a concept (`model`, `migration`, `admin`, …)          |
| `<any> --why`                    | Print a one-paragraph "what does this do" without running it         |

For the lower-level scripting / CI surface (`ai plan / review / apply / validate`), the legacy 0.8.x FK retrofit, schema regeneration, and context inspection, see:

```bash
rustio help advanced
```

## Environment

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`.
- `NO_COLOR` — disable coloured CLI output. The wizard honours this automatically.
- `RUSTIO_CORE_PATH` — use a local `rustio-core` path in generated projects (for RustIO contributors).

## Notes

- The name prompt needs a real terminal. In CI or when stdin is piped, pass the name explicitly: `rustio init mysite`.
- Presets are coarse starting points, not lock-in. You can always add more with `rustio add model <name>` or change the shape with `rustio change "<request>"`.

See the [main repository](https://github.com/abdulwahed-sweden/rustio) for the full guide.

## License

MIT
