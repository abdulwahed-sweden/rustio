use std::fs;
use std::path::Path;
use std::process::{Command as ProcessCommand, ExitCode};

const USAGE: &str = r#"rustio — the RustIO framework CLI

USAGE:
    rustio <COMMAND>

COMMANDS:
    new project <name>        Create a new RustIO project
    new app <name>            Create a new app inside the current project
    run                       Build and run the project in the current directory
    migrate generate <name>   Create a new migration file
    migrate apply [-v]        Apply all pending migrations (verbose with -v / --verbose)
    migrate status            Show applied and pending migrations

ENVIRONMENT:
    RUSTIO_DATABASE_URL       Database URL (default: sqlite://app.db?mode=rwc)
    NO_COLOR                  Disable colored CLI output
"#;

const DEFAULT_DATABASE_URL: &str = "sqlite://app.db?mode=rwc";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = match parse_command(&args) {
        Ok(Command::Help) => {
            print!("{USAGE}");
            Ok(())
        }
        Ok(Command::Version) => {
            println!("rustio {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Ok(Command::NewProject(name)) => new_project(&name),
        Ok(Command::NewApp(name)) => new_app(&name),
        Ok(Command::Run) => run(),
        Ok(Command::MigrateGenerate(name)) => migrate_generate(&name),
        Ok(Command::MigrateApply { verbose }) => migrate_apply(verbose).await,
        Ok(Command::MigrateStatus) => migrate_status().await,
        Err(msg) => {
            out::error_line(&msg);
            eprintln!();
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            out::error_line(&msg);
            ExitCode::from(1)
        }
    }
}

#[derive(Debug, PartialEq)]
enum Command {
    NewProject(String),
    NewApp(String),
    Run,
    MigrateGenerate(String),
    MigrateApply { verbose: bool },
    MigrateStatus,
    Version,
    Help,
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args.get(1).map(String::as_str) {
        None | Some("--help") | Some("-h") | Some("help") => Ok(Command::Help),
        Some("--version") | Some("-V") | Some("version") => Ok(Command::Version),
        Some("run") => {
            if args.len() > 2 {
                return Err(format!("unexpected argument `{}`", args[2]));
            }
            Ok(Command::Run)
        }
        Some("new") => {
            let kind = args
                .get(2)
                .ok_or("usage: rustio new <project|app> <name>")?;
            let name = args
                .get(3)
                .ok_or("usage: rustio new <project|app> <name>")?;
            match kind.as_str() {
                "project" => Ok(Command::NewProject(name.clone())),
                "app" => Ok(Command::NewApp(name.clone())),
                other => Err(format!("unknown subcommand `new {other}`")),
            }
        }
        Some("migrate") => match args.get(2).map(String::as_str) {
            Some("generate") => {
                let name = args.get(3).ok_or("usage: rustio migrate generate <name>")?;
                Ok(Command::MigrateGenerate(name.clone()))
            }
            Some("apply") => {
                let rest = &args[3..];
                let mut verbose = false;
                for a in rest {
                    match a.as_str() {
                        "-v" | "--verbose" => verbose = true,
                        other => return Err(format!("unexpected argument `{other}`")),
                    }
                }
                Ok(Command::MigrateApply { verbose })
            }
            Some("status") => {
                if args.len() > 3 {
                    return Err(format!("unexpected argument `{}`", args[3]));
                }
                Ok(Command::MigrateStatus)
            }
            Some(other) => Err(format!("unknown subcommand `migrate {other}`")),
            None => Err("usage: rustio migrate <generate|apply|status>".into()),
        },
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn new_project(name: &str) -> Result<(), String> {
    validate_name(name)?;
    let root = Path::new(name);
    if root.exists() {
        return Err(format!("directory `{name}` already exists"));
    }

    fs::create_dir_all(root.join("apps")).map_err(err_str)?;
    fs::create_dir_all(root.join("migrations")).map_err(err_str)?;
    fs::create_dir_all(root.join("static")).map_err(err_str)?;
    fs::create_dir_all(root.join("templates")).map_err(err_str)?;

    fs::write(root.join("Cargo.toml"), cargo_toml_tmpl(name)).map_err(err_str)?;
    fs::write(root.join("main.rs"), MAIN_RS).map_err(err_str)?;
    fs::write(root.join("apps/mod.rs"), APPS_MOD_RS).map_err(err_str)?;
    fs::write(root.join(".gitignore"), GITIGNORE).map_err(err_str)?;
    fs::write(root.join("README.md"), render(README_MD, &[("NAME", name)])).map_err(err_str)?;

    out::success("Created project", &format!("\"{name}\""));
    println!();
    out::hint(&format!("cd {name}"));
    out::hint("rustio run");
    Ok(())
}

fn new_app(name: &str) -> Result<(), String> {
    validate_name(name)?;
    if !Path::new("apps/mod.rs").exists() {
        return Err(
            "not inside a RustIO project — expected apps/mod.rs in the current directory".into(),
        );
    }

    let app_dir = Path::new("apps").join(name);
    if app_dir.exists() {
        return Err(format!("app `{name}` already exists"));
    }

    let struct_name = capitalize(name);
    let table_name = pluralize(name);

    fs::create_dir_all(&app_dir).map_err(err_str)?;
    fs::write(app_dir.join("mod.rs"), APP_MOD_RS).map_err(err_str)?;
    fs::write(
        app_dir.join("models.rs"),
        render(
            APP_MODELS_RS,
            &[("STRUCT", &struct_name), ("TABLE", &table_name)],
        ),
    )
    .map_err(err_str)?;
    fs::write(
        app_dir.join("admin.rs"),
        render(APP_ADMIN_RS, &[("STRUCT", &struct_name)]),
    )
    .map_err(err_str)?;
    fs::write(
        app_dir.join("views.rs"),
        render(APP_VIEWS_RS, &[("NAME", name), ("STRUCT", &struct_name)]),
    )
    .map_err(err_str)?;

    register_app_in_mod(name)?;

    let migrations_dir = Path::new("migrations");
    let create_sql = format!(
        "CREATE TABLE {table} (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
);\n",
        table = table_name,
    );
    let migration_path = rustio_core::migrations::generate(
        migrations_dir,
        &format!("create_{table_name}"),
        &create_sql,
    )
    .map_err(err_str)?;

    out::success("Created app", &format!("\"{name}\""));
    println!();
    out::plain(&format!("{:<12} apps/{name}/models.rs", out::dim("model")));
    out::plain(&format!(
        "{:<12} {}",
        out::dim("migration"),
        migration_path.display()
    ));
    out::plain(&format!("{:<12} /admin/{table_name}", out::dim("admin")));
    out::plain(&format!("{:<12} /{name}", out::dim("view")));
    println!();
    out::hint("rustio migrate apply");
    out::hint("rustio run");
    Ok(())
}

fn run() -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "no Cargo.toml in current directory — run this from inside a RustIO project".into(),
        );
    }
    let status = ProcessCommand::new("cargo")
        .arg("run")
        .status()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo run exited with {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn migrate_generate(name: &str) -> Result<(), String> {
    let dir = Path::new("migrations");
    let header = format!("-- migration: {name}\n\n");
    let path = rustio_core::migrations::generate(dir, name, &header).map_err(err_str)?;
    out::success("Created migration", &path.display().to_string());
    Ok(())
}

async fn migrate_apply(verbose: bool) -> Result<(), String> {
    let db = rustio_core::Db::connect(&database_url())
        .await
        .map_err(err_str)?;
    let dir = Path::new("migrations");
    let opts = rustio_core::migrations::ApplyOptions { verbose };
    let applied = rustio_core::migrations::apply_with(&db, dir, opts)
        .await
        .map_err(err_str)?;
    if applied.is_empty() {
        out::info("No pending migrations.");
    } else {
        for f in &applied {
            println!("  {} applied {f}", out::check());
        }
        let n = applied.len();
        let noun = if n == 1 { "migration" } else { "migrations" };
        println!();
        out::success(&format!("Applied {n}"), noun);
    }
    Ok(())
}

async fn migrate_status() -> Result<(), String> {
    let db = rustio_core::Db::connect(&database_url())
        .await
        .map_err(err_str)?;
    let status = rustio_core::migrations::status(&db, Path::new("migrations"))
        .await
        .map_err(err_str)?;

    if status.applied.is_empty() && status.pending.is_empty() {
        out::info("No migrations found.");
        return Ok(());
    }

    if !status.applied.is_empty() {
        println!("{}", out::bold("Applied:"));
        for record in &status.applied {
            println!(
                "  {} {}  {}",
                out::check(),
                record.filename,
                out::dim(&record.applied_at),
            );
        }
    }

    if !status.pending.is_empty() {
        if !status.applied.is_empty() {
            println!();
        }
        println!("{}", out::bold("Pending:"));
        for name in &status.pending {
            println!("  {} {}", out::dot(), name);
        }
    }

    Ok(())
}

fn register_app_in_mod(name: &str) -> Result<(), String> {
    let path = Path::new("apps/mod.rs");
    let current = fs::read_to_string(path).map_err(err_str)?;

    let module_line = format!("pub mod {name};\n");
    let registrations = format!(
        "    router = {name}::admin::register(router, db);\n    router = {name}::views::register(router);\n"
    );

    let updated = current
        .replacen(
            "// -- end modules --",
            &format!("{module_line}// -- end modules --"),
            1,
        )
        .replacen(
            "    // -- end registrations --",
            &format!("{registrations}    // -- end registrations --"),
            1,
        );

    if updated == current {
        return Err(
            "apps/mod.rs is missing marker comments `// -- modules --` and `// -- registrations --` — restore them or recreate the file"
                .into(),
        );
    }

    fs::write(path, updated).map_err(err_str)?;
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(format!(
            "name `{name}` must start with a lowercase letter (e.g. `blog`, `user_profile`)"
        ));
    }
    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' {
            return Err(format!(
                "name `{name}` may only contain lowercase letters, digits, and underscores"
            ));
        }
    }
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        name.to_string()
    } else {
        format!("{name}s")
    }
}

fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn err_str<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn database_url() -> String {
    std::env::var("RUSTIO_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

// Returns the `rustio-core` dependency spec used in generated `Cargo.toml`.
// Defaults to a version matching the CLI's own package version (crates.io).
// Override with `RUSTIO_CORE_PATH=/path/to/rustio-core` for local development.
fn rustio_core_dep() -> String {
    if let Ok(path) = std::env::var("RUSTIO_CORE_PATH") {
        return format!(r#"{{ path = "{path}" }}"#);
    }
    format!(r#""{}""#, env!("CARGO_PKG_VERSION"))
}

fn cargo_toml_tmpl(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{name}"
path = "main.rs"

[dependencies]
rustio-core = {dep}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros"] }}
"#,
        dep = rustio_core_dep(),
    )
}

mod out {
    use std::io::{self, IsTerminal};

    pub fn success(label: &str, message: &str) {
        println!("{} {label} {message}", check());
    }

    pub fn info(message: &str) {
        println!("{message}");
    }

    pub fn hint(text: &str) {
        println!("  {} {text}", colored("→", "36"));
    }

    pub fn plain(text: &str) {
        println!("  {text}");
    }

    pub fn error_line(msg: &str) {
        eprintln!("{} {msg}", colored("error:", "31"));
    }

    pub fn check() -> String {
        colored("✔", "32")
    }

    pub fn dot() -> String {
        colored("•", "33")
    }

    pub fn bold(s: &str) -> String {
        colored(s, "1")
    }

    pub fn dim(s: &str) -> String {
        colored(s, "2")
    }

    fn colored(text: &str, code: &str) -> String {
        if should_color() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn should_color() -> bool {
        if std::env::var("NO_COLOR").is_ok() {
            return false;
        }
        io::stdout().is_terminal()
    }
}

const GITIGNORE: &str = "target/\napp.db\napp.db-shm\napp.db-wal\n";

const README_MD: &str = r#"# {{NAME}}

A RustIO project.

## Commands

    rustio new app <name>       # scaffold an app
    rustio migrate generate X   # create a migration
    rustio migrate apply        # apply pending migrations
    rustio migrate status       # show applied & pending
    rustio run                  # build and run the server

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
"#;

const MAIN_RS: &str = r#"use rustio_core::auth::authenticate;
use rustio_core::defaults::with_defaults;
use rustio_core::{Db, Router, Server};

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Schema is managed by `rustio migrate apply`.
    // Override the database URL with RUSTIO_DATABASE_URL if needed.
    let url = std::env::var("RUSTIO_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());
    let db = Db::connect(&url).await?;

    let router = with_defaults(Router::new()).wrap(authenticate);
    let router = apps::register_all(router, &db);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8000));
    eprintln!("serving on http://{addr}");
    Server::bind(addr).serve_router(router).await?;
    Ok(())
}
"#;

const APPS_MOD_RS: &str = r#"use rustio_core::{Db, Router};

// -- modules --
// -- end modules --

#[allow(unused_mut, unused_variables)]
pub fn register_all(mut router: Router, db: &Db) -> Router {
    // -- registrations --
    // -- end registrations --
    router
}
"#;

const APP_MOD_RS: &str = r#"pub mod admin;
pub mod models;
pub mod views;
"#;

const APP_MODELS_RS: &str = r#"use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(Debug, RustioAdmin)]
pub struct {{STRUCT}} {
    pub id: i64,
    pub name: String,
}

impl Model for {{STRUCT}} {
    const TABLE: &'static str = "{{TABLE}}";
    const COLUMNS: &'static [&'static str] = &["id", "name"];
    const INSERT_COLUMNS: &'static [&'static str] = &["name"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            name: row.get_string("name")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![self.name.clone().into()]
    }
}
"#;

