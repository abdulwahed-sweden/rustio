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
    Update {
        /// Path to the existing schema JSON to evolve.
        schema_file: PathBuf,
        /// Free-form description of the change to apply.
        instruction: String,
        /// Skip the y/N confirmation prompt and write immediately.
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
            AiAction::Generate { prompt, out, force } => {
                tokio_run(ai_generate(prompt, out, force))
            }
            AiAction::Update { schema_file, instruction, yes } => {
                tokio_run(ai_update(schema_file, instruction, yes))
            }
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
                println!("{email} is already {}", new_role.as_str());
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
                "set role of {email} from {} to {}",
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
        "WARNING: {email} is the last active developer. Demoting will \
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
    eprintln!("wrote {}", out.display());
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
async fn ai_update(
    schema_file: PathBuf,
    instruction: String,
    yes: bool,
) -> Result<(), String> {
    let existing = load_schema(&schema_file)?;
    eprintln!(
        "✓ Reading {} ({} model{})",
        schema_file.display(),
        existing.models.len(),
        if existing.models.len() == 1 { "" } else { "s" },
    );
    eprintln!("✓ Calling AI...");

    let updated = rustio_core::ai_gen::update(&existing, &instruction)
        .await
        .map_err(|e| e.to_string())?;

    let changes = rustio_core::ai_gen::diff::diff(&existing, &updated);
    eprintln!();
    eprintln!("Changes:");
    eprintln!("{}", rustio_core::ai_gen::diff::render(&changes));
    eprintln!();

    if !yes && !confirm_save_changes()? {
        eprintln!("aborted; {} unchanged", schema_file.display());
        return Ok(());
    }

    updated.write_to(&schema_file).map_err(|e| e.to_string())?;
    eprintln!("wrote {}", schema_file.display());
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
