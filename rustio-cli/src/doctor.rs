//! v1.5.0 — `rustio doctor`: pre-flight diagnostics for first-run setup.
//!
//! Read-only by contract. Five checks run in causal order; downstream
//! checks render as `⏭` when an upstream blocker fails.
//!
//! Order mirrors the scaffold's `MAIN_RS` startup sequence so a green
//! doctor implies `cargo run` will boot:
//!
//!   1. Project root           (filesystem, walk-up via `scaffold::find_project_root`)
//!   2. DATABASE_URL           (process env, after `dotenvy::dotenv()`)
//!   3. PostgreSQL TCP         (TcpStream::connect_timeout, 2s)
//!   4. PostgreSQL connect     (sqlx pool, SELECT 1)
//!   5. Meilisearch reachable  (`MeiliClient::health()`, warning-only)
//!
//! No mutations. No connection retries. No package-manager detection.
//! Anything not on this list is deferred to v1.6+.

use std::io::{IsTerminal, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::scaffold;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Blocker,
    Warning,
    Skipped,
}

#[derive(Debug)]
pub struct CheckResult {
    pub name: &'static str,
    pub severity: Severity,
    /// One-line text rendered to the right of the symbol+name column.
    pub headline: String,
    /// Multi-line block rendered under the headline. Shown for
    /// `Blocker` / `Warning` always, for `Ok` only with `--verbose`,
    /// never for `Skipped`.
    pub detail: Option<String>,
    /// Optional `( <ms>ms )` suffix on the headline for timing-sensitive
    /// checks (TCP, PG connect, Meili).
    pub elapsed_ms: Option<u128>,
}

pub struct Args {
    pub quiet: bool,
    pub verbose: bool,
    pub no_color: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedUrl {
    pub raw: String,
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub dbname: String,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run(args: Args) -> ExitCode {
    if !args.quiet {
        println!();
        println!("  RustIO doctor — checking your environment");
        println!();
    }

    let mut results: Vec<CheckResult> = Vec::with_capacity(5);

    // 1 — project root
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = scaffold::find_project_root(&cwd);
    results.push(check_project_root(&project_root));
    let project_ok = matches!(results.last().unwrap().severity, Severity::Ok);

    if !project_ok {
        // No project context → none of the env/db checks make sense.
        // Render and exit immediately.
        render(&results, &args);
        return ExitCode::from(1);
    }

    // Mirror the scaffold's MAIN_RS: load `.env` from cwd before reading
    // DATABASE_URL. Silent on failure — `.env` is optional.
    let _ = dotenvy::dotenv();

    // 2 — DATABASE_URL
    let raw = std::env::var("DATABASE_URL").ok();
    let (r2, parsed) = check_database_url(raw);
    let url_ok = matches!(r2.severity, Severity::Ok);
    results.push(r2);

    // 3 — PG TCP
    let tcp_result = if !url_ok {
        skipped("PostgreSQL TCP", "DATABASE_URL invalid")
    } else {
        check_pg_tcp(parsed.as_ref().expect("url_ok ⇒ parsed Some"))
    };
    let tcp_ok = matches!(tcp_result.severity, Severity::Ok);
    results.push(tcp_result);

    // 4 — PG connect (depends on URL parse + TCP)
    let connect_result = if !url_ok {
        skipped("PostgreSQL", "DATABASE_URL invalid")
    } else if !tcp_ok {
        skipped("PostgreSQL", "TCP unreachable")
    } else {
        check_pg_connect(parsed.as_ref().unwrap()).await
    };
    results.push(connect_result);

    // 5 — Meili (independent — runs unless project check failed)
    results.push(check_meili().await);

    render(&results, &args);

    let has_blocker = results.iter().any(|r| r.severity == Severity::Blocker);
    if has_blocker {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

// ---------------------------------------------------------------------------
// Check 1 — project root
// ---------------------------------------------------------------------------

fn check_project_root(root: &Option<PathBuf>) -> CheckResult {
    match root {
        Some(p) => CheckResult {
            name: "Project root",
            severity: Severity::Ok,
            headline: format!("{}  ({})", project_name(p), p.display()),
            detail: None,
            elapsed_ms: None,
        },
        None => CheckResult {
            name: "Project root",
            severity: Severity::Blocker,
            headline: "not inside a RustIO project".into(),
            detail: Some(
                "To create one:\n\n  rustio startproject myapp\n  cd myapp\n  rustio doctor"
                    .into(),
            ),
            elapsed_ms: None,
        },
    }
}

fn project_name(root: &Path) -> String {
    if let Ok(toml) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for line in toml.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("name") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let val = rest.trim().trim_matches('"');
                    if !val.is_empty() {
                        return val.to_string();
                    }
                }
            }
        }
    }
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "(unknown)".into())
}