const APP_ADMIN_RS: &str = r#"use rustio_core::{admin, Db, Router};

use super::models::{{STRUCT}};

pub fn register(router: Router, db: &Db) -> Router {
    admin::register::<{{STRUCT}}>(router, db)
}
"#;

const APP_VIEWS_RS: &str = r#"use rustio_core::{text, Error, Response, Router};

pub fn register(router: Router) -> Router {
    router.get("/{{NAME}}", |_req, _params| async {
        Ok::<Response, Error>(text("{{STRUCT}} views — placeholder\n"))
    })
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        std::iter::once("rustio")
            .chain(parts.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn parse_no_args_is_help() {
        assert_eq!(parse_command(&args(&[])).unwrap(), Command::Help);
    }

    #[test]
    fn parse_help_flag() {
        assert_eq!(parse_command(&args(&["--help"])).unwrap(), Command::Help);
        assert_eq!(parse_command(&args(&["-h"])).unwrap(), Command::Help);
        assert_eq!(parse_command(&args(&["help"])).unwrap(), Command::Help);
    }

    #[test]
    fn parse_version_flag() {
        assert_eq!(
            parse_command(&args(&["--version"])).unwrap(),
            Command::Version
        );
        assert_eq!(parse_command(&args(&["-V"])).unwrap(), Command::Version);
        assert_eq!(
            parse_command(&args(&["version"])).unwrap(),
            Command::Version
        );
    }

    #[test]
    fn parse_run() {
        assert_eq!(parse_command(&args(&["run"])).unwrap(), Command::Run);
    }

    #[test]
    fn parse_run_rejects_extra() {
        assert!(parse_command(&args(&["run", "extra"])).is_err());
    }

    #[test]
    fn parse_new_project() {
        assert_eq!(
            parse_command(&args(&["new", "project", "mysite"])).unwrap(),
            Command::NewProject(String::from("mysite"))
        );
    }

    #[test]
    fn parse_new_app() {
        assert_eq!(
            parse_command(&args(&["new", "app", "blog"])).unwrap(),
            Command::NewApp(String::from("blog"))
        );
    }

    #[test]
    fn parse_new_requires_kind_and_name() {
        assert!(parse_command(&args(&["new"])).is_err());
        assert!(parse_command(&args(&["new", "project"])).is_err());
    }

    #[test]
    fn parse_new_unknown_kind() {
        assert!(parse_command(&args(&["new", "cluster", "x"])).is_err());
    }

    #[test]
    fn parse_migrate_generate() {
        assert_eq!(
            parse_command(&args(&["migrate", "generate", "add_users"])).unwrap(),
            Command::MigrateGenerate(String::from("add_users"))
        );
    }

    #[test]
    fn parse_migrate_apply() {
        assert_eq!(
            parse_command(&args(&["migrate", "apply"])).unwrap(),
            Command::MigrateApply { verbose: false }
        );
    }

    #[test]
    fn parse_migrate_apply_verbose() {
        assert_eq!(
            parse_command(&args(&["migrate", "apply", "-v"])).unwrap(),
            Command::MigrateApply { verbose: true }
        );
        assert_eq!(
            parse_command(&args(&["migrate", "apply", "--verbose"])).unwrap(),
            Command::MigrateApply { verbose: true }
        );
    }

    #[test]
    fn parse_migrate_status() {
        assert_eq!(
            parse_command(&args(&["migrate", "status"])).unwrap(),
            Command::MigrateStatus
        );
    }

    #[test]
    fn parse_migrate_generate_requires_name() {
        assert!(parse_command(&args(&["migrate", "generate"])).is_err());
    }

    #[test]
    fn parse_migrate_unknown_subcommand() {
        assert!(parse_command(&args(&["migrate", "rollback"])).is_err());
    }

    #[test]
    fn parse_migrate_apply_rejects_unknown_flag() {
        assert!(parse_command(&args(&["migrate", "apply", "foo"])).is_err());
        assert!(parse_command(&args(&["migrate", "apply", "--nope"])).is_err());
    }

    #[test]
    fn parse_migrate_status_rejects_extra() {
        assert!(parse_command(&args(&["migrate", "status", "foo"])).is_err());
    }

    #[test]
    fn parse_unknown_command() {
        assert!(parse_command(&args(&["banana"])).is_err());
    }

    #[test]
    fn validate_name_accepts_valid() {
        assert!(validate_name("blog").is_ok());
        assert!(validate_name("blog_posts").is_ok());
        assert!(validate_name("a1").is_ok());
    }

    #[test]
    fn validate_name_rejects_bad_start() {
        assert!(validate_name("").is_err());
        assert!(validate_name("1blog").is_err());
        assert!(validate_name("Blog").is_err());
        assert!(validate_name("_blog").is_err());
    }

    #[test]
    fn validate_name_error_suggests_valid_form() {
        let msg = validate_name("Blog").unwrap_err();
        assert!(msg.contains("lowercase letter"));
    }

    #[test]
    fn validate_name_rejects_bad_chars() {
        assert!(validate_name("blog-posts").is_err());
        assert!(validate_name("blog.posts").is_err());
        assert!(validate_name("Blog").is_err());
    }

    #[test]
    fn capitalize_handles_simple() {
        assert_eq!(capitalize("blog"), "Blog");
        assert_eq!(capitalize("user"), "User");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("a"), "A");
    }

    #[test]
    fn pluralize_appends_s_when_missing() {
        assert_eq!(pluralize("blog"), "blogs");
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("post"), "posts");
    }

    #[test]
    fn pluralize_leaves_trailing_s_alone() {
        assert_eq!(pluralize("posts"), "posts");
        assert_eq!(pluralize("users"), "users");
        assert_eq!(pluralize("news"), "news");
    }

    #[test]
    fn render_substitutes_vars() {
        let tpl = "name={{NAME}} struct={{STRUCT}}";
        assert_eq!(
            render(tpl, &[("NAME", "blog"), ("STRUCT", "Blog")]),
            "name=blog struct=Blog"
        );
    }

    #[test]
    fn render_leaves_unknown_vars_alone() {
        let tpl = "{{UNKNOWN}} {{KNOWN}}";
        assert_eq!(render(tpl, &[("KNOWN", "k")]), "{{UNKNOWN}} k");
    }
}
