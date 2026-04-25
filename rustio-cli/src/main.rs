use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
// 0.9.x AI surface: planner emits PlanResult (plan + explanation); review_plan
// builds a structured PlanReview; execute_plan_document writes files atomically.
use rustio_core::ai::{
    execute_plan_document, generate_plan, load_plan, review_plan, ExecuteOptions, LoadedPlan,
    PlanDocument, PlanRequest,
};
use rustio_core::auth::{self, Role};
use rustio_core::migrations;
use rustio_core::orm::Db;
use rustio_core::schema::{Schema, SCHEMA_VERSION};

#[derive(Parser)]
#[command(name = "rustio", version, about = "The RustIO command-line tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new project or app.
    New {
        #[command(subcommand)]
        kind: NewKind,
    },
    /// Run database migrations.
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// Manage users.
    User {
        #[command(subcommand)]
        action: UserAction,
    },
    /// Manage groups.
    Group {
        #[command(subcommand)]
        action: GroupAction,
    },
    /// Manage permissions.
    Perm {
        #[command(subcommand)]
        action: PermAction,
    },
    /// Work with the AI planner.
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
    /// Print the current schema JSON.
    Schema {
        #[arg(long, default_value = "rustio.schema.json")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum NewKind {
    Project { name: String },
    App { name: String },
}

