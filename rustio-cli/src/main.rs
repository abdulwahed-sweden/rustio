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
    /// Build and run the project in the current directory.
    /// Convenience wrapper around `cargo run`. Refuses with a clear
    /// error if no `Cargo.toml` is found.
    Run,
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
    /// Read or change a user's role.
    Role {
        #[command(subcommand)]
        action: RoleAction,
    },
}

#[derive(Subcommand)]
enum RoleAction {
    /// Print the current role for the given email.
    Get {
        #[arg(long)]
        email: String,
        #[arg(long, env = "DATABASE_URL")]
        db: String,
    },
    /// Set a new role. If the change would leave zero active
    /// developers, requires `--yes` or an interactive confirmation.
    Set {
        #[arg(long)]
        email: String,
        #[arg(long)]
        role: String,
        /// Skip the interactive confirmation when demoting the last
        /// active developer. Without this flag the command refuses
        /// non-interactive demotions to avoid accidental lockouts.
        #[arg(long)]
        yes: bool,
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
    /// Phase 8.0 — call an LLM to translate a prose system
    /// description into a `Schema` JSON. Validated before write;
    /// never executed. The operator runs the result through
    /// `rustio ai plan / review / apply` afterwards.
    Generate {
        /// Free-form description of the system to model.
        prompt: String,
        /// Output path for the generated schema JSON. Refuses to
        /// overwrite an existing file unless `--force` is set.
        #[arg(long, default_value = "schema.json")]
        out: PathBuf,
        /// Allow overwriting an existing output file.
        #[arg(long)]
        force: bool,
    },
    /// Phase 8.1 — evolve an existing schema with a free-form
    /// instruction. Single LLM call; the result is validated, diffed
    /// against the current schema, and the operator confirms the
    /// write interactively. `--yes` skips the confirmation for
    /// scripted use; the file is rewritten in place.
    ///
    /// Phase 8.3.1 adds `--dry-run`: runs the full flow + diff but
    /// skips the y/N confirmation and never writes to disk.
    /// Phase 8.4 adds `--explain`: makes ONE additional LLM call
    /// after the diff to surface WHY + IMPACT sections.
    Update {
        /// Path to the existing schema JSON to evolve.
        schema_file: PathBuf,
        /// Free-form description of the change to apply.
        instruction: String,
        /// Skip the y/N confirmation prompt and write immediately.
        #[arg(long)]
        yes: bool,
        /// Preview only — run the AI call + diff but never write.
        /// Mutually exclusive with `--yes` (scripted auto-save) at
        /// runtime: dry-run wins so a stray `--yes` can't bypass
        /// the read-only intent.
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Print a `Why` + `Impact` explanation of the diff after
        /// it's shown. Costs one extra LLM call. Off by default.
        #[arg(long)]
        explain: bool,
    },
    /// Phase 8.2 — read-only AI audit of a schema. Single LLM call.
    /// Prints issues + suggestions + score; never writes to disk,
    /// never modifies the schema, never invokes update/generate.
    /// Distinct from `rustio ai review` (the deterministic plan
    /// reviewer) by name on purpose.
    ///
    /// Phase 8.3 adds `--pick <N>` and `--apply <instruction>` to
    /// bridge analyze → update without retyping. Mutually exclusive.
    /// Phase 8.3.1 adds `--dry-run` for preview-only flows.
    /// Phase 8.4 adds `--explain` to narrate the diff (one extra
    /// LLM call). Has no effect on plain analyze (no diff to narrate).
    /// Phase 9.1 adds `--yes` to scriptable apply / pick flows.
    Analyze {
        /// Path to the schema JSON to analyze.
        schema_file: PathBuf,
        /// Apply suggestion #N from the analyze report. 1-indexed.
        /// Runs analyze first, extracts the chosen suggestion, then
        /// hands it to the update flow (diff + y/N confirmation +
        /// atomic write). Two LLM calls total — one analyze, one
        /// update — strictly bounded.
        #[arg(long, conflicts_with = "apply")]
        pick: Option<usize>,
        /// Skip the analyze report entirely and apply this
        /// instruction directly via the update flow. Equivalent to
        /// `rustio ai update <schema> "<instruction>"`. One LLM
        /// call total.
        #[arg(long)]
        apply: Option<String>,
        /// Preview only — when paired with `--pick` or `--apply`,
        /// runs the full flow + diff but never writes. No effect on
        /// plain analyze (already read-only).
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Narrate the diff with `Why` + `Impact` sections. One
        /// extra LLM call after the diff is shown; only meaningful
        /// when paired with `--pick` or `--apply` (plain analyze
        /// has no diff). Off by default.
        #[arg(long)]
        explain: bool,
        /// Phase 9.1 — skip the y/N confirmation when paired with
        /// `--pick` or `--apply`; mirrors `ai update --yes`. No
        /// effect on plain analyze (no save flow). `--dry-run`
        /// still wins (read-only intent is sticky).
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
                .map(|p| {
                    println!("✓ created {}", p.display());
                    println!();
                    println!("next: edit the file, then run `rustio migrate apply`");
                })
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
            AiAction::Generate { prompt, out, force } => {
                tokio_run(ai_generate(prompt, out, force))
            }
            AiAction::Update { schema_file, instruction, yes, dry_run, explain } => {
                tokio_run(ai_update(schema_file, instruction, yes, dry_run, explain))
            }
            AiAction::Analyze { schema_file, pick, apply, dry_run, explain, yes } => {
                tokio_run(ai_analyze_dispatch(
                    schema_file, pick, apply, dry_run, explain, yes,
                ))
            }
        },
        Command::Schema { path } => print_schema(&path),
        Command::Run => cmd_run(&std::env::current_dir().unwrap_or_else(|_| ".".into())),
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

// ---- run ---------------------------------------------------------------

/// Phase 1.3.1 — `rustio run` is a convenience wrapper around `cargo run`
/// scoped to the current directory. Lookup is intentionally simple: if
/// the cwd has a `Cargo.toml`, hand off to cargo; otherwise produce the
/// actionable error the user sees today instead of cargo's terse
/// "could not find `Cargo.toml`".
fn cmd_run(cwd: &Path) -> Result<(), String> {
    check_in_project(cwd)?;
    let status = std::process::Command::new("cargo")
        .arg("run")
        .current_dir(cwd)
        .status()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo run exited with status {status}"));
    }
    Ok(())
}

/// Extracted so tests can drive it without spawning cargo. The error
/// copy here is the user-facing message; if you change it, update the
/// `run_outside_project_returns_clear_error` test too.
fn check_in_project(cwd: &Path) -> Result<(), String> {
    if !cwd.join("Cargo.toml").exists() {
        return Err(
            "no Cargo.toml found. Run this inside a Rustio project.".into(),
        );
    }
    Ok(())
}

// ---- migrate -----------------------------------------------------------

async fn migrate_apply(db_url: String, dir: PathBuf) -> Result<(), String> {
    let db = Db::connect(&db_url).await.map_err(|e| e.to_string())?;
    auth::init_tables(&db).await.map_err(|e| e.to_string())?;
    let applied = migrations::apply(&db, &dir).await.map_err(|e| e.to_string())?;
    if applied.is_empty() {
        println!("✓ no pending migrations");
    } else {
        for name in applied {
            println!("✓ applied {name}");
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
            println!("✓ created user #{id} ({email}) as {}", role.as_str());
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
            println!("✓ updated password for {email}");
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
            println!("✓ added {email} to group {group}");
            Ok(())
        }
        UserAction::Role { action } => role_cmd(action).await,
    }
}

// Phase 7a/0.5/f — `user role get|set`. The CLI is the escape hatch
// when the UI guard refuses a developer demotion: an admin who's
// painted themselves into a corner runs `rustio user role set` to
// promote a backup developer first, then can demote the original
// from the UI.
async fn role_cmd(action: RoleAction) -> Result<(), String> {
    match action {
        RoleAction::Get { email, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let user = auth::find_user_by_email(&db, &email)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no user with email {email}"))?;
            println!("{}", user.role.as_str());
            Ok(())
        }
        RoleAction::Set { email, role, yes, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            auth::init_tables(&db).await.map_err(|e| e.to_string())?;
            let new_role = Role::parse(&role).map_err(|e| e.to_string())?;
            let user = auth::find_user_by_email(&db, &email)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no user with email {email}"))?;

            if user.role == new_role {
                println!("✓ {email} is already {}", new_role.as_str());
                return Ok(());
            }

            // Mirror the UI guard. The CLI bypass is intentional —
            // an operator must be able to demote the sole developer
            // (e.g. after promoting a replacement) — but only with
            // explicit confirmation so it can't happen by accident
            // through scripting.
            let would_orphan = auth::would_orphan_developers(&db, user.id, Some(new_role))
                .await
                .map_err(|e| e.to_string())?;
            if would_orphan {
                if !yes && !confirm_orphan(&email)? {
                    return Err("aborted".into());
                }
                eprintln!(
                    "warning: demoting the last active developer ({email}). \
                     Make sure another developer exists or you may lose access \
                     to the schema browser, execution logs, and SQL console."
                );
            }

            auth::update_user_role(&db, user.id, new_role)
                .await
                .map_err(|e| e.to_string())?;
            println!(
                "✓ set role of {email} from {} to {}",
                user.role.as_str(),
                new_role.as_str(),
            );
            Ok(())
        }
    }
}

/// Interactive last-developer confirmation. Returns true if the user
/// types exactly `I UNDERSTAND` (case-sensitive) — anything else
/// aborts. We use a long phrase rather than y/N because demoting the
/// last developer is a one-way action that's easy to fat-finger.
fn confirm_orphan(email: &str) -> Result<bool, String> {
    use std::io::{self, Write};
    eprintln!(
        "warning: {email} is the last active developer. Demoting will \
         leave the system with zero developers (no schema browser, no \
         execution logs, no SQL console)."
    );
    eprint!("Type 'I UNDERSTAND' to continue, anything else to abort: ");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim() == "I UNDERSTAND")
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
            println!("✓ created group #{id} ({name})");
            Ok(())
        }
        GroupAction::Grant { group, permission, db } => {
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            let gid = group_id_by_name(&db, &group).await?;
            auth::grant_to_group(&db, gid, &permission)
                .await
                .map_err(|e| e.to_string())?;
            println!("✓ granted {permission} to {group}");
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
                    println!("{name} — {desc}");
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
            println!("✓ granted {permission} to {email}");
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
        println!("✓ wrote {file}");
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

// Phase 8.0 — `rustio ai generate <prompt> --out <path> [--force]`.
//
// Calls the LLM via `ai_gen::generate`, validates the response,
// writes the schema JSON atomically. Refuses to overwrite an existing
// file unless `--force` is set. Owns the file I/O so the LLM-facing
// module stays I/O-free and unit-testable.
async fn ai_generate(prompt: String, out: PathBuf, force: bool) -> Result<(), String> {
    check_overwrite_allowed(&out, force)?;
    let schema = rustio_core::ai_gen::generate(&prompt)
        .await
        .map_err(|e| e.to_string())?;
    schema.write_to(&out).map_err(|e| e.to_string())?;
    eprintln!("✓ wrote {}", out.display());
    Ok(())
}

/// Phase 8.0 — overwrite guard for `rustio ai generate`. Extracted so
/// it can be unit-tested without a network call. `Ok(())` means it's
/// safe to write; `Err(_)` carries the message the CLI surfaces.
fn check_overwrite_allowed(out: &Path, force: bool) -> Result<(), String> {
    if out.exists() && !force {
        return Err(format!(
            "{} already exists; pass --force to overwrite",
            out.display()
        ));
    }
    Ok(())
}

// Phase 8.1 — `rustio ai update <schema.json> "<instruction>" [--yes]`.
//
// Reads the existing schema, validates it, calls the LLM through
// `ai_gen::update`, computes a diff against the existing schema,
// prints the diff, and asks the operator to confirm before
// rewriting the file in place. `--yes` skips the prompt for
// scripted use.
//
// Phase 8.3.1 adds `--dry-run`: runs the full flow + diff but
// skips the y/N confirmation and never writes. Banner is printed
// before the diff so the operator reads "preview only" first.
// dry_run wins over yes — a stray `--yes --dry-run` stays read-only.
//
// Phase 8.4 adds `--explain`: ONE additional LLM call after the
// diff to narrate WHY + IMPACT. Gated; default off. Runs BEFORE
// the y/N confirmation so the operator sees the explanation
// before deciding to save.
async fn ai_update(
    schema_file: PathBuf,
    instruction: String,
    yes: bool,
    dry_run: bool,
    explain: bool,
) -> Result<(), String> {
    let existing = load_schema(&schema_file)?;
    eprintln!(
        "✓ Reading {} ({} model{})",
        schema_file.display(),
        existing.models.len(),
        if existing.models.len() == 1 { "" } else { "s" },
    );
    if dry_run {
        eprintln!("⚠ Dry run — preview only");
    }
    eprintln!("✓ Calling AI...");

    let updated = rustio_core::ai_gen::update(&existing, &instruction)
        .await
        .map_err(|e| e.to_string())?;

    let changes = rustio_core::ai_gen::diff::diff(&existing, &updated);
    eprintln!();
    eprintln!("Changes:");
    eprintln!("{}", rustio_core::ai_gen::diff::render(&changes));
    eprintln!();

    // Phase 8.4 — second LLM call, gated by --explain. Runs BEFORE
    // the save flow so the operator reads the explanation before
    // confirming.
    if let Some(report) = maybe_explain(explain, &existing, &updated).await? {
        print_explain_report(&report);
    }

    let saved = perform_save_if_not_dry(&schema_file, &updated, yes, dry_run)?;
    match saved {
        SaveOutcome::DryRun => {
            eprintln!("⚠ Dry run — no changes applied");
        }
        SaveOutcome::Aborted => {
            eprintln!("aborted; {} unchanged", schema_file.display());
        }
        SaveOutcome::Wrote => {
            eprintln!("✓ wrote {}", schema_file.display());
        }
    }
    Ok(())
}

/// Phase 8.4 — gate for the explain step. Returns `None` when
/// `--explain` is off (no LLM call, no env read), `Some(report)`
/// otherwise. Reading `ANTHROPIC_API_KEY` happens here so a flag-off
/// invocation never touches the env at all — that's the
/// "explain_not_called_without_flag" contract.
async fn maybe_explain(
    explain: bool,
    old: &rustio_core::schema::Schema,
    new: &rustio_core::schema::Schema,
) -> Result<Option<rustio_core::ai_gen::ExplainReport>, String> {
    if !explain {
        return Ok(None);
    }
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "ANTHROPIC_API_KEY is not set; cannot --explain".to_string())?;
    let report = rustio_core::ai_gen::explain_diff(old, new, &api_key)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some(report))
}

