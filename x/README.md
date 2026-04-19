# x

A [RustIO](https://github.com/abdulwahed-sweden/rustio) project.

## Run it

    rustio migrate apply      # apply schema changes
    rustio run                # build and start the server on :8000

## Commands

    rustio new app <name>         # scaffold an app inside this project
    rustio migrate generate <n>   # create an empty migration file
    rustio migrate apply [-v]     # apply pending migrations
    rustio migrate status         # show applied + pending
    rustio run                    # build and run the server
    rustio --version              # print CLI version

## Layout

- `main.rs` — entry point (RustIO uses a top-level `main.rs` by convention)
- `apps/` — one directory per app (models, views, admin)
- `migrations/` — SQL migrations, applied in filename order
- `static/`, `templates/` — asset directories
- `app.db` — default SQLite database (gitignored)

## Configuration

- `RUSTIO_DATABASE_URL` — override the default `sqlite://app.db?mode=rwc`
- `NO_COLOR` — disable colored CLI output

## Default auth (dev only)

Replace before deploying.

- `Authorization: Bearer dev-admin` — admin access
- `Authorization: Bearer dev-user` — non-admin