#[derive(Subcommand)]
enum MigrateAction {
    Apply {
        #[arg(long, env = "DATABASE_URL")]
        db: String,
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
    },
    Generate {
        name: String,
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
    },
    Status {
        #[arg(long, env = "DATABASE_URL")]
        db: String,
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum UserAction {
    /// Create a user. If `--password` is omitted it will be prompted.
    Create {
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "admin")]
        role: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
    /// Reset a user's password.
    SetPassword {
        #[arg(long)]
        email: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
    /// Add a user to a group.
    AddToGroup {
        #[arg(long)]
        email: String,
        #[arg(long)]
        group: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
}

#[derive(Subcommand)]
enum GroupAction {
    Create {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
    Grant {
        #[arg(long)]
        group: String,
        #[arg(long)]
        permission: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
}

#[derive(Subcommand)]
enum PermAction {
    /// List every registered permission.
    List {
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
    /// Grant a permission directly to a user (prefer groups).
    GrantUser {
        #[arg(long)]
        email: String,
        #[arg(long)]
        permission: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
}

#[derive(Subcommand)]
enum AiAction {
    Plan { prompt: String },
    Review {
        plan_file: PathBuf,
        #[arg(long, default_value = "rustio.schema.json")]
        schema: PathBuf,
    },
    Apply {
        plan_file: PathBuf,
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let out: Result<(), String> = match cli.command {
        Command::New { kind } => match kind {
            NewKind::Project { name } => scaffold::project(&name),
            NewKind::App { name } => scaffold::app(&name),
        },
        Command::Migrate { action } => match action {
            MigrateAction::Apply { db, dir } => tokio_run(migrate_apply(db, dir)),
            MigrateAction::Generate { name, dir } => migrations::generate(&dir, &name)
                .map(|p| println!("created {}", p.display()))
                .map_err(|e| e.to_string()),
            MigrateAction::Status { db, dir } => tokio_run(migrate_status(db, dir)),
        },
        Command::User { action } => tokio_run(user_cmd(action)),
        Command::Group { action } => tokio_run(group_cmd(action)),
        Command::Perm { action } => tokio_run(perm_cmd(action)),
        Command::Ai { action } => match action {
            AiAction::Plan { prompt } => ai_plan(&prompt),
            AiAction::Review { plan_file, schema } => ai_review(&plan_file, &schema),
            AiAction::Apply { plan_file, dir, yes } => ai_apply(&plan_file, &dir, yes),
        },
        Command::Schema { path } => print_schema(&path),
    };

    match out {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn tokio_run<F>(fut: F) -> Result<(), String>
where
    F: std::future::Future<Output = Result<(), String>>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(fut)
}

// ---- migrate -----------------------------------------------------------

async fn migrate_apply(db_url: String, dir: PathBuf) -> Result<(), String> {
    let db = Db::connect(&db_url).await.map_err(|e| e.to_string())?;
    auth::init_tables(&db).await.map_err(|e| e.to_string())?;
    let applied = migrations::apply(&db, &dir).await.map_err(|e| e.to_string())?;
    if applied.is_empty() {
        println!("no pending migrations");
    } else {
        for name in applied {
            println!("applied {name}");
        }
    }
    Ok(())
}

async fn migrate_status(db_url: String, dir: PathBuf) -> Result<(), String> {
    let db = Db::connect(&db_url).await.map_err(|e| e.to_string())?;
    let rows = migrations::status(&db, &dir).await.map_err(|e| e.to_string())?;
    if rows.is_empty() {
        println!("no migrations in {}", dir.display());
        return Ok(());
    }
    for (name, applied) in rows {
        let marker = if applied { "[x]" } else { "[ ]" };
        println!("{marker} {name}");
    }
    Ok(())
}

// ---- users -------------------------------------------------------------

async fn user_cmd(action: UserAction) -> Result<(), String> {
    match action {
        UserAction::Create { email, password, role, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            auth::init_tables(&db).await.map_err(|e| e.to_string())?;
            let role = Role::parse(&role).map_err(|e| e.to_string())?;
            let password = resolve_password(password)?;
            let id = auth::create_user(&db, &email, &password, role)
                .await
                .map_err(|e| e.to_string())?;
            println!("created user #{id} ({email}) as {}", role.as_str());
            Ok(())
        }
        UserAction::SetPassword { email, password, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let user = auth::find_user_by_email(&db, &email)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no user with email {email}"))?;
            let password = resolve_password(password)?;
            auth::set_password(&db, user.id, &password)
                .await
                .map_err(|e| e.to_string())?;
            println!("updated password for {email}");
            Ok(())
        }
        UserAction::AddToGroup { email, group, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let user = auth::find_user_by_email(&db, &email)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no user with email {email}"))?;
            let gid = group_id_by_name(&db, &group).await?;
            auth::add_user_to_group(&db, user.id, gid)
                .await
                .map_err(|e| e.to_string())?;
            println!("added {email} to group {group}");
            Ok(())
        }
    }
}

fn resolve_password(provided: Option<String>) -> Result<String, String> {
    if let Some(p) = provided {
        return Ok(p);
    }
    rpassword::prompt_password("Password: ").map_err(|e| e.to_string())
}

// ---- groups ------------------------------------------------------------

async fn group_cmd(action: GroupAction) -> Result<(), String> {
    match action {
        GroupAction::Create { name, description, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            auth::init_tables(&db).await.map_err(|e| e.to_string())?;
            let id = auth::create_group(&db, &name, &description)
                .await
                .map_err(|e| e.to_string())?;
            println!("created group #{id} ({name})");
            Ok(())
        }
        GroupAction::Grant { group, permission, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let gid = group_id_by_name(&db, &group).await?;
            auth::grant_to_group(&db, gid, &permission)
                .await
                .map_err(|e| e.to_string())?;
            println!("granted {permission} to {group}");
            Ok(())
        }
    }
}

async fn group_id_by_name(db: &Db, name: &str) -> Result<i64, String> {
    use sqlx::Row as _;
    let row = sqlx::query("SELECT id FROM rustio_groups WHERE name = $1")
        .bind(name)
        .fetch_optional(db.pool())
        .await
        .map_err(|e| e.to_string())?;
    let row = row.ok_or_else(|| format!("no group named {name}"))?;
    row.try_get::<i64, _>("id").map_err(|e| e.to_string())
}

// ---- permissions -------------------------------------------------------

async fn perm_cmd(action: PermAction) -> Result<(), String> {
    match action {
        PermAction::List { db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            use sqlx::Row as _;
            let rows = sqlx::query("SELECT name, description FROM rustio_permissions ORDER BY name")
                .fetch_all(db.pool())
                .await
                .map_err(|e| e.to_string())?;
            if rows.is_empty() {
                println!("no permissions registered yet");
                return Ok(());
            }
            for r in rows {
                let name: String = r.try_get("name").map_err(|e| e.to_string())?;
                let desc: String = r.try_get("description").map_err(|e| e.to_string())?;
                if desc.is_empty() {
                    println!("{name}");
                } else {
                    println!("{name}  — {desc}");
                }
            }
            Ok(())
        }
        PermAction::GrantUser { email, permission, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let user = auth::find_user_by_email(&db, &email)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no user with email {email}"))?;
            auth::grant_to_user(&db, user.id, &permission)
                .await
                .map_err(|e| e.to_string())?;
            println!("granted {permission} to {email}");
            Ok(())
        }
    }
}

// ---- ai ----------------------------------------------------------------

// 0.9.x port: `ai_plan` now needs a schema to plan against. The CLI loads
// the project's `rustio.schema.json` (or an empty schema if the file isn't
// there yet) and feeds it to `generate_plan`. The output is the bare Plan
// JSON — same on-disk shape the previous CLI emitted.
fn ai_plan(prompt: &str) -> Result<(), String> {
    let schema = load_schema(Path::new("rustio.schema.json"))?;
    let result = generate_plan(&schema, None, PlanRequest::new(prompt))
        .map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&result.plan).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn ai_review(plan_file: &Path, schema_file: &Path) -> Result<(), String> {
    let plan_text = std::fs::read_to_string(plan_file).map_err(|e| e.to_string())?;
    // 0.9.x: `load_plan` accepts either a raw Plan JSON or a wrapped
    // PlanDocument. The CLI doesn't care which — it just needs a `Plan`
    // reference to feed `review_plan`.
    let loaded = load_plan(&plan_text).map_err(|e| e.to_string())?;
    let plan = match &loaded {
        LoadedPlan::Document(doc) => doc.plan.clone(),
        LoadedPlan::RawPlan(p) => p.clone(),
    };
    let schema = load_schema(schema_file)?;
    let review = review_plan(&schema, &plan, None).map_err(|e| e.to_string())?;
    // PlanReview isn't directly Serialize; project the public fields the
    // previous CLI surface advertised so external tooling stays parsing-
    // compatible.
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "risk": review.risk,
        "impact": review.impact,
        "warnings": review.warnings,
    }))
    .map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn ai_apply(plan_file: &Path, dir: &Path, allow_destructive: bool) -> Result<(), String> {
    let plan_text = std::fs::read_to_string(plan_file).map_err(|e| e.to_string())?;
    // 0.9.x execute requires a saved PlanDocument (so the apply has the
    // reviewer's risk/impact verdict alongside the plan). Refuse a raw
    // Plan with a clear pointer rather than fabricating a doc on the fly.
    let loaded = load_plan(&plan_text).map_err(|e| e.to_string())?;
    let doc: PlanDocument = match loaded {
        LoadedPlan::Document(d) => d,
        LoadedPlan::RawPlan(_) => {
            return Err(format!(
                "`{}` is a raw plan. `ai apply` needs a saved PlanDocument — re-run `ai plan` with the new --save flag once it lands.",
                plan_file.display(),
            ));
        }
    };
    let opts = ExecuteOptions { allow_destructive };
    let outcome = execute_plan_document(dir, &doc, &opts, None).map_err(|e| e.to_string())?;
    // 0.9.x renames `files_written` → `generated_files` and the items
    // are project-relative `String`s, not full `PathBuf`s.
    for file in outcome.generated_files {
        println!("wrote {file}");
    }
    Ok(())
}

fn print_schema(path: &Path) -> Result<(), String> {
    let schema = load_schema(path)?;
    let json = serde_json::to_string_pretty(&schema).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn load_schema(path: &Path) -> Result<Schema, String> {
    if !path.exists() {
        // Empty placeholder schema — same shape an empty `Admin::new()`
        // would emit. Caller can populate it via `from_admin` after the
        // project compiles.
        return Ok(Schema {
            version: SCHEMA_VERSION,
            rustio_version: env!("CARGO_PKG_VERSION").into(),
            models: Vec::new(),
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

// ---- scaffold ----------------------------------------------------------

mod scaffold {
    use std::fs;
    use std::path::Path;

    pub fn project(name: &str) -> Result<(), String> {
        let root = Path::new(name);
        if root.exists() {
            return Err(format!("{name} already exists"));
        }
        fs::create_dir_all(root.join("src").join("apps")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("migrations")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("templates")).map_err(|e| e.to_string())?;

        fs::write(root.join("Cargo.toml"), cargo_toml(name)).map_err(|e| e.to_string())?;
        fs::write(root.join("src").join("main.rs"), MAIN_RS).map_err(|e| e.to_string())?;
        fs::write(root.join("src").join("apps").join("mod.rs"), "").map_err(|e| e.to_string())?;
        fs::write(root.join(".gitignore"), GITIGNORE).map_err(|e| e.to_string())?;
        fs::write(root.join(".env.example"), ENV_EXAMPLE).map_err(|e| e.to_string())?;

        println!("scaffolded {name}");
        println!();
        println!("next steps:");
        println!("  cd {name}");
        println!("  cp .env.example .env    # edit DATABASE_URL, MEILI_URL");
        println!("  cargo run");
        Ok(())
    }

    pub fn app(name: &str) -> Result<(), String> {
        let app_dir = Path::new("src").join("apps").join(name);
        if app_dir.exists() {
            return Err(format!("app {name} already exists"));
        }
        fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
        fs::write(app_dir.join("mod.rs"), "pub mod models;\n").map_err(|e| e.to_string())?;
        fs::write(app_dir.join("models.rs"), APP_MODELS_RS).map_err(|e| e.to_string())?;
        println!("created app {name}");
        Ok(())
    }

    fn cargo_toml(name: &str) -> String {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
rustio-core = "1.0"
tokio = {{ version = "1", features = ["rt-multi-thread", "macros"] }}
chrono = {{ version = "0.4", features = ["serde"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
env_logger = "0.11"
log = "0.4"
"#
        )
    }

    const MAIN_RS: &str = r#"use std::net::SocketAddr;
use std::time::Duration;

use rustio_core::admin::{register_admin_routes, Admin};
use rustio_core::background;
use rustio_core::middleware::{self, RateLimiter};
use rustio_core::migrations;
use rustio_core::orm::Db;
use rustio_core::router::Router;
use rustio_core::server::Server;
use rustio_core::templates::Templates;
use rustio_core::auth;

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let db_url = std::env::var("DATABASE_URL")?;
    let db = Db::connect(&db_url).await?;
    auth::init_tables(&db).await?;
    migrations::apply(&db, "migrations").await?;
    background::spawn_housekeeping(db.clone());

    let template_dir = std::env::var("RUSTIO_TEMPLATE_DIR").unwrap_or_else(|_| "templates".into());
    let templates = Templates::new(Some(template_dir.into()))?;

    let admin = Admin::new();
    admin.seed_permissions(&db).await?;

    let router = Router::new()
        .middleware(middleware::rate_limit(RateLimiter::default_limits()))
        .middleware(middleware::logger)
        .middleware(middleware::security_headers)
        .middleware(middleware::gzip)
        .middleware(middleware::csrf_protect);
    let router = register_admin_routes(router, admin, db, templates);

    let addr: SocketAddr = "127.0.0.1:8000".parse()?;
    Server::new(router, addr).run().await?;
    Ok(())
}
"#;

    const APP_MODELS_RS: &str = r#"// Add your models here. Example:
//
// use chrono::{DateTime, Utc};
// use rustio_core::{Error, Model, Row, RustioAdmin, Value};
//
// #[derive(Debug, RustioAdmin)]
// pub struct Post {
//     pub id: i64,
//     pub title: String,
//     pub body: String,
//     pub published: bool,
//     pub created_at: DateTime<Utc>,
// }
"#;

    const GITIGNORE: &str = "/target\n.env\n";

    const ENV_EXAMPLE: &str = r#"DATABASE_URL=postgres://postgres:dev@localhost/myapp
MEILI_URL=http://localhost:7700
# MEILI_MASTER_KEY=your-key-if-configured
RUST_LOG=info
"#;
}
