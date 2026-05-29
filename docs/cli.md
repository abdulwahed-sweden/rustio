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
| `start`                          | Open the setup menu inside an existing project          |
| `new project <name>`             | Create a new project at `./<name>`                      |
| `new app <name>`                 | Create a new app inside the current project             |
| `run`                            | Build the project and serve on `http://127.0.0.1:8000`  |
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
# Scaffold a new project and run it
rustio init mysite --preset blog
cd mysite
rustio migrate apply
rustio run
```

```bash
# Evolve an existing schema through the typed AI pipeline
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
- `RUSTIO_CORE_PATH`      Override the `rustio-core` path dep in generated `Cargo.toml`
- `NO_COLOR`              Disable coloured CLI output