// ---------------------------------------------------------------------------
// Check 2 — DATABASE_URL
// ---------------------------------------------------------------------------

pub(crate) fn check_database_url(raw: Option<String>) -> (CheckResult, Option<ParsedUrl>) {
    match raw {
        None => (
            CheckResult {
                name: "DATABASE_URL",
                severity: Severity::Blocker,
                headline: "is not set".into(),
                detail: Some(
                    "Set it in your shell:\n\n  \
                     export DATABASE_URL=postgres://postgres:dev@localhost/yourapp_dev\n\n\
                     Or in .env at the project root:\n\n  \
                     echo 'DATABASE_URL=postgres://postgres:dev@localhost/yourapp_dev' >> .env"
                        .into(),
                ),
                elapsed_ms: None,
            },
            None,
        ),
        Some(raw_str) => match parse_database_url(&raw_str) {
            Ok(p) => {
                let masked = render_url_masked(&p);
                (
                    CheckResult {
                        name: "DATABASE_URL",
                        severity: Severity::Ok,
                        headline: masked,
                        detail: None,
                        elapsed_ms: None,
                    },
                    Some(p),
                )
            }
            Err(e) => (
                CheckResult {
                    name: "DATABASE_URL",
                    severity: Severity::Blocker,
                    headline: format!("malformed: {e}"),
                    detail: Some(
                        "Expected shape:\n\n  \
                         postgres://user:password@host:port/database"
                            .into(),
                    ),
                    elapsed_ms: None,
                },
                None,
            ),
        },
    }
}

pub(crate) fn parse_database_url(raw: &str) -> Result<ParsedUrl, String> {
    let u = url::Url::parse(raw).map_err(|e| e.to_string())?;
    if !matches!(u.scheme(), "postgres" | "postgresql") {
        return Err(format!(
            "scheme `{}` not supported (expected postgres://…)",
            u.scheme()
        ));
    }
    let host = u
        .host_str()
        .ok_or_else(|| "missing host".to_string())?
        .to_string();
    let port = u.port().unwrap_or(5432);
    let user = u.username();
    if user.is_empty() {
        return Err("missing username".to_string());
    }
    let user = user.to_string();
    // url::Url percent-decodes the password automatically via `password()`.
    let password = u.password().unwrap_or("").to_string();
    let dbname = u.path().trim_start_matches('/').to_string();
    if dbname.is_empty() {
        return Err("missing database name".to_string());
    }
    Ok(ParsedUrl {
        raw: raw.to_string(),
        user,
        password,
        host,
        port,
        dbname,
    })
}

pub(crate) fn render_url_masked(p: &ParsedUrl) -> String {
    let pw = if p.password.is_empty() {
        String::new()
    } else {
        ":***".into()
    };
    format!(
        "postgres://{}{}@{}:{}/{}",
        p.user, pw, p.host, p.port, p.dbname
    )
}

// ---------------------------------------------------------------------------
// Check 3 — PostgreSQL TCP
// ---------------------------------------------------------------------------