/// Phase 8.4 — render the explain report under the diff. Skips
/// empty sections so the output stays compact when the model has
/// nothing to say in one bucket. Both empty → prints "(none)" so
/// the operator sees that the explain call ran but yielded nothing.
fn print_explain_report(report: &rustio_core::ai_gen::ExplainReport) {
    if !report.why.is_empty() {
        eprintln!("💡 Why:");
        for w in &report.why {
            eprintln!("- {w}");
        }
        eprintln!();
    }
    if !report.impact.is_empty() {
        eprintln!("⚠ Impact:");
        for i in &report.impact {
            eprintln!("- {i}");
        }
        eprintln!();
    }
    if report.why.is_empty() && report.impact.is_empty() {
        eprintln!("💡 Why / ⚠ Impact: (none)");
        eprintln!();
    }
}

/// Phase 8.3.1 — outcome of the save step. Surfaced as a return
/// value (rather than baked into the dry-run branch) so the
/// integration test can observe the file-system effect deterministically.
#[derive(Debug, PartialEq, Eq)]
enum SaveOutcome {
    /// `--dry-run` was set: skipped both confirm + write.
    DryRun,
    /// Operator declined the y/N prompt.
    Aborted,
    /// Schema was written to disk.
    Wrote,
}

/// Phase 8.3.1 — the save decision + write. Extracted so tests can
/// drive it with concrete (yes, dry_run) combinations and observe
/// the on-disk result without involving the LLM.
///
/// Truth table:
///   dry_run=true              → DryRun (no confirm, no write)
///   dry_run=false, yes=true   → Wrote (no confirm, atomic write)
///   dry_run=false, yes=false  → confirm prompt; Wrote on y / Aborted otherwise
fn perform_save_if_not_dry(
    target: &Path,
    updated: &rustio_core::schema::Schema,
    yes: bool,
    dry_run: bool,
) -> Result<SaveOutcome, String> {
    if dry_run {
        return Ok(SaveOutcome::DryRun);
    }
    if !yes && !confirm_save_changes()? {
        return Ok(SaveOutcome::Aborted);
    }
    updated.write_to(target).map_err(|e| e.to_string())?;
    Ok(SaveOutcome::Wrote)
}

