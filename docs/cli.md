# CLI

## Install

```bash
cargo install rustio-cli
```

## Commands

### `rustio <command>`

| Command                          | What it does                                            |
|----------------------------------|---------------------------------------------------------|
| `init [name]`                    | Scaffold a project (wizard with no name, non-interactive with one) |
| `start`                          | Reopen the setup menu (Empty / Template) in a project   |
| `new project <name>`             | Create a new project at `./<name>`                      |
| `add model <name>`               | Create one model inside the current project             |
| `change "<change>"`              | Change the schema from plain English, then offer to migrate |
| `evolve "<change>"`              | Retired spelling of `change`; prints a note, then runs it    |
| `run [--port <n>]`               | Build the project and serve on `http://127.0.0.1:8000`  |
| `migrate generate <name>`        | Create an empty migration file under `migrations/`      |
| `migrate apply`                  | Apply all pending migrations                            |
| `migrate status`                 | List applied and pending migrations                     |
| `schema`                         | Regenerate `rustio.schema.json` from the live admin     |
| `user create`                    | Create a user (interactive when flags are omitted)      |
| `doctor`                         | Health-check the current project and report fixes       |
| `explain <topic>`                | Show inline docs (`model`, `migration`, `ai`, ...)      |
| `ai plan "<change>"`             | Parse a natural-language change request into a typed plan |
| `ai review <path>`               | Print risk, impact, and warnings for a saved plan       |
| `ai apply <path>`                | Apply a reviewed plan (writes files; runs no migrations) |
| `help`                           | Show available commands                                 |

## Examples

```bash
# The whole loop, start to finish
rustio init booklend           # pick Empty
cd booklend
rustio add model book          # repeat for member, loan
rustio migrate apply
rustio change "add author as String to Book"
rustio user create --email you@example.com --password secret --role admin
rustio run
```

```bash
# Change an existing schema through the typed AI pipeline
rustio ai plan "add date_of_birth as DateTime to posts" --save plan.json
rustio ai review plan.json
rustio ai apply plan.json --yes
rustio migrate apply
```

## Flags

- `-h`, `--help`       Show help for any command
- `-V`, `--version`    Print version
- `--why`              Append to any command to print a one-paragraph explanation without running it

## Environment

- `RUSTIO_DATABASE_URL`   Database URL (default: `sqlite://app.db?mode=rwc`)
- `RUSTIO_PORT`           Port the generated `main.rs` binds (default: `8000`; set by `rustio run --port`)
- `RUSTIO_QUIET`          Suppress the project binary's own startup lines (set by `rustio run`)
- `RUSTIO_CORE_PATH`      Override the `rustio-core` path dep in generated `Cargo.toml`
- `NO_COLOR`              Disable coloured CLI output