pub(crate) fn check_pg_tcp(parsed: &ParsedUrl) -> CheckResult {
    let start = Instant::now();
    let addr_str = format!("{}:{}", parsed.host, parsed.port);
    let mut iter = match addr_str.to_socket_addrs() {
        Ok(it) => it,
        Err(e) => {
            return CheckResult {
                name: "PostgreSQL TCP",
                severity: Severity::Blocker,
                headline: format!("cannot resolve `{}`", parsed.host),
                detail: Some(format!(
                    "DNS lookup failed: {e}\n\n\
                     Check the host portion of DATABASE_URL."
                )),
                elapsed_ms: None,
            };
        }
    };
    let first = match iter.next() {
        Some(addr) => addr,
        None => {
            return CheckResult {
                name: "PostgreSQL TCP",
                severity: Severity::Blocker,
                headline: format!("`{}` resolved to no addresses", parsed.host),
                detail: Some("Check the host portion of DATABASE_URL.".into()),
                elapsed_ms: None,
            };
        }
    };
    match TcpStream::connect_timeout(&first, Duration::from_secs(2)) {
        Ok(_) => CheckResult {
            name: "PostgreSQL TCP",
            severity: Severity::Ok,
            headline: format!("reachable on {}:{}", parsed.host, parsed.port),
            detail: None,
            elapsed_ms: Some(start.elapsed().as_millis()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => CheckResult {
            name: "PostgreSQL TCP",
            severity: Severity::Blocker,
            headline: format!("timed out after 2s on {}:{}", parsed.host, parsed.port),
            detail: Some(pg_unreachable_recipe()),
            elapsed_ms: None,
        },
        Err(_) => CheckResult {
            name: "PostgreSQL TCP",
            severity: Severity::Blocker,
            headline: format!("unreachable on {}:{}", parsed.host, parsed.port),
            detail: Some(pg_unreachable_recipe()),
            elapsed_ms: None,
        },
    }
}

fn pg_unreachable_recipe() -> String {
    "Most likely Postgres isn't running. Start it:\n\n  \
     macOS:    brew services start postgresql@16\n  \
     Linux:    sudo systemctl start postgresql\n  \
     Docker:   docker run --rm -d -p 5432:5432 \\\n            \
                 -e POSTGRES_PASSWORD=dev --name rustio-pg postgres:16\n\n\
     After it starts, re-run `rustio doctor`."
        .into()
}

// ---------------------------------------------------------------------------
// Check 4 — PostgreSQL connect + simple query
// ---------------------------------------------------------------------------

pub(crate) async fn check_pg_connect(parsed: &ParsedUrl) -> CheckResult {
    use sqlx::postgres::PgPoolOptions;
    let start = Instant::now();
    let pool = match PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&parsed.raw)
        .await
    {
        Ok(p) => p,
        Err(e) => return map_pg_error(&e, parsed),
    };
    let result = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await;
    pool.close().await;
    match result {
        Ok(_) => CheckResult {
            name: "PostgreSQL",
            severity: Severity::Ok,
            headline: "connected".into(),
            detail: None,
            elapsed_ms: Some(start.elapsed().as_millis()),
        },
        Err(e) => map_pg_error(&e, parsed),
    }
}

pub(crate) fn map_pg_error(err: &sqlx::Error, parsed: &ParsedUrl) -> CheckResult {
    if let sqlx::Error::Database(db_err) = err {
        if let Some(code) = db_err.code() {
            if let Some(r) = map_pg_sqlstate(code.as_ref(), parsed) {
                return r;
            }
        }
    }
    if matches!(err, sqlx::Error::PoolTimedOut) {
        return CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: "reachable but slow to accept connections".into(),
            detail: Some("Server may be overloaded. Try restarting it.".into()),
            elapsed_ms: None,
        };
    }
    if matches!(err, sqlx::Error::Io(_)) {
        return CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: "connection dropped during handshake".into(),
            detail: Some(
                "This usually means the server restarted mid-connect.\n\
                 Try again, or check the server logs."
                    .into(),
            ),
            elapsed_ms: None,
        };
    }
    generic_pg_failure(err)
}

/// Pure SQLSTATE → CheckResult mapping. Extracted so unit tests can drive
/// it without constructing real `sqlx::Error::Database` instances (the
/// `DatabaseError` trait has private constructors).
pub(crate) fn map_pg_sqlstate(code: &str, parsed: &ParsedUrl) -> Option<CheckResult> {
    match code {
        // invalid_password
        "28P01" => Some(CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: format!("authentication failed for user `{}`", parsed.user),
            detail: Some(format!(
                "Check the password in DATABASE_URL.\n\n\
                 If you don't know it, recreate the user:\n\n  \
                 dropuser {user}\n  \
                 createuser -P {user}",
                user = parsed.user
            )),
            elapsed_ms: None,
        }),
        // invalid_authorization_specification (incl. role missing)
        "28000" => Some(CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: format!("role `{}` does not exist", parsed.user),
            detail: Some(format!(
                "Create it:\n\n  createuser -s {user}\n\n\
                 The `-s` flag makes it a superuser, fine for development.",
                user = parsed.user
            )),
            elapsed_ms: None,
        }),
        // invalid_catalog_name
        "3D000" => Some(CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: format!("database `{}` does not exist", parsed.dbname),
            detail: Some(format!(
                "Create it:\n\n  createdb {db}\n\n\
                 Or to set ownership:\n\n  createdb -O {user} {db}",
                db = parsed.dbname,
                user = parsed.user
            )),
            elapsed_ms: None,
        }),
        // insufficient_privilege
        "42501" => Some(CheckResult {
            name: "PostgreSQL",
            severity: Severity::Blocker,
            headline: format!(
                "user `{}` lacks privileges on `{}`",
                parsed.user, parsed.dbname
            ),
            detail: Some(format!(
                "Grant access:\n\n  \
                 psql -c 'GRANT ALL ON DATABASE {db} TO {user};' postgres",
                db = parsed.dbname,
                user = parsed.user
            )),
            elapsed_ms: None,
        }),
        _ => None,
    }
}