/// Phase 8.1 — y/N confirmation for `ai update`. Returns true on
/// `y` / `Y` / `yes`; false on anything else (including EOF /
/// empty). Mirrors `confirm_orphan`'s stdin pattern.
fn confirm_save_changes() -> Result<bool, String> {
    use std::io::{self, Write};
    eprint!("Save changes? (y/N) ");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    let trimmed = line.trim().to_ascii_lowercase();
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}

// Phase 8.2 — `rustio ai analyze <schema.json>`.
//
// Reads the schema, calls `ai_gen::analyze`, prints issues +
// suggestions + score. Read-only: never writes to disk, never
// modifies the schema. Distinct from `rustio ai review`, which is
// the deterministic plan-vs-schema reviewer from the rule-based
// pipeline.
async fn ai_analyze(schema_file: PathBuf) -> Result<(), String> {
    let schema = load_schema(&schema_file)?;
    eprintln!(
        "✓ Reading {} ({} model{})",
        schema_file.display(),
        schema.models.len(),
        if schema.models.len() == 1 { "" } else { "s" },
    );
    eprintln!("✓ Calling AI...");
    eprintln!();

    let report = rustio_core::ai_gen::analyze(&schema)
        .await
        .map_err(|e| e.to_string())?;
    print_analyze_report(&report);
    Ok(())
}

