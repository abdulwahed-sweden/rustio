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

Commit messages: short, imperative, present tense, optionally prefixed with the affected area:

```text
feat(admin): add search filter
fix(migrations): handle BOM in .sql files
docs: clarify RUSTIO_DATABASE_URL behavior
```

## Signed commits

Every commit that lands on `main` must carry a signature GitHub can verify — the `Protect main` ruleset on the repo enforces it. You can sign with either SSH or GPG; the rule is signature-format-agnostic.

The SSH path is the simpler one — it reuses your existing SSH key:

```bash
# 1. Tell git to use SSH-based signing, point at your public key,
#    and sign every commit + tag automatically.
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519.pub   # or whichever key you use
git config --global commit.gpgsign true
git config --global tag.gpgsign true

# 2. Register the SAME public key on GitHub as a *Signing Key*
#    (this is a separate slot from your auth key — even if the key
#    material is identical, it has to be added under "Signing Key").
gh auth refresh -s admin:ssh_signing_key                    # one-time scope grant
gh ssh-key add ~/.ssh/id_ed25519.pub --title "<host> signing" --type signing
# or via the web UI: https://github.com/settings/ssh/new

# 3. Set up local signature verification so `git log` shows green
#    instead of erroring on the SSH format.
mkdir -p ~/.config/git
printf 'your-github-email@example.com %s\n' "$(cat ~/.ssh/id_ed25519.pub)" \
  > ~/.config/git/allowed_signers
git config --global gpg.ssh.allowedSignersFile ~/.config/git/allowed_signers
```

Verify the setup:

```bash
git log --format='%h  %G?  %s' -1
# Expect: G in the middle column on a fresh commit. N = unsigned;
#         E = error (key file wrong / not registered on GitHub).
```

If you have GPG already configured for another project, that works too — just leave `gpg.format` unset (or set to `openpgp`) and make sure your public key is uploaded under `https://github.com/settings/gpg/new`. The rule only checks "did GitHub verify it", not which format you used.

History is **not** rewritten: pre-existing unsigned commits stay as-is. Only commits being pushed going forward need signing. See the entry under `[Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md) for the policy change record.

## Workspace layout

- `rustio-core/` — runtime library (HTTP, router, middleware, context, errors, auth, ORM, admin, migrations).
- `rustio-cli/` — the `rustio` binary (scaffolding, migrations, run).
- `rustio-macros/` — procedural macros (`#[derive(RustioAdmin)]`).

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