fn generic_pg_failure(err: &sqlx::Error) -> CheckResult {
    CheckResult {
        name: "PostgreSQL",
        severity: Severity::Blocker,
        headline: "refused the connection".into(),
        detail: Some(format!(
            "Original error:\n  {err}\n\n\
             Check that DATABASE_URL matches your Postgres setup."
        )),
        elapsed_ms: None,
    }
}

// ---------------------------------------------------------------------------
// Check 5 — Meilisearch (warning only)
// ---------------------------------------------------------------------------

async fn check_meili() -> CheckResult {
    use rustio_core::search::MeiliClient;
    let url = std::env::var("MEILI_URL").unwrap_or_else(|_| "http://localhost:7700".into());
    let api_key = std::env::var("MEILI_MASTER_KEY").ok();
    let start = Instant::now();
    let client = match MeiliClient::new(&url, api_key) {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                name: "Meilisearch",
                severity: Severity::Warning,
                headline: format!("misconfigured: {e}"),
                detail: Some(meili_unreachable_recipe()),
                elapsed_ms: None,
            };
        }
    };
    match client.health().await {
        Ok(()) => CheckResult {
            name: "Meilisearch",
            severity: Severity::Ok,
            headline: format!("reachable on {url}"),
            detail: None,
            elapsed_ms: Some(start.elapsed().as_millis()),
        },
        Err(_) => CheckResult {
            name: "Meilisearch",
            severity: Severity::Warning,
            headline: format!("unreachable on {url}"),
            detail: Some(meili_unreachable_recipe()),
            elapsed_ms: None,
        },
    }
}