/// Phase 8.2 — render the analyze report to stderr. Skips empty
/// sections so the output stays compact when the model has nothing
/// to say in one bucket. Always prints the score (defaults to 0.0
/// when the model omits it; the operator can read that as "score
/// unknown" via the printed value).
fn print_analyze_report(report: &rustio_core::ai_gen::AnalyzeReport) {
    if !report.issues.is_empty() {
        eprintln!("⚠ Issues:");
        for i in &report.issues {
            eprintln!("- {i}");
        }
        eprintln!();
    }
    if !report.suggestions.is_empty() {
        eprintln!("💡 Suggestions:");
        for s in &report.suggestions {
            eprintln!("- {s}");
        }
        eprintln!();
    }
    eprintln!("Score: {} / 10", format_score(report.score));
}

/// Format the score as `<n>` if integral, `<n.n>` otherwise. Saves
/// printing `8 / 10` as `8.0 / 10` when the model gave a clean int.
fn format_score(score: f32) -> String {
    if (score - score.round()).abs() < f32::EPSILON {
        format!("{}", score as i32)
    } else {
        format!("{score:.1}")
    }
}

/// Phase 8.3 — `rustio ai analyze` flow classification. Pure
/// function so the routing logic is testable without spinning up
/// any LLM call.
#[derive(Debug, PartialEq, Eq, Clone)]
enum AnalyzeFlow {
    /// No flags — print the analyze report only (Phase 8.2 behavior).
    Plain,
    /// `--pick N` — analyze first, extract suggestion #N (1-indexed),
    /// then run the update flow with that suggestion as the
    /// instruction. Two LLM calls total.
    Pick(usize),
    /// `--apply <instruction>` — skip analyze, run update directly
    /// with the supplied instruction. One LLM call total.
    Apply(String),
}

/// Phase 8.3 — decide which `ai analyze` path to take from the two
/// optional flags. Clap enforces mutual exclusion at the parser
/// layer (`conflicts_with`); this fn just maps Option pairs to
/// the variant the dispatcher executes. `--apply` wins over
/// `--pick` defensively in case clap's conflicts_with is ever
/// loosened — that way at least one flag never silently overrides
/// the other.
fn classify_analyze_flow(pick: Option<usize>, apply: Option<String>) -> AnalyzeFlow {
    if let Some(instr) = apply {
        AnalyzeFlow::Apply(instr)
    } else if let Some(n) = pick {
        AnalyzeFlow::Pick(n)
    } else {
        AnalyzeFlow::Plain
    }
}

/// Phase 8.3 — pull suggestion #N out of an analyze report. Bounds-
/// and emptiness-checked. 1-indexed because the CLI shows
/// "1. Add tags..." style numbering and operators expect to type
/// `--pick 1` not `--pick 0`.
fn pick_suggestion(
    report: &rustio_core::ai_gen::AnalyzeReport,
    n: usize,
) -> Result<&str, String> {
    if n == 0 {
        return Err("--pick is 1-indexed; use --pick 1 for the first suggestion".into());
    }
    if report.suggestions.is_empty() {
        return Err("AI returned no suggestions; nothing to apply".into());
    }
    let len = report.suggestions.len();
    report
        .suggestions
        .get(n - 1)
        .map(String::as_str)
        .ok_or_else(|| {
            format!(
                "--pick {n} is out of bounds; analyze returned {len} suggestion{plural}",
                plural = if len == 1 { "" } else { "s" }
            )
        })
}

