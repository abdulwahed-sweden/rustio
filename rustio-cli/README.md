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

> Project name: mysite
> Choose a starting preset:
    Basic — empty project, add apps later
  › Blog  — scaffolds a posts app with admin + views
    API   — scaffolds an items app with admin + views
> Proceed? (Y/n)
```

Then:

```bash
cd mysite
rustio migrate apply
rustio run
```

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/). The admin lives at `/admin` — send `Authorization: Bearer dev-admin`.

## Non-interactive

Skip the wizard by passing a name (and optionally a preset):

```bash
rustio init mysite                  # basic preset
rustio init mysite --preset blog    # scaffold a posts app too
rustio init mysite --preset api     # scaffold an items app too
```

## Commands

| Command                         | What it does                                                         |
| ------------------------------- | -------------------------------------------------------------------- |
| `rustio init`                   | Interactive wizard: name + preset + confirm                          |
| `rustio init <name>`            | Non-interactive scaffold (default preset: `basic`)                   |
| `rustio init <name> --preset P` | Non-interactive with a preset (`basic` / `blog` / `api`)             |
| `rustio new project <name>`     | Create a new project directly (no wizard)                            |
| `rustio new app <name>`         | Scaffold an app inside the current project                           |
| `rustio migrate generate <n>`   | Create a new migration file                                          |
| `rustio migrate apply [-v]`     | Apply pending migrations (`-v` prints each statement)                |
| `rustio migrate status`         | Show applied and pending migrations                                  |
| `rustio run`                    | Build and run the project in the current directory                   |
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
