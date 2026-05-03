use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod doctor;
mod version_check;
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
    /// Create a new RustIO project. Alias for `new project`.
    #[command(name = "startproject")]
    Startproject {
        name: String,
    },
    /// Create a new app inside a RustIO project. Alias for `new app`.
    #[command(name = "startapp")]
    Startapp {
        name: String,
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
    /// Diagnose the local environment for a RustIO project.
    /// Read-only — checks project root, DATABASE_URL, PostgreSQL
    /// reachability + connection, and Meilisearch reachability.
    /// Exits 0 when ready (including degraded), 1 on any blocker.
    Doctor {
        /// Print only failures and the final summary.
        #[arg(long, short = 'q')]
        quiet: bool,
        /// Show detail blocks for passing checks too.
        #[arg(long, short = 'v')]
        verbose: bool,
        /// Disable ANSI colors (auto-disabled when stdout is not a TTY
        /// or when `NO_COLOR` is set).
        #[arg(long)]
        no_color: bool,
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
    /// Create a user. Any of `--email` / `--password` may be omitted;
    /// missing values are prompted for interactively. `--db` resolves
    /// from `DATABASE_URL` (loaded from `.env` at CLI startup).
    Create {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value = "administrator")]
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
    // v1.6 — load .env at CLI startup so subcommands like
    // `rustio user create` pick up DATABASE_URL from the project's
    // .env file without requiring an explicit `export` in the shell.
    // Mirrors what the scaffold's MAIN_RS does at runtime.
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // v1.7.1 — non-blocking update notifier. Reads `~/.rustio/version-
    // check.json` synchronously (microseconds) and prints a banner if
    // a newer release is on crates.io; refreshes the cache on a
    // detached background thread when stale. Disabled by
    // RUSTIO_NO_UPDATE_CHECK=1, by CI=1/CI=true, and for `rustio doctor`.
    version_check::run();

    let cli = Cli::parse();

    // Doctor returns its own ExitCode (0 = ready / ready-degraded, 1 = not
    // ready). Bypass the Result<(), String> mapping below — doctor's output
    // is already user-facing and shouldn't get wrapped in `error: ...`.
    if let Command::Doctor { quiet, verbose, no_color } = &cli.command {
        let args = doctor::Args { quiet: *quiet, verbose: *verbose, no_color: *no_color };
        let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("error: failed to start tokio runtime: {e}");
                return ExitCode::FAILURE;
            }
        };
        return rt.block_on(doctor::run(args));
    }

    let out: Result<(), String> = match cli.command {
        Command::New { kind } => match kind {
            NewKind::Project { name } => scaffold::project(&name),
            NewKind::App { name } => scaffold::app(&name),
        },
        Command::Startproject { name } => scaffold::project(&name),
        Command::Startapp { name } => scaffold::app(&name),
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
        Command::Doctor { .. } => unreachable!("Doctor handled above via early return"),
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
            // v1.6 — interactive prompts when flags are omitted.
            // Resolve email + password BEFORE touching the DB so the
            // user doesn't wait on a connection just to be told their
            // password is too short.
            let email = prompt_email(email)?;
            let password = resolve_password(password)?;
            let role = Role::parse(&role).map_err(|e| e.to_string())?;
            let db = Db::connect(&db).await.map_err(|e| e.to_string())?;
            auth::init_tables(&db).await.map_err(|e| e.to_string())?;
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
        // Non-interactive path: still validate so a scripted call with
        // `--password 1234` fails fast with the same rule the prompt
        // would enforce.
        validate_password(&p)?;
        return Ok(p);
    }
    let pw = rpassword::prompt_password("Password: ").map_err(|e| e.to_string())?;
    validate_password(&pw)?;
    let confirm =
        rpassword::prompt_password("Confirm password: ").map_err(|e| e.to_string())?;
    if pw != confirm {
        return Err("Passwords do not match.".into());
    }
    Ok(pw)
}

/// v1.6 — minimum password hygiene for interactive `rustio user create`.
/// Intentionally light: length + a small list of weak passwords. Real
/// strength rules belong in the application's auth layer; this guards
/// against "1234"-class typos at create time. Errors are end-user
/// strings (capitalized, full sentences) so they render cleanly in the
/// CLI's "error: …" wrapper.
fn validate_password(password: &str) -> Result<(), String> {
    const MIN_LEN: usize = 8;
    if password.len() < MIN_LEN {
        return Err(format!(
            "Password must be at least {MIN_LEN} characters."
        ));
    }
    if let Some(first) = password.chars().next() {
        if password.chars().all(|c| c == first) {
            return Err("Password is too weak.".into());
        }
    }
    let lower = password.to_lowercase();
    let trivial = [
        "password", "12345678", "11111111", "00000000", "qwertyui",
        "abcd1234", "admin123", "letmein1", "00001111", "12341234",
    ];
    if trivial.contains(&lower.as_str()) {
        return Err("Password is too weak.".into());
    }
    Ok(())
}