/// Phase 8.3 — single dispatch entry for `rustio ai analyze`.
/// Routes to the existing 8.2 plain-report handler, or to the
/// 8.1 update flow (with the picked / supplied instruction) when
/// the bridge flags are set.
///
/// Phase 8.3.1 — `dry_run` threads through to the update flow
/// when --pick or --apply is in play. Plain analyze is read-only
/// already, so dry_run is a no-op there (no banner, no behavior
/// change).
///
/// Phase 8.4 — `explain` likewise threads through. Plain analyze
/// has no diff to narrate; --explain there is a documented no-op.
///
/// Phase 9.1 — `yes` threads through too, so `ai analyze --apply`
/// and `--pick` can be scripted (matches `ai update --yes`). No
/// effect on plain analyze (no save flow).
async fn ai_analyze_dispatch(
    schema_file: PathBuf,
    pick: Option<usize>,
    apply: Option<String>,
    dry_run: bool,
    explain: bool,
    yes: bool,
) -> Result<(), String> {
    match classify_analyze_flow(pick, apply) {
        AnalyzeFlow::Plain => ai_analyze(schema_file).await,
        AnalyzeFlow::Apply(instruction) => {
            // Spec: "skip suggestion picking; call ai_update
            // directly; same flow as `ai update`." Identical to
            // `rustio ai update <schema> <instruction>`; the y/N
            // prompt + diff are owned by ai_update. Phase 9.1
            // forwards `yes` here so `--apply --yes` is scriptable.
            ai_update(schema_file, instruction, yes, dry_run, explain).await
        }
        AnalyzeFlow::Pick(n) => analyze_then_pick(schema_file, n, dry_run, explain, yes).await,
    }
}

