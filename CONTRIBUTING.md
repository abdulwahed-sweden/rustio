# Contributing to RustIO

Thanks for considering a contribution. This document covers the day-to-day workflow.

## Development setup

```bash
git clone https://github.com/abdulwahed-sweden/rustio.git
cd rustio
cargo build --workspace
cargo test --workspace --all-targets
```

To test the CLI against the local source instead of the crates.io version, set `RUSTIO_CORE_PATH` when generating projects:

```bash
RUSTIO_CORE_PATH=$(pwd)/rustio-core cargo run -p rustio-cli -- new project /tmp/demo
```

## Required checks

Before opening a PR, these must pass locally:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

CI runs the same checks on every push.

## Workspace layout

- `rustio-core/` — runtime library (HTTP, router, middleware, context, errors, auth, ORM, admin, migrations).
- `rustio-cli/` — the `rustio` binary (scaffolding, migrations, run).
- `rustio-macros/` — procedural macros (`#[derive(RustioAdmin)]`).

## Commit messages

Short, imperative, present tense. Prefix with the affected area when helpful:

```
feat(admin): add search filter
fix(migrations): handle BOM in .sql files
docs: clarify RUSTIO_DATABASE_URL behavior
```

## Breaking changes

Pre-1.0, we may ship breaking changes in minor versions (`0.x`). Call them out explicitly in the PR description and update `CHANGELOG.md`.

## Adding tests

- Unit tests live in `#[cfg(test)] mod tests` blocks in the same file as the code they cover.
- Integration tests that need a real DB use `Db::memory()` + `#[tokio::test]`.
- End-to-end CLI tests spin up a scaffolded project and hit it with `curl`; keep them out of `cargo test` unless they're fast.

## Reporting bugs

Use the bug report template. Include `rustio --version`, `rustc --version`, OS, and a minimal reproduction.

## Security

For security issues, see [SECURITY.md](SECURITY.md). Do not open a public issue.

## License

By contributing, you agree that your contributions will be licensed under the MIT license.
