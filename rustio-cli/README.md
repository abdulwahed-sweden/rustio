# rustio-cli

The `rustio` binary — developer CLI for the [RustIO](https://github.com/abdulwahed-sweden/rustio) web framework.

## Install

    cargo install rustio-cli

## Quick start

    rustio new project mysite
    cd mysite
    rustio new app blog
    rustio migrate apply
    rustio run

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/).

## Commands

    rustio new project <name>     # scaffold a new project
    rustio new app <name>         # scaffold an app in the current project
    rustio migrate generate <n>   # create a new migration
    rustio migrate apply          # apply pending migrations
    rustio migrate status         # list applied + pending migrations
    rustio run                    # build and run the current project

## Environment

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`.
- `NO_COLOR` — disable colored output.
- `RUSTIO_CORE_PATH` — use a local `rustio-core` path in generated projects (for RustIO contributors).

See the [main repository](https://github.com/abdulwahed-sweden/rustio) for the full guide.

## License

MIT