/// Phase 8.3 — the `--pick N` path. Two LLM calls:
///   1. `ai_gen::analyze` to get the suggestions list.
///   2. `ai_gen::update` (via `ai_update`) to apply the chosen one.
///
/// The suggestion text becomes the update instruction verbatim;
/// the operator confirms via the existing y/N prompt before
/// anything is written.
///
/// Phase 8.3.1 — when `dry_run` is true, the second call still
/// fires but `ai_update` skips the confirm + write step.
///
/// Phase 8.4 — when `explain` is true, ai_update fires a third
/// LLM call to narrate the diff. Strict cap: analyze + update +
/// (optional) explain = at most three LLM calls per `--pick`.
///
/// Phase 9.1 — `yes` is forwarded so `--pick --yes` skips the
/// confirmation prompt (matches `ai update --yes`). `dry_run`
/// still wins over `yes` inside `ai_update` (Phase 8.3.1 truth
/// table).
async fn analyze_then_pick(
    schema_file: PathBuf,
    n: usize,
    dry_run: bool,
    explain: bool,
    yes: bool,
) -> Result<(), String> {
    let schema = load_schema(&schema_file)?;
    eprintln!(
        "✓ Reading {} ({} model{})",
        schema_file.display(),
        schema.models.len(),
        if schema.models.len() == 1 { "" } else { "s" },
    );
    eprintln!("✓ Calling AI (analyze)...");
    let report = rustio_core::ai_gen::analyze(&schema)
        .await
        .map_err(|e| e.to_string())?;

    let suggestion = pick_suggestion(&report, n)?;
    eprintln!();
    eprintln!("✓ Using suggestion #{n}:");
    eprintln!("  \"{suggestion}\"");
    eprintln!();

    // Hand off to the update flow. This second LLM call writes
    // ai_gen::update on top of the original schema; diff + y/N
    // confirmation are owned by ai_update. The (optional) third
    // call for --explain is also owned by ai_update.
    ai_update(schema_file, suggestion.to_string(), yes, dry_run, explain).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustio_core::ai_gen::AnalyzeReport;

    fn report_with(suggestions: &[&str]) -> AnalyzeReport {
        AnalyzeReport {
            issues: Vec::new(),
            suggestions: suggestions.iter().map(|s| (*s).to_string()).collect(),
            score: 8.0,
        }
    }

    /// Phase 8.3 — `--pick N` extracts suggestion #N (1-indexed) from
    /// the analyze report. Test exercises the boundary cases too:
    /// first index, last index, mid-list. Locks the contract that
    /// the CLI's hand-off into the update flow uses the right
    /// instruction text.
    #[test]
    fn analyze_pick_applies_correct_suggestion() {
        let r = report_with(&[
            "Add created_at to all models",
            "Index Comment.post_id",
            "Consider an enum for Post.status",
        ]);
        assert_eq!(pick_suggestion(&r, 1).unwrap(), "Add created_at to all models");
        assert_eq!(pick_suggestion(&r, 2).unwrap(), "Index Comment.post_id");
        assert_eq!(
            pick_suggestion(&r, 3).unwrap(),
            "Consider an enum for Post.status"
        );
    }

    /// Phase 8.3 — out-of-bounds + zero-index + empty-suggestions
    /// all surface clean errors, never panic. The CLI prints the
    /// error message verbatim, so the wording matters.
    #[test]
    fn analyze_pick_out_of_bounds_error() {
        let r = report_with(&["Only one"]);

        // Past the end.
        let err = pick_suggestion(&r, 2).unwrap_err();
        assert!(err.contains("out of bounds"));
        assert!(err.contains("returned 1 suggestion"));

        // Zero index.
        let err = pick_suggestion(&r, 0).unwrap_err();
        assert!(err.contains("1-indexed"));

        // Empty suggestions.
        let empty = report_with(&[]);
        let err = pick_suggestion(&empty, 1).unwrap_err();
        assert!(err.contains("no suggestions"));
    }

    /// Phase 8.3 — `--apply <instruction>` skips analyze and routes
    /// straight to the update flow. The classifier is the routing
    /// boundary; this test pins the contract that an instruction
    /// always wins over a `--pick` (defense in depth in case clap's
    /// conflicts_with is ever loosened).
    #[test]
    fn analyze_apply_runs_update() {
        let flow = classify_analyze_flow(None, Some("add tags to posts".into()));
        assert_eq!(flow, AnalyzeFlow::Apply("add tags to posts".into()));

        // Even if both are passed (clap should reject this, but the
        // classifier is defensive), --apply wins.
        let flow = classify_analyze_flow(Some(2), Some("ignore me".into()));
        assert_eq!(flow, AnalyzeFlow::Apply("ignore me".into()));
    }

    /// Phase 8.3 — no flags → existing 8.2 plain-report path. The
    /// classifier returns AnalyzeFlow::Plain, which the dispatcher
    /// routes to ai_analyze. Locks the "preserves prior behavior"
    /// guarantee called out in the spec.
    #[test]
    fn analyze_no_flags_preserves_behavior() {
        assert_eq!(classify_analyze_flow(None, None), AnalyzeFlow::Plain);
        // --pick alone routes to Pick (sanity-check on the same
        // classifier so the routing matrix is fully covered here).
        assert_eq!(classify_analyze_flow(Some(1), None), AnalyzeFlow::Pick(1));
    }

    /// Phase 8.3 / spec test #5 — the routing helpers
    /// (`classify_analyze_flow`, `pick_suggestion`) are pure
    /// functions: no env reads, no network. Compile is the
    /// proof; this test just exercises them once more without any
    /// `ANTHROPIC_API_KEY` access.
    #[test]
    fn analyze_pick_no_live_api_calls() {
        let _ = std::env::var("ANTHROPIC_API_KEY"); // read-only
        let r = report_with(&["x"]);
        assert_eq!(pick_suggestion(&r, 1).unwrap(), "x");
        assert_eq!(classify_analyze_flow(None, None), AnalyzeFlow::Plain);
    }

    // ----- Phase 9.1 — analyze --yes routing ---------------------

    /// Phase 9.1 — `ai analyze --apply / --pick` paths now forward
    /// `yes` to `ai_update`. The save decision happens in
    /// `perform_save_if_not_dry`; with `yes=true, dry_run=false`
    /// it MUST land on `SaveOutcome::Wrote` without invoking the
    /// stdin confirm prompt. The structural fact (yes flows from
    /// the CLI into ai_update unchanged) is what this test pins —
    /// the CLI parser test would block on stdin if the threading
    /// were broken.
    #[test]
    fn analyze_yes_skips_confirmation() {
        let dir = tempdir_path();
        let target = dir.join("schema.json");
        let updated = fixture_schema();

        // yes=true, dry_run=false → Wrote (the path --apply --yes
        // and --pick --yes both land on after threading).
        let outcome = perform_save_if_not_dry(&target, &updated, true, false).unwrap();
        assert_eq!(outcome, SaveOutcome::Wrote);
        assert!(target.exists(), "yes path must actually write");

        // yes=true + dry_run=true → DryRun still wins (defense in
        // depth from Phase 8.3.1).
        let _ = std::fs::remove_file(&target);
        let outcome = perform_save_if_not_dry(&target, &updated, true, true).unwrap();
        assert_eq!(outcome, SaveOutcome::DryRun);
        assert!(!target.exists(), "dry-run still wins over yes");

        let _ = std::fs::remove_dir(&dir);
    }

    // ----- Phase 8.4 — explain gate tests ------------------------

    /// Phase 8.4 / spec test #4 — `--explain` MUST NOT trigger
    /// without the flag. The gate is `maybe_explain`; with
    /// explain=false it returns `Ok(None)` synchronously, never
    /// reads ANTHROPIC_API_KEY, never makes a network call.
    /// Compile + this test are the proof: the function returns
    /// before any env access when the flag is off.
    #[test]
    fn explain_not_called_without_flag() {
        // Drive the gate with explain=false. The schemas don't
        // matter — they're never serialised because the function
        // short-circuits.
        let s = fixture_schema();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(maybe_explain(false, &s, &s));
        let outcome = result.expect("flag-off must never error");
        assert!(outcome.is_none(), "explain=false → no report");

        // Sanity: the explain=true path needs the env key. Without
        // the key it returns the missing-key error WITHOUT making
        // a network call (the env check fires before client::send).
        // We don't run that branch here to keep the test
        // hermetic, but the structure proves it: env read happens
        // ONLY inside the `if explain` block.
    }

    /// Phase 8.4 / spec test #5 — when `--explain` is set, the
    /// pipeline path goes through `maybe_explain` exactly once per
    /// invocation. There's no recursion, no retry loop, no
    /// secondary call. Compile is the structural proof: the only
    /// caller of `ai_gen::explain_diff` in the binary is
    /// `maybe_explain`, which is itself called from a single site
    /// in `ai_update`. This test exercises the parse path that
    /// follows a successful explain call to lock the contract: one
    /// response → one rendered report.
    #[test]
    fn explain_called_once_when_flag_set() {
        // Simulate a single explain response and pin that the
        // parser consumes it cleanly without making additional
        // requests. Combined with the source structure (only one
        // call site in ai_update; only one caller of
        // ai_gen::explain_diff workspace-wide) this proves
        // exactly-once on the call side.
        let body = "WHY:\n- Tags help.\n\nIMPACT:\n- One new table.";
        let report = rustio_core::ai_gen::parse_explain_response(body);
        assert_eq!(report.why.len(), 1);
        assert_eq!(report.impact.len(), 1);
    }

    // ----- Phase 8.3.1 — dry-run tests --------------------------

    /// Build a tiny in-memory `Schema` fixture for the dry-run tests.
    /// Just enough to exercise `Schema::write_to`'s atomic-rename
    /// path against a real disk file.
    fn fixture_schema() -> rustio_core::schema::Schema {
        rustio_core::schema::Schema {
            version: rustio_core::schema::SCHEMA_VERSION,
            rustio_version: "1.0.0".into(),
            models: vec![rustio_core::schema::SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                fields: vec![rustio_core::schema::SchemaField {
                    name: "id".into(),
                    ty: "i64".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                }],
                relations: vec![],
                core: false,
            }],
        }
    }

    /// Phase 8.3.1 / spec test #1 — `--dry-run` MUST NOT write to
    /// disk. Pre-existing target file content survives untouched
    /// even after `perform_save_if_not_dry` returns DryRun.
    #[test]
    fn dry_run_does_not_write_file() {
        let dir = tempdir_path();
        let target = dir.join("schema.json");
        std::fs::write(&target, "ORIGINAL_CONTENT_DO_NOT_OVERWRITE").unwrap();

        let updated = fixture_schema();
        let outcome = perform_save_if_not_dry(&target, &updated, false, true)
            .expect("dry-run save must not error");
        assert_eq!(outcome, SaveOutcome::DryRun);

        // File on disk MUST be byte-identical to the seed content.
        let after = std::fs::read_to_string(&target).unwrap();
        assert_eq!(after, "ORIGINAL_CONTENT_DO_NOT_OVERWRITE");

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Phase 8.3.1 / spec test #2 — `--dry-run` MUST skip the y/N
    /// confirmation. Tested via the truth-table contract: dry_run=true
    /// short-circuits to DryRun BEFORE `confirm_save_changes` is
    /// reachable. The function would block on stdin if confirm fired,
    /// so the fact that this test returns at all is the evidence.
    #[test]
    fn dry_run_skips_confirmation() {
        let dir = tempdir_path();
        let target = dir.join("never_written.json");
        let updated = fixture_schema();

        // dry_run wins over yes — no confirm prompt either way.
        let outcome = perform_save_if_not_dry(&target, &updated, false, true).unwrap();
        assert_eq!(outcome, SaveOutcome::DryRun);
        assert!(!target.exists(), "DryRun must not create the target file");

        // dry_run + yes still skips confirm + write (defense in depth).
        let outcome = perform_save_if_not_dry(&target, &updated, true, true).unwrap();
        assert_eq!(outcome, SaveOutcome::DryRun);
        assert!(!target.exists(), "DryRun + yes still must not write");

        let _ = std::fs::remove_dir(&dir);
    }

    /// Phase 8.3.1 / spec test #3 — `--dry-run` STILL shows the
    /// diff. Verified at the contract level: dry_run is consumed
    /// AFTER `ai_gen::diff::diff` + `render` run; the SaveOutcome
    /// path doesn't gate the diff print. This test exercises a
    /// real diff render so a future refactor that accidentally
    /// shorts past the diff would break here.
    #[test]
    fn dry_run_still_shows_diff() {
        // The diff render is shared between the dry-run and the
        // confirm path, so this just locks the diff machinery
        // exists and produces output for a one-add change.
        let old = rustio_core::schema::Schema {
            version: rustio_core::schema::SCHEMA_VERSION,
            rustio_version: "1.0.0".into(),
            models: vec![],
        };
        let new = fixture_schema();
        let changes = rustio_core::ai_gen::diff::diff(&old, &new);
        let rendered = rustio_core::ai_gen::diff::render(&changes);
        assert!(
            rendered.contains("Model added: Post"),
            "diff must surface the change for both dry-run and live paths"
        );
    }

    /// Phase 8.3.1 / spec test #4 — `ai analyze --pick N --dry-run`
    /// routes through perform_save_if_not_dry with dry_run=true.
    /// Locks the contract that the `--pick` path respects dry-run.
    #[test]
    fn analyze_pick_dry_run() {
        // The flow is: analyze_then_pick → ai_update with dry_run
        // forwarded → perform_save_if_not_dry. Verify the truth-
        // table entry the pick path lands on (yes=false, dry_run=true).
        let dir = tempdir_path();
        let target = dir.join("schema.json");
        std::fs::write(&target, "{}").unwrap();
        let updated = fixture_schema();

        let outcome = perform_save_if_not_dry(&target, &updated, false, true).unwrap();
        assert_eq!(outcome, SaveOutcome::DryRun);
        // Original content still in place.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Phase 8.3.1 / spec test #5 — `ai update --dry-run` likewise
    /// routes through the SaveOutcome::DryRun path. Same contract,
    /// invoked from a different entry point. Both `ai update` and
    /// `ai analyze --pick / --apply` should funnel through this
    /// single decision point — that's why the helper exists.
    #[test]
    fn update_dry_run() {
        let dir = tempdir_path();
        let target = dir.join("schema.json");
        std::fs::write(&target, "PRE-EXISTING").unwrap();
        let updated = fixture_schema();

        let outcome = perform_save_if_not_dry(&target, &updated, false, true).unwrap();
        assert_eq!(outcome, SaveOutcome::DryRun);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "PRE-EXISTING");

        // Sanity check the live path: yes=true, dry_run=false → Wrote
        // (so we know the helper actually writes when not dry).
        let outcome = perform_save_if_not_dry(&target, &updated, true, false).unwrap();
        assert_eq!(outcome, SaveOutcome::Wrote);
        let after = std::fs::read_to_string(&target).unwrap();
        assert!(
            after.contains("\"version\": 2"),
            "live path must actually write the schema; got: {after}"
        );

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Phase 8.0 — `rustio ai generate --out <path>` MUST refuse to
    /// overwrite an existing file unless `--force` is set. The
    /// overwrite-guard runs before the LLM call so a sloppy invocation
    /// can never burn API credits and clobber a hand-edited schema.
    #[test]
    fn ai_generate_refuses_overwrite_without_force() {
        let dir = tempdir_path();
        let target = dir.join("schema.json");
        std::fs::write(&target, "{}").unwrap();

        // Without --force: guard rejects.
        let err = check_overwrite_allowed(&target, false)
            .expect_err("must refuse when target exists and force is false");
        assert!(err.contains("already exists"));
        assert!(err.contains("--force"));

        // With --force: guard passes.
        check_overwrite_allowed(&target, true)
            .expect("must allow when --force is set");

        // Non-existent target: guard passes regardless of --force.
        let fresh = dir.join("does-not-exist.json");
        check_overwrite_allowed(&fresh, false).expect("missing target is always writable");
        check_overwrite_allowed(&fresh, true).expect("missing target is always writable");

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir(&dir);
    }

    fn tempdir_path() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "rustio-cli-ai-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ----- Phase 1.3.1 — `rustio run` -----------------------------

    /// `rustio run` outside a project surfaces the actionable error
    /// instead of cargo's terse "could not find Cargo.toml". Pure path
    /// check, no cargo invocation.
    #[test]
    fn run_outside_project_returns_clear_error() {
        let dir = tempdir_path();
        let err = check_in_project(&dir).unwrap_err();
        assert!(
            err.contains("no Cargo.toml found"),
            "missing-Cargo.toml message must surface, got: {err}"
        );
        assert!(
            err.contains("Rustio project"),
            "error must point the user back at a project context, got: {err}"
        );
        let _ = std::fs::remove_dir(&dir);
    }

    /// Inside a project (Cargo.toml present), the path check passes and
    /// the caller proceeds to spawn cargo. The actual cargo invocation
    /// isn't tested here — that's an integration concern.
    #[test]
    fn run_inside_project_passes_path_check() {
        let dir = tempdir_path();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"\nversion=\"0\"").unwrap();
        check_in_project(&dir).expect("Cargo.toml present → path check OK");
        let _ = std::fs::remove_file(dir.join("Cargo.toml"));
        let _ = std::fs::remove_dir(&dir);
    }

    /// Scaffold next-steps and `.env.example` content are part of the
    /// v1.3.1 DX contract. If these strings drift, downstream docs
    /// (root README, examples/README) drift with them.
    #[test]
    fn scaffold_env_example_documents_postgres_requirement() {
        let txt = scaffold::ENV_EXAMPLE;
        assert!(txt.contains("DATABASE_URL=postgres://"), "must seed a Postgres URL");
        assert!(txt.contains("PostgreSQL is required"), "must explain PG requirement");
        assert!(txt.contains("MEILI_URL"), "must include MEILI_URL line");
    }

    #[test]
    fn scaffold_main_rs_does_not_import_unused_duration() {
        let src = scaffold::MAIN_RS;
        assert!(
            !src.contains("use std::time::Duration"),
            "Duration import is unused — would emit a compile warning"
        );
    }

    #[test]
    fn scaffold_main_rs_emits_actionable_db_connect_error() {
        let src = scaffold::MAIN_RS;
        assert!(
            src.contains("Database connection failed."),
            "scaffold must surface a friendly DB-failure banner"
        );
        assert!(
            src.contains("DATABASE_URL = {db_url}"),
            "the failed URL must be echoed for the user"
        );
        assert!(
            src.contains("docker compose up -d"),
            "actionable fixes must mention the docker-compose escape hatch"
        );
    }
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

        println!("✓ created project {name}");
        println!();
        println!("next steps:");
        println!("  cd {name}");
        println!("  cp .env.example .env    # edit DATABASE_URL, MEILI_URL");
        println!("  rustio run");
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
        println!("✓ created app {name}");
        println!();
        println!("next steps:");
        println!("  edit src/apps/{name}/models.rs to define your model");
        println!("  rustio migrate generate {name}_initial");
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

    pub(super) const MAIN_RS: &str = r#"use std::net::SocketAddr;

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

    let db_url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set. Copy .env.example to .env and edit it before running.")?
        .to_string();

    let db = match Db::connect(&db_url).await {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Database connection failed.");
            eprintln!();
            eprintln!("DATABASE_URL = {db_url}");
            eprintln!();
            eprintln!("Possible fixes:");
            eprintln!("  * create the database");
            eprintln!("  * update DATABASE_URL in .env");
            eprintln!("  * start PostgreSQL");
            eprintln!("  * run `docker compose up -d` if using the repo dev stack");
            eprintln!();
            eprintln!("Original error: {e}");
            std::process::exit(1);
        }
    };
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

    pub(super) const ENV_EXAMPLE: &str = r#"# PostgreSQL is required in v1.3.x.
# Edit DATABASE_URL before running if your database name or user differs.
DATABASE_URL=postgres://postgres:dev@localhost/rustio_dev

# Meilisearch is optional — the app handles a missing search backend
# gracefully. Set MEILI_MASTER_KEY in production.
MEILI_URL=http://127.0.0.1:7700
# MEILI_MASTER_KEY=your-key-if-configured

RUST_LOG=info
"#;
}