fn meili_unreachable_recipe() -> String {
    "Search will be unavailable; the app boots fine without it. To install:\n\n  \
     macOS:    brew install meilisearch && brew services start meilisearch\n  \
     Docker:   docker run --rm -d -p 7700:7700 \\\n            \
                 --name rustio-meili getmeili/meilisearch:v1.10"
        .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn skipped(name: &'static str, reason: &str) -> CheckResult {
    CheckResult {
        name,
        severity: Severity::Skipped,
        headline: format!("(skipped — {reason})"),
        detail: None,
        elapsed_ms: None,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render(results: &[CheckResult], args: &Args) {
    let use_color = !args.no_color
        && std::env::var_os("NO_COLOR").is_none()
        && std::io::stdout().is_terminal();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    if args.quiet {
        for r in results {
            if matches!(r.severity, Severity::Blocker | Severity::Warning) {
                let sym = symbol_for(r.severity, use_color);
                let _ = writeln!(out, "{} {}: {}", sym, r.name, r.headline);
            }
        }
        let _ = writeln!(out);
        let _ = render_summary(&mut out, results, use_color);
        return;
    }

    for r in results {
        render_check_default(&mut out, r, args, use_color);
    }
    let _ = writeln!(out);
    let _ = render_summary(&mut out, results, use_color);
}

fn render_check_default<W: Write>(out: &mut W, r: &CheckResult, args: &Args, use_color: bool) {
    let sym = symbol_for(r.severity, use_color);
    let _ = write!(out, "  {}  {:<16}{}", sym, r.name, r.headline);
    if let Some(ms) = r.elapsed_ms {
        let _ = write!(out, "  ({ms}ms)");
    }
    let _ = writeln!(out);

    let show_detail = match r.severity {
        Severity::Blocker | Severity::Warning => r.detail.is_some(),
        Severity::Ok => args.verbose && r.detail.is_some(),
        Severity::Skipped => false,
    };
    if show_detail {
        let _ = writeln!(out);
        for line in r.detail.as_ref().unwrap().lines() {
            let _ = writeln!(out, "       {line}");
        }
        let _ = writeln!(out);
    }
}

pub(crate) fn render_summary<W: Write>(
    out: &mut W,
    results: &[CheckResult],
    use_color: bool,
) -> std::io::Result<()> {
    let blockers = results
        .iter()
        .filter(|r| r.severity == Severity::Blocker)
        .count();
    let warnings = results
        .iter()
        .filter(|r| r.severity == Severity::Warning)
        .count();

    if blockers == 0 && warnings == 0 {
        writeln!(
            out,
            "  Status: READY {}",
            colorize("✓", Color::Green, use_color)
        )?;
        // v1.5.0 polish — give the beginner the literal next command.
        writeln!(out)?;
        writeln!(out, "  Next: cargo run")?;
    } else if blockers == 0 {
        // DEGRADED: warnings only. The app boots; some feature (today:
        // Meilisearch / search) won't work until the warning is resolved.
        // Same exit code as READY (0) so CI doesn't fail on optional deps.
        writeln!(
            out,
            "  Status: READY (DEGRADED) {}",
            colorize("⚠", Color::Yellow, use_color)
        )?;
        writeln!(out)?;
        writeln!(out, "  Next: cargo run")?;
    } else {
        let warn_part = if warnings > 0 {
            format!(
                ", {warnings} warning{}",
                if warnings == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        };
        writeln!(
            out,
            "  Status: NOT READY — {blockers} blocking issue{}{warn_part}",
            if blockers == 1 { "" } else { "s" }
        )?;
        // No `Next:` line for NOT READY — each failed check already
        // carries its own fix recipe (".../After it starts, re-run
        // `rustio doctor`"). A blanket "Next: …" would either duplicate
        // those or pick the wrong one when there are multiple blockers.
    }
    Ok(())
}

#[derive(Copy, Clone)]
enum Color {
    Green,
    Red,
    Yellow,
    Dim,
}

fn colorize(s: &str, color: Color, use_color: bool) -> String {
    if !use_color {
        return s.to_string();
    }
    let code = match color {
        Color::Green => "\x1b[32m",
        Color::Red => "\x1b[31m",
        Color::Yellow => "\x1b[33m",
        Color::Dim => "\x1b[2m",
    };
    format!("{code}{s}\x1b[0m")
}

fn symbol_for(sev: Severity, use_color: bool) -> String {
    let (sym, color) = match sev {
        Severity::Ok => ("✓", Color::Green),
        Severity::Blocker => ("✗", Color::Red),
        Severity::Warning => ("⚠", Color::Yellow),
        Severity::Skipped => ("⏭", Color::Dim),
    };
    colorize(sym, color, use_color)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(host: &str, port: u16, user: &str, pw: &str, db: &str) -> ParsedUrl {
        ParsedUrl {
            raw: format!("postgres://{user}:{pw}@{host}:{port}/{db}"),
            user: user.into(),
            password: pw.into(),
            host: host.into(),
            port,
            dbname: db.into(),
        }
    }

    fn ok(name: &'static str) -> CheckResult {
        CheckResult {
            name,
            severity: Severity::Ok,
            headline: "ok".into(),
            detail: None,
            elapsed_ms: None,
        }
    }
    fn blocker(name: &'static str) -> CheckResult {
        CheckResult {
            name,
            severity: Severity::Blocker,
            headline: "blocker".into(),
            detail: Some("fix me".into()),
            elapsed_ms: None,
        }
    }
    fn warn(name: &'static str) -> CheckResult {
        CheckResult {
            name,
            severity: Severity::Warning,
            headline: "warning".into(),
            detail: Some("heads up".into()),
            elapsed_ms: None,
        }
    }

    // ----- URL parsing -----

    #[test]
    fn doctor_url_parse_extracts_components() {
        let p = parse_database_url("postgres://alice:secret@db.local:6543/shop_dev").unwrap();
        assert_eq!(p.user, "alice");
        assert_eq!(p.password, "secret");
        assert_eq!(p.host, "db.local");
        assert_eq!(p.port, 6543);
        assert_eq!(p.dbname, "shop_dev");
    }

    #[test]
    fn doctor_url_parse_defaults_port_to_5432() {
        let p = parse_database_url("postgres://u:p@localhost/d").unwrap();
        assert_eq!(p.port, 5432);
    }

    #[test]
    fn doctor_url_parse_rejects_missing_dbname() {
        let err = parse_database_url("postgres://u:p@localhost").unwrap_err();
        assert!(
            err.contains("missing database name"),
            "expected dbname error, got: {err}"
        );
    }

    #[test]
    fn doctor_url_parse_rejects_wrong_scheme() {
        let err = parse_database_url("mysql://u:p@localhost/d").unwrap_err();
        assert!(err.contains("scheme"), "expected scheme error, got: {err}");
    }

    #[test]
    fn doctor_url_parse_rejects_missing_username() {
        // `postgres://localhost/d` → no userinfo at all
        let err = parse_database_url("postgres://localhost/d").unwrap_err();
        assert!(
            err.contains("missing username"),
            "expected username error, got: {err}"
        );
    }

    #[test]
    fn doctor_url_render_masks_password() {
        let p = parsed("localhost", 5432, "alice", "shouldnotappear", "shop");
        let s = render_url_masked(&p);
        assert!(s.contains(":***@"), "password must be masked: {s}");
        assert!(
            !s.contains("shouldnotappear"),
            "literal password leaked: {s}"
        );
    }

    #[test]
    fn doctor_url_render_omits_colon_when_no_password() {
        let p = parsed("localhost", 5432, "alice", "", "shop");
        let s = render_url_masked(&p);
        assert!(
            s.contains("postgres://alice@"),
            "no password ⇒ no `:` before @, got: {s}"
        );
        assert!(!s.contains("***"), "no password ⇒ no mask, got: {s}");
    }

    // ----- Severity summary / exit-code intent -----

    #[test]
    fn doctor_summary_all_green_is_ready() {
        let mut buf: Vec<u8> = Vec::new();
        let results = vec![ok("a"), ok("b"), ok("c")];
        render_summary(&mut buf, &results, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("READY"), "expected READY, got: {s}");
        assert!(!s.contains("DEGRADED"), "must not say DEGRADED: {s}");
        assert!(!s.contains("NOT READY"), "must not say NOT READY: {s}");
    }

    /// v1.5.0 polish — Next-step hint must appear on the green path so the
    /// beginner knows what to type. Pairs with the same hint on DEGRADED
    /// (the app boots in both cases).
    #[test]
    fn doctor_summary_ready_includes_next_cargo_run() {
        let mut buf: Vec<u8> = Vec::new();
        let results = vec![ok("a"), ok("b")];
        render_summary(&mut buf, &results, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("Next: cargo run"),
            "READY summary must end with `Next: cargo run`, got:\n{s}"
        );
    }

    #[test]
    fn doctor_summary_only_warnings_is_degraded() {
        let mut buf: Vec<u8> = Vec::new();
        let results = vec![ok("a"), warn("b")];
        render_summary(&mut buf, &results, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("READY (DEGRADED)"),
            "expected DEGRADED, got: {s}"
        );
        // v1.5.0 polish — DEGRADED uses the ⚠ symbol, no "— N warning(s)" suffix.
        assert!(
            s.contains('⚠'),
            "DEGRADED must include the ⚠ symbol, got: {s}"
        );
        assert!(
            !s.contains("warning"),
            "DEGRADED no longer shows the warning count (replaced by ⚠ symbol), got: {s}"
        );
    }

    /// v1.5.0 polish — DEGRADED is still a "ready to run" state, so the
    /// Next-step hint applies just like the all-green case. Search just
    /// won't work until the user installs Meilisearch.
    #[test]
    fn doctor_summary_degraded_includes_next_cargo_run() {
        let mut buf: Vec<u8> = Vec::new();
        let results = vec![ok("a"), warn("b")];
        render_summary(&mut buf, &results, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("Next: cargo run"),
            "DEGRADED summary must end with `Next: cargo run`, got:\n{s}"
        );
    }

    #[test]
    fn doctor_summary_any_blocker_is_not_ready() {
        let mut buf: Vec<u8> = Vec::new();
        let results = vec![ok("a"), blocker("b"), warn("c")];
        render_summary(&mut buf, &results, false).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("NOT READY"), "expected NOT READY, got: {s}");
        assert!(s.contains("1 blocking issue"), "must show count: {s}");
        assert!(s.contains("1 warning"), "must show warning count: {s}");
        // v1.5.0 polish — NOT READY does NOT get a generic Next: line; the
        // per-failure recipes carry the user's actual next step.
        assert!(
            !s.contains("Next: cargo run"),
            "NOT READY must not show `Next: cargo run` (recipes per failure)"
        );
    }

    // ----- PG SQLSTATE mapping -----

    #[test]
    fn doctor_pg_sqlstate_28p01_auth_failed() {
        let p = parsed("localhost", 5432, "alice", "x", "shop");
        let r = map_pg_sqlstate("28P01", &p).expect("should map");
        assert_eq!(r.severity, Severity::Blocker);
        assert!(
            r.headline.contains("authentication failed"),
            "headline: {}",
            r.headline
        );
        assert!(r.headline.contains("alice"), "headline names user");
        let detail = r.detail.unwrap();
        assert!(detail.contains("dropuser alice"), "recipe: {detail}");
        assert!(detail.contains("createuser -P alice"), "recipe: {detail}");
    }

    #[test]
    fn doctor_pg_sqlstate_28000_role_missing() {
        let p = parsed("localhost", 5432, "alice", "x", "shop");
        let r = map_pg_sqlstate("28000", &p).expect("should map");
        assert!(
            r.headline.contains("role `alice` does not exist"),
            "headline: {}",
            r.headline
        );
        assert!(
            r.detail.unwrap().contains("createuser -s alice"),
            "must suggest createuser"
        );
    }

    #[test]
    fn doctor_pg_sqlstate_3d000_db_missing() {
        let p = parsed("localhost", 5432, "alice", "x", "shop_dev");
        let r = map_pg_sqlstate("3D000", &p).expect("should map");
        assert!(
            r.headline.contains("database `shop_dev` does not exist"),
            "headline: {}",
            r.headline
        );
        let detail = r.detail.unwrap();
        assert!(detail.contains("createdb shop_dev"), "recipe: {detail}");
        assert!(
            detail.contains("createdb -O alice shop_dev"),
            "ownership recipe: {detail}"
        );
    }

    #[test]
    fn doctor_pg_sqlstate_42501_privileges() {
        let p = parsed("localhost", 5432, "alice", "x", "shop_dev");
        let r = map_pg_sqlstate("42501", &p).expect("should map");
        assert!(r.headline.contains("alice"), "names user");
        assert!(r.headline.contains("shop_dev"), "names db");
        assert!(
            r.detail.unwrap().contains("GRANT ALL ON DATABASE shop_dev"),
            "must suggest GRANT"
        );
    }

    #[test]
    fn doctor_pg_sqlstate_unknown_returns_none() {
        let p = parsed("localhost", 5432, "alice", "x", "shop");
        assert!(map_pg_sqlstate("XX000", &p).is_none());
        assert!(map_pg_sqlstate("", &p).is_none());
    }

    // ----- DATABASE_URL check wrapper -----

    #[test]
    fn doctor_check_database_url_unset_is_blocker() {
        let (r, parsed) = check_database_url(None);
        assert_eq!(r.severity, Severity::Blocker);
        assert!(parsed.is_none());
        assert!(r.headline.contains("not set"), "headline: {}", r.headline);
        assert!(
            r.detail.unwrap().contains("export DATABASE_URL"),
            "recipe must include export"
        );
    }

    #[test]
    fn doctor_check_database_url_malformed_is_blocker() {
        let (r, parsed) = check_database_url(Some("not a url".into()));
        assert_eq!(r.severity, Severity::Blocker);
        assert!(parsed.is_none());
        assert!(
            r.headline.contains("malformed"),
            "headline: {}",
            r.headline
        );
    }

    #[test]
    fn doctor_check_database_url_valid_is_ok_with_masked_render() {
        let (r, parsed) = check_database_url(Some(
            "postgres://alice:topsecret@localhost/shop".into(),
        ));
        assert_eq!(r.severity, Severity::Ok);
        let p = parsed.unwrap();
        assert_eq!(p.user, "alice");
        assert_eq!(p.password, "topsecret");
        assert!(
            !r.headline.contains("topsecret"),
            "headline must mask: {}",
            r.headline
        );
        assert!(r.headline.contains(":***@"), "headline must mask: {}", r.headline);
    }
}
