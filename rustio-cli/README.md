# rustio-cli

The `rustio` command-line tool: scaffold projects, build & run them,
apply migrations, manage users / groups / permissions, and use the
AI planner against your schemas.

Install:

```
cargo install rustio-cli
```

Quick path from zero:

```
rustio new project myapp
cd myapp
cp .env.example .env       # edit DATABASE_URL if needed
rustio run                 # convenience wrapper around `cargo run`
```

- Framework overview: [repo README](https://github.com/abdulwahed-sweden/rustio#readme)
- Schema gallery: [`examples/`](https://github.com/abdulwahed-sweden/rustio/tree/main/examples)
- Cross-cutting rules: [`examples/CONVENTIONS.md`](https://github.com/abdulwahed-sweden/rustio/blob/main/examples/CONVENTIONS.md)
