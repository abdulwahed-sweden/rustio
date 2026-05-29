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

`rustio init <name>` scaffolds a Rust project and opens the setup menu — a small Guided / Manual / Import picker. Pick **Guided** and describe what you're building in one sentence:

```text
  How would you like to begin?
  › Guided — I'll propose a starting shape and walk it with you
    Manual — I'll get out of the way; you add models one at a time
    Import — read an existing rustio.schema.json (coming soon)

  ? What are you building?
  › a small clinic with patients and appointments

  I read this as a `clinic` project.
  Here's what I'd suggest:
    1. Patient      name, date_of_birth, phone
    2. Doctor       name, specialty
    3. Appointment  patient_id, doctor_id, scheduled_for, notes
```

RustIO walks each model with one keystroke (`add` / `skip`) and shows a system-blueprint summary before any file is written. The technical view (typed plan operations, risk classification, warnings) lives behind a *"Show technical details"* choice — you decide what lands.

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
rustio evolve "add a status field to tasks"
```

RustIO proposes the diff as a small blueprint, shows you the risk, and lets you pick **Apply** / **Show technical details** / **Cancel**. On Apply, it writes the model edit + a migration; you then run `rustio migrate apply` to bring the DB up to date.

The planner expresses changes inside a fixed vocabulary (add field, rename field, add relation, change type, …). If your request doesn't fit, it **refuses** rather than guessing.

## Non-interactive

Skip the menu by passing the preset and app upfront:

```bash
rustio init readlist --preset blog                    # default app: posts
rustio init readlist --preset blog --app books        # custom app name
```

## Common commands

For a small day-one surface, run `rustio help`. The everyday loop:

| Command                          | What it does                                                         |
| -------------------------------- | -------------------------------------------------------------------- |
| `rustio init [name]`             | Scaffold a project + open the setup menu                             |
| `rustio start`                   | Re-open the setup menu inside an existing project                    |
| `rustio new app <name>`          | Add a new model to the current project                               |
| `rustio run`                     | Build (cargo build) + start the server on `:8000`                    |
| `rustio evolve "<request>"`      | Describe a change in plain English — RustIO proposes the diff        |
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

- The interactive setup menu needs a real terminal. In CI or when stdin is piped, pass a name + preset explicitly: `rustio init mysite --preset basic`.
- Presets are coarse starting points, not lock-in. You can always add more with `rustio new app <name>` or change the shape with `rustio evolve "<request>"`.

See the [main repository](https://github.com/abdulwahed-sweden/rustio) for the full guide.

## License

MIT
