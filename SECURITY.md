# Security Policy

## Supported versions

RustIO is pre-1.0. Only the latest `0.x.y` release on crates.io receives security fixes.

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅         |
| < 0.1   | ❌         |

## Reporting a vulnerability

Email **abdulwahed.sweden@gmail.com** with:

- A description of the issue.
- Steps to reproduce, or a minimal proof of concept.
- The affected RustIO version.
- Any disclosure timeline you have in mind.

Please **do not** open a public GitHub issue for vulnerabilities. Expect an acknowledgement within 72 hours.

## Scope

In-scope: issues in `rustio-cli`, `rustio-core`, `rustio-macros` published to crates.io.

Out of scope for 0.x:

- The bundled dev authentication (`dev-admin`, `dev-user`) — it is explicitly not a production auth scheme.
- Default absence of CSRF protection on admin forms — documented limitation; CSRF is planned.
- Leaking `Error::Internal` messages to clients — documented limitation; sanitization is planned.

If you find a hardening gap that falls outside scope, please still report it; we want the full picture even if the fix waits for a future release.

## Credit

With your consent, we credit reporters in the corresponding release notes.
