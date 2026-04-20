# rustio-cli

The `rustio` binary — developer CLI for the [RustIO](https://github.com/abdulwahed-sweden/rustio) web framework.

## Install

```bash
cargo install rustio-cli
```

## Quick start

```bash
rustio init
```

`rustio init` launches an interactive wizard:

```text
  RustIO
  Let's set up your project.

> Project name: readlist
> Choose a starting preset:
    Basic — empty project, add apps later
  › Blog  — scaffolds one app with admin + views
    API   — scaffolds one app with admin + views
> What should your first model track? books
> Proceed? (Y/n)
```

Then:

```bash
cd readlist
rustio migrate apply
rustio user create --email you@example.com --password secret --role admin
rustio run
```

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/). The admin lives at `/admin` — sign in with the email + password you created.

## Non-interactive

Skip the wizard by passing a name (and optionally a preset or a custom app name):

```bash
rustio init readlist                                  # basic preset
rustio init readlist --preset blog                    # default app: posts
rustio init readlist --preset blog --app books        # custom app name
```

## Commands

| Command                         | What it does                                                         |
| ------------------------------- | -------------------------------------------------------------------- |
| `rustio init`                   | Interactive wizard: name + preset + confirm                          |
| `rustio init <name>`            | Non-interactive scaffold (default preset: `basic`)                   |
| `rustio init <name> --preset P` | Non-interactive with a preset (`basic` / `blog` / `api`)             |
| `rustio init <name> --app X`    | Override the scaffolded app name (e.g. `books`, `tasks`, `links`)    |
| `rustio new project <name>`     | Create a new project directly (no wizard)                            |
| `rustio new app <name>`         | Scaffold an app inside the current project                           |
| `rustio migrate generate <n>`   | Create a new migration file                                          |
| `rustio migrate apply [-v]`     | Apply pending migrations (`-v` prints each statement)                |
| `rustio migrate status`         | Show applied and pending migrations                                  |
| `rustio schema`                 | Write `rustio.schema.json` from the compiled admin                   |
| `rustio run`                    | Build and run the project in the current directory                   |
| `rustio user create ...`        | Create a real user in the auth tables                                |
| `rustio ai plan "<prompt>" [--save <path>]` | Plan a schema change; optionally save a reviewable document |
| `rustio ai review <path>`       | Review a saved plan against the current schema                       |
| `rustio ai validate <path>`     | Terse validate-only gate for CI                                      |
| `rustio ai apply <path> [--yes]` | Apply a reviewed plan (writes files, never runs migrations)         |
| `rustio context show`           | Show parsed `rustio.context.json` + inferred flags (GDPR, PII, …)    |
| `rustio context validate`       | Parse context; exit 0 on success                                     |
| `rustio --version`              | Print the CLI version                                                |

## Environment

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`.
- `NO_COLOR` — disable colored CLI output. The wizard honors this automatically.
- `RUSTIO_CORE_PATH` — use a local `rustio-core` path in generated projects (for RustIO contributors).

## Notes

- `rustio init` needs a real terminal. In CI or when stdin is piped, pass a name explicitly: `rustio init mysite [--preset …]`.
- Presets are coarse starting points, not lock-in. Every preset is "a project plus N apps" — you can always add more with `rustio new app <name>`.

See the [main repository](https://github.com/abdulwahed-sweden/rustio) for the full guide.

## License

MIT