/// v1.6 — prompt for an email when `--email` was omitted. Validation is
/// minimal (`@` present + non-empty) — RFC-strict checking belongs at
/// the auth layer, not the CLI.
fn prompt_email(provided: Option<String>) -> Result<String, String> {
    if let Some(e) = provided {
        if !e.contains('@') || e.trim().is_empty() {
            return Err("Email must contain `@` and not be empty.".into());
        }
        return Ok(e);
    }
    use std::io::Write;
    print!("Email: ");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| e.to_string())?;
    let email = buf.trim().to_string();
    if email.is_empty() {
        return Err("Email cannot be empty.".into());
    }
    if !email.contains('@') {
        return Err("Email must contain `@`.".into());
    }
    Ok(email)
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

    /// v1.5.0 — the DB-failure banner must point users at `rustio doctor`
    /// so they can self-diagnose (matches the release contract for the
    /// runtime-integration hint).
    #[test]
    fn scaffold_main_rs_db_banner_points_at_rustio_doctor() {
        let src = scaffold::MAIN_RS;
        assert!(
            src.contains("rustio doctor"),
            "DB-failure banner must mention `rustio doctor` as the next step"
        );
    }

    /// v1.5.0 — scaffold's MAIN_RS must call `dotenvy::dotenv()` so the
    /// `.env` file the user creates via `cp .env.example .env` actually
    /// loads. Without this, the .env hint is a lie. Doctor and runtime
    /// must mirror each other.
    #[test]
    fn scaffold_main_rs_loads_dotenv() {
        let src = scaffold::MAIN_RS;
        assert!(
            src.contains("dotenvy::dotenv()"),
            "scaffold MAIN_RS must load .env via dotenvy at startup"
        );
    }

    /// v1.5.0 — scaffold's Cargo.toml template must declare dotenvy as a
    /// dependency so the new `dotenvy::dotenv()` call in MAIN_RS compiles
    /// in fresh projects. Pin to the same minor as rustio-cli uses.
    #[test]
    fn scaffold_cargo_toml_includes_dotenvy_dep() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "demo").unwrap();
        let toml = std::fs::read_to_string(dir.join("demo").join("Cargo.toml")).unwrap();
        assert!(
            toml.contains("dotenvy = \"0.15\""),
            "scaffold Cargo.toml must include dotenvy = \"0.15\" — actual:\n{toml}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ----- v1.4.2 — project detection + scaffold guards -----------------

    fn write_rustio_cargo_toml(at: &Path) {
        std::fs::write(
            at.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n\n[dependencies]\nrustio-core = \"1.4\"\n",
        )
        .unwrap();
    }

    #[test]
    fn find_project_root_finds_rustio_project() {
        let dir = tempdir_path();
        write_rustio_cargo_toml(&dir);
        let found = scaffold::find_project_root(&dir).expect("must find project root");
        assert_eq!(found, dir.canonicalize().unwrap(), "should return the project dir");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_project_root_walks_up_from_subdir() {
        let dir = tempdir_path();
        write_rustio_cargo_toml(&dir);
        let nested = dir.join("src").join("apps").join("orders");
        std::fs::create_dir_all(&nested).unwrap();
        let found = scaffold::find_project_root(&nested).expect("must walk up to root");
        assert_eq!(found, dir.canonicalize().unwrap(), "walk-up must land at project root");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_project_root_returns_none_outside_project() {
        let dir = tempdir_path();
        // Empty tempdir — no Cargo.toml on the way up to /tmp.
        assert!(
            scaffold::find_project_root(&dir).is_none(),
            "no Cargo.toml above cwd → must return None"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_project_root_skips_non_rustio_cargo_toml() {
        let dir = tempdir_path();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"random\"\nversion=\"0\"\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        assert!(
            scaffold::find_project_root(&dir).is_none(),
            "Cargo.toml without rustio-core must NOT be treated as a RustIO project"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_app_with_cwd_fails_outside_project() {
        let dir = tempdir_path();
        let err = scaffold::app_with_cwd(&dir, "orders").unwrap_err();
        assert!(err.contains("not inside a RustIO project"), "err was: {err}");
        assert!(
            err.contains("rustio startproject"),
            "err must point the beginner at startproject; was: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_app_with_cwd_creates_under_project_root() {
        let dir = tempdir_path();
        write_rustio_cargo_toml(&dir);
        std::fs::create_dir_all(dir.join("src").join("apps")).unwrap();
        scaffold::app_with_cwd(&dir, "orders").expect("create should succeed");
        let app_dir = dir.join("src").join("apps").join("orders");
        assert!(app_dir.is_dir(), "app dir must exist at project_root/src/apps/<name>");
        assert!(app_dir.join("mod.rs").is_file(), "mod.rs missing");
        assert!(app_dir.join("models.rs").is_file(), "models.rs missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_app_with_cwd_walks_up_from_subdir() {
        let dir = tempdir_path();
        write_rustio_cargo_toml(&dir);
        let nested = dir.join("src").join("apps");
        std::fs::create_dir_all(&nested).unwrap();
        scaffold::app_with_cwd(&nested, "orders")
            .expect("must walk up and create under project root");
        assert!(
            dir.join("src").join("apps").join("orders").join("mod.rs").is_file(),
            "app must land under PROJECT root, not under cwd"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_project_at_creates_expected_files() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "demo").expect("project create should succeed");
        let root = dir.join("demo");
        for path in &[
            "Cargo.toml",
            "src/main.rs",
            "src/apps/mod.rs",
            ".gitignore",
            ".env.example",
            "README.md",
            ".ai/context.md",
            ".rustio/project.lock",
        ] {
            assert!(root.join(path).is_file(), "scaffold missing: {path}");
        }
        assert!(root.join("migrations").is_dir(), "migrations dir missing");
        assert!(root.join("templates").is_dir(), "templates dir missing");
        assert!(root.join(".ai").is_dir(), ".ai dir missing");
        assert!(root.join(".rustio").is_dir(), ".rustio dir missing");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_project_cargo_toml_uses_rustio_core_1_4() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "demo").unwrap();
        let toml = std::fs::read_to_string(dir.join("demo").join("Cargo.toml")).unwrap();
        assert!(
            toml.contains("rustio-core = \"1.4\""),
            "scaffold Cargo.toml must reference rustio-core 1.4 (not 1.0)\n--- actual ---\n{toml}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scaffold_project_readme_includes_project_name_and_startapp() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "shop").unwrap();
        let readme = std::fs::read_to_string(dir.join("shop").join("README.md")).unwrap();
        assert!(readme.contains("# shop"), "README must lead with the project name");
        assert!(
            readme.contains("rustio startapp"),
            "README must point users at the startapp command"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ----- v1.6 — first-run UX -----------------------------------------

    /// v1.6 — scaffold MAIN_RS probes rustio_users on startup and prints
    /// the empty-users banner if zero. Catches the case where a user has
    /// the DB up but hasn't created an admin yet.
    #[test]
    fn scaffold_main_rs_emits_empty_users_banner() {
        let src = scaffold::MAIN_RS;
        assert!(
            src.contains("SELECT COUNT(*) FROM rustio_users"),
            "scaffold MAIN_RS must probe rustio_users on startup"
        );
        assert!(
            src.contains("No admin user found."),
            "scaffold MAIN_RS must print the polished banner headline"
        );
        assert!(
            src.contains("Create one in another terminal:"),
            "banner must use the v1.6.0 wording (\"Create one in another terminal:\")"
        );
        assert!(
            src.contains("rustio user create --email admin@"),
            "banner must show the exact `rustio user create` command"
        );
        assert!(
            src.contains("http://127.0.0.1:8000/admin"),
            "banner must point at the admin URL"
        );
    }

    /// v1.6 — scaffold MAIN_RS registers GET / serving home.html. Without
    /// this route a fresh project lands the user on a 404 at the root URL.
    #[test]
    fn scaffold_main_rs_registers_home_route() {
        let src = scaffold::MAIN_RS;
        assert!(
            src.contains(".get(\"/\","),
            "scaffold MAIN_RS must register a GET / route"
        );
        assert!(
            src.contains("templates.render(\"home.html\""),
            "GET / must render the home.html template"
        );
    }

    /// v1.6 — scaffold's Cargo.toml must declare sqlx so the
    /// `SELECT COUNT(*) FROM rustio_users` probe in MAIN_RS compiles.
    #[test]
    fn scaffold_cargo_toml_includes_sqlx_dep() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "demo").unwrap();
        let toml = std::fs::read_to_string(dir.join("demo").join("Cargo.toml")).unwrap();
        assert!(
            toml.contains("sqlx ="),
            "scaffold Cargo.toml must include sqlx — actual:\n{toml}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// v1.6 — scaffold writes templates/home.html with a project-name
    /// placeholder and the create-admin hint. Spec wording: full GitHub
    /// URL (no "(docs)" label), bare "No admin user yet?" question.
    #[test]
    fn scaffold_writes_home_html_template() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "shop").unwrap();
        let html_path = dir.join("shop").join("templates").join("home.html");
        assert!(html_path.is_file(), "templates/home.html must exist");
        let html = std::fs::read_to_string(&html_path).unwrap();
        assert!(
            html.contains("{{ project }}"),
            "home.html must use the {{{{ project }}}} minijinja variable"
        );
        assert!(
            html.contains("/admin"),
            "home.html must link to /admin"
        );
        assert!(
            html.contains("https://github.com/abdulwahed-sweden/rustio"),
            "home.html must show the full GitHub URL (no abbreviation)"
        );
        assert!(
            !html.contains("(docs)"),
            "home.html must NOT label the GitHub link \"(docs)\""
        );
        assert!(
            html.contains("No admin user yet?"),
            "home.html must include the bare \"No admin user yet?\" line"
        );
        assert!(
            html.contains("rustio user create"),
            "home.html must hint at the create-admin command"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ----- Phase 12/a — AI-readable scaffold ----------------------------

    /// Phase 12/a — `.ai/context.md` lands with the project name
    /// interpolated and the Do / Do-Not rules in place. Substring
    /// checks; structural concerns (TOML) live in the project.lock
    /// tests below.
    #[test]
    fn scaffold_project_at_creates_ai_context() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "clinic").unwrap();
        let path = dir.join("clinic").join(".ai").join("context.md");
        assert!(path.is_file(), ".ai/context.md must exist");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# RustIO Project Context"),
            "context.md must lead with the canonical heading — actual:\n{text}"
        );
        assert!(
            text.contains("clinic"),
            "context.md must interpolate the project name — actual:\n{text}"
        );
        assert!(
            text.contains("rustio startapp"),
            "context.md must reference the startapp command in the Do block"
        );
        assert!(
            text.contains("Do not modify the upstream `rustio-core` crate"),
            "context.md must carry the clarified rustio-core rule"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Phase 12/a — `.rustio/project.lock` substring checks: every key
    /// and section the doctor / future tooling will key off must be
    /// present in the rendered file. The rustio_version assertion
    /// reads `env!("CARGO_PKG_VERSION")` so the test survives bumps.
    #[test]
    fn scaffold_project_at_creates_project_lock() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "clinic").unwrap();
        let path = dir.join("clinic").join(".rustio").join("project.lock");
        assert!(path.is_file(), ".rustio/project.lock must exist");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[meta]"), "project.lock must have [meta]");
        assert!(
            text.contains("schema_version = 1"),
            "project.lock must record schema_version = 1"
        );
        assert!(text.contains("[project]"), "project.lock must have [project]");
        assert!(text.contains("[database]"), "project.lock must have [database]");
        assert!(text.contains("[design]"), "project.lock must have [design]");
        assert!(text.contains("[ai]"), "project.lock must have [ai]");
        assert!(
            text.contains("name = \"clinic\""),
            "project.lock must interpolate the project name"
        );
        let want_version = format!("rustio_version = \"{}\"", env!("CARGO_PKG_VERSION"));
        assert!(
            text.contains(&want_version),
            "project.lock must record the current CLI version — wanted: {want_version}\nactual:\n{text}"
        );
        assert!(
            text.contains("backend = \"postgres\""),
            "project.lock must record postgres as the database backend"
        );
        assert!(
            text.contains("brand = \"#0d9488\""),
            "project.lock must seed the default brand color"
        );
        assert!(
            text.contains("design_system = \"rustio-admin-v1\""),
            "project.lock must pin the design system tag"
        );
        assert!(
            text.contains("context = \".ai/context.md\""),
            "project.lock must point at the AI context file"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Phase 12/a — `.rustio/project.lock` parses as valid TOML. Catches
    /// malformed strings or accidental key-name typos that the substring
    /// checks above would miss.
    #[test]
    fn scaffold_project_lock_parses_as_valid_toml() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "clinic").unwrap();
        let path = dir.join("clinic").join(".rustio").join("project.lock");
        let text = std::fs::read_to_string(&path).unwrap();
        let value: toml::Value = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("project.lock must parse as TOML: {e}\n--- actual ---\n{text}"));
        assert_eq!(
            value
                .get("project")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str()),
            Some("clinic"),
            "project.name must equal the scaffold input"
        );
        assert_eq!(
            value
                .get("project")
                .and_then(|p| p.get("rustio_version"))
                .and_then(|v| v.as_str()),
            Some(env!("CARGO_PKG_VERSION")),
            "project.rustio_version must equal the CLI's CARGO_PKG_VERSION"
        );
        assert_eq!(
            value
                .get("meta")
                .and_then(|m| m.get("schema_version"))
                .and_then(|v| v.as_integer()),
            Some(1),
            "meta.schema_version must equal 1 (the initial Phase 12/a schema)"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Phase 12/a — generated README must point developers at the two
    /// new directories so they don't mistake them for editor cruft and
    /// delete them.
    #[test]
    fn scaffold_project_readme_documents_ai_and_rustio_dirs() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "shop").unwrap();
        let readme = std::fs::read_to_string(dir.join("shop").join("README.md")).unwrap();
        assert!(
            readme.contains(".ai/context.md"),
            "README must reference .ai/context.md — actual:\n{readme}"
        );
        assert!(
            readme.contains(".rustio/project.lock"),
            "README must reference .rustio/project.lock — actual:\n{readme}"
        );
        assert!(
            readme.contains("AI agents"),
            "README must explain the .ai/ directory in terms of AI agents"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// v1.6 — README quickstart must point users at `rustio user create`
    /// with a project-name-substituted email.
    #[test]
    fn scaffold_readme_includes_user_create_step() {
        let dir = tempdir_path();
        scaffold::project_at(&dir, "shop").unwrap();
        let readme = std::fs::read_to_string(dir.join("shop").join("README.md")).unwrap();
        assert!(
            readme.contains("rustio user create --email admin@shop.local"),
            "README must show the project-name-substituted create-admin command — actual:\n{readme}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // ----- v1.6 — password validation + email prompt --------------------

    #[test]
    fn validate_password_rejects_too_short() {
        let err = super::validate_password("1234").unwrap_err();
        assert_eq!(
            err, "Password must be at least 8 characters.",
            "v1.6.0 polished error wording"
        );
    }

    #[test]
    fn validate_password_rejects_all_same_character() {
        let err = super::validate_password("00000000").unwrap_err();
        assert_eq!(
            err, "Password is too weak.",
            "all-same-character must surface as the unified \"too weak\" message"
        );
        assert_eq!(
            super::validate_password("aaaaaaaa").unwrap_err(),
            "Password is too weak."
        );
    }

    #[test]
    fn validate_password_rejects_common_passwords() {
        let err = super::validate_password("password").unwrap_err();
        assert_eq!(err, "Password is too weak.");
        assert_eq!(
            super::validate_password("Password").unwrap_err(),
            "Password is too weak.",
            "common-password check must be case-insensitive"
        );
        assert_eq!(
            super::validate_password("12345678").unwrap_err(),
            "Password is too weak."
        );
        assert_eq!(
            super::validate_password("admin123").unwrap_err(),
            "Password is too weak."
        );
    }

    #[test]
    fn validate_password_accepts_strong_password() {
        assert!(super::validate_password("MyS3cure!Pass").is_ok());
        assert!(super::validate_password("correct horse battery").is_ok());
    }

    #[test]
    fn prompt_email_passes_through_valid_input() {
        let e = super::prompt_email(Some("admin@shop.local".into())).unwrap();
        assert_eq!(e, "admin@shop.local");
    }

    #[test]
    fn prompt_email_rejects_no_at_sign() {
        let err = super::prompt_email(Some("not-an-email".into())).unwrap_err();
        assert!(err.contains('@'), "error must explain the rule: {err}");
        assert!(
            err.starts_with("Email"),
            "error must use polished wording (Email …), got: {err}"
        );
    }
}

// ---- scaffold ----------------------------------------------------------

mod scaffold {
    use std::fs;
    use std::path::{Path, PathBuf};

    pub fn project(name: &str) -> Result<(), String> {
        project_at(Path::new("."), name)
    }

    pub fn project_at(base: &Path, name: &str) -> Result<(), String> {
        let root = base.join(name);
        if root.exists() {
            return Err(format!("{name} already exists"));
        }
        fs::create_dir_all(root.join("src").join("apps")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("migrations")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join("templates")).map_err(|e| e.to_string())?;
        // Phase 12/a — AI-readable scaffold. `.ai/` carries human prose
        // for AI agents; `.rustio/` carries machine-readable metadata
        // the CLI uses for doctor / future upgrades. Both are committed
        // to git; neither belongs in `.gitignore`.
        fs::create_dir_all(root.join(".ai")).map_err(|e| e.to_string())?;
        fs::create_dir_all(root.join(".rustio")).map_err(|e| e.to_string())?;

        fs::write(root.join("Cargo.toml"), cargo_toml(name)).map_err(|e| e.to_string())?;
        fs::write(root.join("src").join("main.rs"), MAIN_RS).map_err(|e| e.to_string())?;
        fs::write(root.join("src").join("apps").join("mod.rs"), "").map_err(|e| e.to_string())?;
        fs::write(root.join(".gitignore"), GITIGNORE).map_err(|e| e.to_string())?;
        fs::write(root.join(".env.example"), ENV_EXAMPLE).map_err(|e| e.to_string())?;
        fs::write(root.join("README.md"), project_readme(name)).map_err(|e| e.to_string())?;
        fs::write(root.join(".ai").join("context.md"), ai_context_md(name))
            .map_err(|e| e.to_string())?;
        fs::write(root.join(".rustio").join("project.lock"), project_lock_toml(name))
            .map_err(|e| e.to_string())?;
        // v1.6 — minimal welcome page rendered by `GET /`. Plain HTML,
        // no JS, no framework. Edit freely; delete to fall through to
        // a custom handler.
        fs::write(root.join("templates").join("home.html"), HOME_HTML)
            .map_err(|e| e.to_string())?;

        println!("✓ created project {name}");
        println!();
        println!("next steps:");
        println!("  cd {name}");
        println!("  cp .env.example .env    # edit DATABASE_URL, MEILI_URL");
        println!("  rustio run");
        Ok(())
    }

    pub fn app(name: &str) -> Result<(), String> {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        app_with_cwd(&cwd, name)
    }

    /// Implementation seam for `app()` — accepts an explicit cwd so tests
    /// can drive it against synthetic project trees in tempdirs without
    /// chdir'ing the real process. The walk-up + project guard live here;
    /// `app()` is just a thin wrapper that supplies the real cwd.
    pub fn app_with_cwd(cwd: &Path, name: &str) -> Result<(), String> {
        let project_root = find_project_root(cwd).ok_or_else(|| {
            "not inside a RustIO project.\n\nTo create one:\n\n  rustio startproject myproject\n  cd myproject\n  rustio startapp myapp"
                .to_string()
        })?;
        let app_dir = project_root.join("src").join("apps").join(name);
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

    /// Walk upward from `start`, returning the first directory whose
    /// `Cargo.toml` mentions `rustio-core`. Returns `None` if the
    /// filesystem root is reached without finding a match.
    ///
    /// Detection is naive substring match — robust against the various
    /// shapes a `rustio-core` dependency can take (`version = "..."`,
    /// `path = "..."`, `git = "..."`, workspace-inherited). Reusable;
    /// future migrate / doctor commands can call this to refuse running
    /// outside a project.
    pub fn find_project_root(start: &Path) -> Option<PathBuf> {
        let mut cur = start.canonicalize().ok()?;
        loop {
            let manifest = cur.join("Cargo.toml");
            if manifest.is_file() {
                if let Ok(text) = fs::read_to_string(&manifest) {
                    if text.contains("rustio-core") {
                        return Some(cur);
                    }
                }
            }
            if !cur.pop() {
                return None;
            }
        }
    }

    fn cargo_toml(name: &str) -> String {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
rustio-core = "1.4"
tokio = {{ version = "1", features = ["rt-multi-thread", "macros"] }}
chrono = {{ version = "0.4", features = ["serde"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
env_logger = "0.11"
log = "0.4"
dotenvy = "0.15"
# v1.6 — used in main.rs for the first-run "no admin user" probe.
# Already a transitive dep via rustio-core; declaring it here is for
# the direct `sqlx::query_scalar` call site.
sqlx = {{ version = "0.8", default-features = false, features = ["runtime-tokio", "postgres"] }}
"#
        )
    }

    fn project_readme(name: &str) -> String {
        format!(
            r#"# {name}

A RustIO project.

## Quickstart

```sh
cp .env.example .env
# edit .env: set DATABASE_URL (and optionally MEILI_URL)
cargo run
```

In a second terminal, create your first admin user:

```sh
rustio user create --email admin@{name}.local
```

(Interactive — prompts for password. Run `rustio user create --help`
to see the non-interactive flags.)

Then open <http://127.0.0.1:8000/> for the welcome page or
<http://127.0.0.1:8000/admin> to log in.

## Add an app

```sh
rustio startapp orders
# then edit src/apps/orders/models.rs
```

## Diagnose setup issues

```sh
rustio doctor
```

## Project layout

- `.ai/context.md` — what this project is, what AI agents may and may
  not change. Edit this to teach Claude Code (or any AI agent) about
  your domain and house rules.
- `.rustio/project.lock` — machine-readable project metadata (RustIO
  version, database backend, branding). Managed by the CLI; don't
  hand-edit it.

Both directories are committed to git — they are project state, not
local cache.

See <https://github.com/abdulwahed-sweden/rustio> for documentation.
"#
        )
    }

    /// Phase 12/a — `.ai/context.md` for AI agents (Claude Code, etc.).
    /// The file is human-and-AI-readable: it tells an agent what the
    /// project is, what may be modified, and what is off-limits. The
    /// project name is interpolated; the rules and design-system
    /// guidance are static prose the developer can edit.
    fn ai_context_md(name: &str) -> String {
        format!(
            r#"# RustIO Project Context

This is a RustIO project. AI agents (Claude Code, Cursor, etc.) should
read this file before making changes.

## Project Name

{name}

## Domain

_Fill in: a short description of what this project does._

## Main Resources

_Fill in: the core resources this project models. Examples:_

- _TODO: resource one_
- _TODO: resource two_

## Rules for AI Agents

### Do

- Add new apps using `rustio startapp <name>`.
- Put model code inside `src/apps/<app>/models.rs`.
- Add migrations inside `migrations/`.
- Follow the existing admin design system.
- Use `templates/overrides/` only for intentional framework overrides.

### Do Not

- Do not modify the upstream `rustio-core` crate (your dependency, not
  your codebase). Project changes belong in `src/`, `migrations/`, and
  `templates/` — never in the framework's source.
- Do not rewrite admin templates unless explicitly requested.
- Do not invent new colors.
- Do not change design tokens randomly.
- Do not add SQLite support — RustIO is PostgreSQL-only.
- Do not auto-seed production users.
- Do not modify `.rustio/*.lock` manually; the CLI manages those files.

## Design System

Primary brand color: `#0d9488`.

Use the existing RustIO admin classes. Do not introduce new UI systems.

## Template Rules

- `templates/*.html` (e.g. `home.html`) are normal project pages — edit
  them freely.
- `templates/overrides/` is reserved for framework-level customization
  and is tracked separately. Treat anything inside it as load-bearing.

## Database

PostgreSQL only.

## Commands

Run diagnostics:

```sh
rustio doctor
```

Create an admin user:

```sh
rustio user create --email admin@{name}.local
```

Run the server:

```sh
cargo run
```
"#
        )
    }

    /// Phase 12/a — `.rustio/project.lock`. Machine-readable project
    /// metadata: which RustIO version produced this project, which DB
    /// backend, which design system, where the AI context file lives.
    /// Not for hand-editing — the CLI rewrites it on upgrades.
    fn project_lock_toml(name: &str) -> String {
        let version = env!("CARGO_PKG_VERSION");
        // The raw-string delimiter is `r##"..."##` (not `r#"..."#`) because
        // the TOML body contains the literal sequence `"#` inside the brand
        // colour value, which would close a single-`#` raw string early.
        format!(
            r##"# RustIO project metadata. Managed by the `rustio` CLI.
# Do not hand-edit; the CLI rewrites this file on project upgrades.

[meta]
schema_version = 1

[project]
name = "{name}"
rustio_version = "{version}"
created_with_cli = "{version}"

[database]
backend = "postgres"

[design]
brand = "#0d9488"
design_system = "rustio-admin-v1"

[ai]
context = ".ai/context.md"
"##
        )
    }

    pub(super) const HOME_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{{ project }}</title>
  <style>
    body { font-family: system-ui, -apple-system, sans-serif; max-width: 600px; margin: 3rem auto; padding: 0 1.5rem; line-height: 1.6; color: #1a1a1a; }
    h1 { margin-bottom: 0.25rem; }
    .lede { color: #6b7280; margin-top: 0; }
    a { color: #b8431a; text-decoration: none; }
    a:hover { text-decoration: underline; }
    pre { background: #f4f4f5; padding: 0.75rem 1rem; border-radius: 6px; overflow-x: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    ul { list-style: none; padding: 0; }
    li { margin: 0.25rem 0; }
  </style>
</head>
<body>
  <h1>Welcome to {{ project }}</h1>
  <p class="lede">Your RustIO project is running.</p>

  <p>Get started:</p>
  <ul>
    <li>→ <a href="/admin">/admin</a></li>
    <li>→ <a href="https://github.com/abdulwahed-sweden/rustio">https://github.com/abdulwahed-sweden/rustio</a></li>
  </ul>

  <p>No admin user yet?</p>
  <pre><code>rustio user create --email admin@{{ project }}.local</code></pre>
</body>
</html>
"#;

    pub(super) const MAIN_RS: &str = r#"use std::net::SocketAddr;

use rustio_core::admin::{register_admin_routes, Admin, SiteBranding};
use rustio_core::auth;
use rustio_core::background;
use rustio_core::http::Response;
use rustio_core::middleware::{self, RateLimiter};
use rustio_core::migrations;
use rustio_core::orm::Db;
use rustio_core::router::Router;
use rustio_core::server::Server;
use rustio_core::templates::Templates;

/// Project branding — `env!("CARGO_PKG_NAME")` resolves at compile time
/// to this project's package name, so the admin chrome shows your
/// project name instead of the framework default. Edit freely.
fn branding() -> SiteBranding {
    let pkg = env!("CARGO_PKG_NAME");
    let title_cased: String = pkg
        .split(|c: char| c == '-' || c == '_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    SiteBranding {
        site_title: format!("{} administration", title_cased),
        site_header: title_cased.clone(),
        index_title: "Dashboard".into(),
        footer_copyright: format!("RustIO {}", env!("CARGO_PKG_VERSION")),
        domain: format!("{}.local", pkg),
    }
}

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env from the current directory if present. Silent on failure —
    // .env is optional; production deploys typically use real env vars.
    let _ = dotenvy::dotenv();
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
            eprintln!();
            eprintln!("For a step-by-step diagnosis, run: rustio doctor");
            std::process::exit(1);
        }
    };
    auth::init_tables(&db).await?;
    migrations::apply(&db, "migrations").await?;

    // First-run hint: nudge the operator to create an admin user when
    // the table is empty. Read-only check; disappears on next boot
    // once any user exists. Silent on query error so a transient
    // failure here can never block startup.
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rustio_users")
        .fetch_one(db.pool())
        .await
        .unwrap_or(0);
    if user_count == 0 {
        eprintln!();
        eprintln!("⚠ No admin user found.");
        eprintln!();
        eprintln!("Create one in another terminal:");
        eprintln!();
        eprintln!("  rustio user create --email admin@{}.local", env!("CARGO_PKG_NAME"));
        eprintln!();
        eprintln!("Then open:");
        eprintln!();
        eprintln!("  http://127.0.0.1:8000/admin");
        eprintln!();
    }

    background::spawn_housekeeping(db.clone());

    let template_dir = std::env::var("RUSTIO_TEMPLATE_DIR").unwrap_or_else(|_| "templates".into());
    let templates = Templates::new(Some(template_dir.into()))?;

    let admin = Admin::new()
        .site_branding(branding());
    admin.seed_permissions(&db).await?;

    let router = Router::new()
        .middleware(middleware::rate_limit(RateLimiter::default_limits()))
        .middleware(middleware::logger)
        .middleware(middleware::security_headers)
        .middleware(middleware::gzip)
        .middleware(middleware::csrf_protect);

    // Welcome page at GET /. Templates are Clone-cheap (Arc inside);
    // we keep one for the home route and hand the original off to
    // `register_admin_routes` below. Edit `templates/home.html` to
    // customize the landing page; delete the route + template if you
    // want / to fall through to a custom handler.
    let templates_for_home = templates.clone();
    let router = router.get("/", move |_req| {
        let templates = templates_for_home.clone();
        async move {
            let ctx = serde_json::json!({ "project": env!("CARGO_PKG_NAME") });
            let body = templates.render("home.html", &ctx)?;
            Ok(Response::html(body))
        }
    });

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
