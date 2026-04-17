# Security Policy

## Supported versions

RustIO is pre-1.0. Only the latest `0.x.y` release on crates.io receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Reporting a vulnerability

Email **<abdulwahed.sweden@gmail.com>** with:

- A description of the issue.
- Steps to reproduce, or a minimal proof of concept.
- The affected RustIO version.
- Any disclosure timeline you have in mind.

Please **do not** open a public GitHub issue for vulnerabilities. Expect an acknowledgement within 72 hours.

## Scope

In-scope: issues in `rustio-cli`, `rustio-core`, `rustio-macros` published to crates.io.

Out of scope for 0.x:

- The bundled dev authentication (`dev-admin`, `dev-user`) — it is explicitly not a production auth scheme. As of 0.2.2, the built-in `authenticate` middleware refuses to recognize these tokens when `RUSTIO_ENV=production` is set. You still need to register your own auth middleware in that mode.
- CSRF protection on admin forms — see the note below.

If you find a hardening gap that falls outside scope, please still report it; we want the full picture even if the fix waits for a future release.

## CSRF: current threat model

RustIO 0.x admin auth uses `Authorization: Bearer` headers. Browsers **do not** automatically include custom headers on cross-origin requests, so a malicious third-party page cannot forge admin actions on a signed-in user's behalf — CSRF is not directly exploitable in this mode.

When cookie-based session auth lands in a future release, CSRF becomes a real concern. Per-request CSRF tokens on admin forms will ship in the same release.

If you deploy something that stores admin credentials in a cookie or uses same-origin fetch with credentials today, you should add your own CSRF protection.

## Production guard

As of 0.2.2, setting `RUSTIO_ENV=production` (or `RUSTIO_ENV=prod`) disables the built-in `dev-admin` / `dev-user` token mapping. A process that boots in production mode and doesn't register a real auth middleware will return 401 from every admin route. The first production request also emits a one-time stderr warning.

## Credit

With your consent, we credit reporters in the corresponding release notes.
