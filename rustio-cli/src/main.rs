use std::fs;
use std::path::Path;
use std::process::{Command as ProcessCommand, ExitCode};

mod wizard;

const USAGE: &str = r#"rustio — the RustIO framework CLI

USAGE:
    rustio <COMMAND>

COMMANDS:
    init [name]               Start the interactive wizard, or scaffold directly with a name
    new project <name>        Create a new RustIO project
    new app <name>            Create a new app inside the current project
    run                       Build and run the project in the current directory
    migrate generate <name>   Create a new migration file
    migrate apply [-v]        Apply all pending migrations (verbose with -v / --verbose)
    migrate status            Show applied and pending migrations
    schema                    Write rustio.schema.json at the project root
    ai                        (0.5.0) Show the AI boundary summary
    ai plan "<prompt>"        (0.5.0) Plan a schema change (read-only — never executes)
    user create [opts]        Create a user in the auth tables (interactive if args omitted)

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
        Ok(Command::Init { name, preset, app }) => init_command(name, preset, app),
        Ok(Command::NewProject(name)) => new_project(&name),
        Ok(Command::NewApp(name)) => new_app(&name),
        Ok(Command::Run) => run(),
        Ok(Command::MigrateGenerate(name)) => migrate_generate(&name),
        Ok(Command::MigrateApply { verbose }) => migrate_apply(verbose).await,
        Ok(Command::MigrateStatus) => migrate_status().await,
        Ok(Command::Schema) => schema_command(),
        Ok(Command::Ai(sub)) => ai_command(sub),
        Ok(Command::UserCreate {
            email,
            password,
            role,
        }) => user_create_command(email, password, role).await,
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
    /// `rustio init` — interactive wizard when no name is provided,
    /// non-interactive scaffold when a name is given.
    Init {
        name: Option<String>,
        preset: Option<wizard::Preset>,
        app: Option<String>,
    },
    NewProject(String),
    NewApp(String),
    Run,
    MigrateGenerate(String),
    MigrateApply {
        verbose: bool,
    },
    MigrateStatus,
    /// Emit `rustio.schema.json` at the project root by running the
    /// built binary with `--dump-schema`.
    Schema,
    /// `rustio ai …`. Dispatches to the AI planner or (with no
    /// argument) prints a summary of the AI boundary.
    Ai(AiCommand),
    /// `rustio user create` — seeds a user in the auth tables so
    /// someone can actually sign in to `/admin`.
    UserCreate {
        email: Option<String>,
        password: Option<String>,
        role: Option<String>,
    },
    Version,
    Help,
}

/// `rustio ai …` subcommands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AiCommand {
    /// `rustio ai` — informational summary of the AI boundary. No edits.
    Overview(Option<String>),
    /// `rustio ai plan "<prompt>"` — run the 0.5.0 planner. Reads the
    /// schema + optional context, prints a structured plan. Never
    /// writes files, never executes.
    Plan(String),
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
        Some("init") => parse_init_args(&args[2..]),
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
        Some("schema") => {
            if args.len() > 2 {
                return Err(format!("unexpected argument `{}`", args[2]));
            }
            Ok(Command::Schema)
        }
        Some("ai") => parse_ai_command(&args[2..]),
        Some("user") => match args.get(2).map(String::as_str) {
            Some("create") => parse_user_create_args(&args[3..]),
            Some(other) => Err(format!("unknown subcommand `user {other}`")),
            None => Err("usage: rustio user create [--email E] [--password P] [--role R]".into()),
        },
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

/// Parse arguments to `rustio init`. Accepts a positional project name
/// and the flags:
///
/// - `--preset <basic|blog|api>` — starter preset.
/// - `--app <name>` — override the first app's name (overrides the
///   preset default). Ignored under `--preset basic`.
/// - `--db <kind>` — reserved for future drivers; today only SQLite is
///   supported and the value is ignored.
fn parse_init_args(rest: &[String]) -> Result<Command, String> {
    let mut name: Option<String> = None;
    let mut preset: Option<wizard::Preset> = None;
    let mut app: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--preset" => {
                let v = rest
                    .get(i + 1)
                    .ok_or("missing value for --preset (expected basic, blog, or api)")?;
                preset = Some(v.parse::<wizard::Preset>()?);
                i += 2;
            }
            "--app" => {
                let v = rest
                    .get(i + 1)
                    .ok_or("missing value for --app (expected a name like `books`)")?;
                app = Some(v.clone());
                i += 2;
            }
            "--db" => {
                // Reserved. SQLite is the only driver today; accept any value
                // so scripts that already specify it don't break.
                if rest.get(i + 1).is_none() {
                    return Err("missing value for --db".into());
                }
                i += 2;
            }
            other if !other.starts_with('-') && name.is_none() => {
                name = Some(other.to_string());
                i += 1;
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok(Command::Init { name, preset, app })
}

fn init_command(
    name: Option<String>,
    preset: Option<wizard::Preset>,
    app: Option<String>,
) -> Result<(), String> {
    // If a name is provided, we're in non-interactive mode. Otherwise launch
    // the wizard. The wizard will fail fast with a clear message when stdin
    // is not a terminal (e.g. piped input, CI) — the correct fix there is to
    // pass the arguments explicitly.
    let plan = match name {
        Some(n) => wizard::Plan {
            project_name: n,
            preset: preset.unwrap_or(wizard::Preset::Basic),
            app_name: app,
        },
        None => wizard::run(preset, app)?,
    };
    wizard::execute(&plan)
}

pub(crate) fn new_project(name: &str) -> Result<(), String> {
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

pub(crate) fn new_app(name: &str) -> Result<(), String> {
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

    let struct_name = singular_capitalize(name);
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
        render(
            APP_VIEWS_RS,
            &[
                ("NAME", name),
                ("STRUCT", &struct_name),
                ("TABLE", &table_name),
            ],
        ),
    )
    .map_err(err_str)?;

    register_app_in_mod(name)?;

    let migrations_dir = Path::new("migrations");
    let create_sql = format!(
        "CREATE TABLE {table} (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0
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

    // First compile pulls in sqlx + hyper + tokio from scratch and takes
    // 30–60s on a clean machine. Warn the user so they don't suspect
    // `rustio run` has hung. Subsequent runs reuse `target/` and are
    // effectively instant.
    if !Path::new("target").exists() {
        eprintln!("rustio: first run compiles dependencies (~1 min). Subsequent runs are instant.");
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
        return Ok(());
    }

    for f in &applied {
        println!("  {} applied {f}", out::check());
    }
    let n = applied.len();
    let noun = if n == 1 { "migration" } else { "migrations" };
    println!();
    out::success(&format!("Applied {n}"), noun);

    // Auto-dump the schema so rustio.schema.json stays in sync with the
    // persisted shape. Best-effort: if the project doesn't compile (or
    // doesn't have a --dump-schema handler — true for 0.3.x-era layouts),
    // we print a hint and let the user regenerate explicitly. Migration
    // success is not gated on this.
    println!();
    out::plain("Regenerating rustio.schema.json …");
    if let Err(msg) = try_dump_schema() {
        out::info("  skipped (run `rustio schema` once your project compiles)");
        if verbose {
            eprintln!("  reason: {msg}");
        }
    }
    Ok(())
}

/// Shell out to `cargo run -- --dump-schema`. Returns an error if the
/// user's project doesn't compile or its `main.rs` is pre-0.4.0 and
/// doesn't handle the flag. Callers may treat the error as a hint, not
/// a hard failure — persisted schema changes stay applied regardless.
fn try_dump_schema() -> Result<(), String> {
    let status = ProcessCommand::new("cargo")
        .args(["run", "--quiet", "--", "--dump-schema"])
        .status()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo run --dump-schema exited with {}",
            status.code().unwrap_or(-1)
        ));
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

/// `rustio schema` — compile + run the project with `--dump-schema`.
/// The generated `main.rs` watches for that flag, invokes
/// `rustio_core::Schema::from_admin`, and writes `rustio.schema.json`
/// before returning. This CLI command is a thin driver over that.
fn schema_command() -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "no Cargo.toml in current directory — run this from inside a RustIO project".into(),
        );
    }
    try_dump_schema()
}

/// Parse `--email X --password Y --role R` in any order. All three are
/// optional at the CLI level; the `user_create_command` falls back to
/// interactive prompts for anything that's missing.
fn parse_user_create_args(rest: &[String]) -> Result<Command, String> {
    let mut email: Option<String> = None;
    let mut password: Option<String> = None;
    let mut role: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--email" => {
                email = Some(
                    rest.get(i + 1)
                        .ok_or("missing value for --email")?
                        .to_string(),
                );
                i += 2;
            }
            "--password" => {
                password = Some(
                    rest.get(i + 1)
                        .ok_or("missing value for --password")?
                        .to_string(),
                );
                i += 2;
            }
            "--role" => {
                role = Some(
                    rest.get(i + 1)
                        .ok_or("missing value for --role (admin or user)")?
                        .to_string(),
                );
                i += 2;
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }
    Ok(Command::UserCreate {
        email,
        password,
        role,
    })
}

/// `rustio user create` — interactively (or non-interactively) create
/// a user in the auth tables. Required because a fresh project has no
/// users and otherwise nobody can sign in to `/admin`.
///
/// The command runs against `RUSTIO_DATABASE_URL` (default
/// `sqlite://app.db?mode=rwc`). If the DB doesn't have `rustio_users`
/// yet, we call `ensure_core_tables` up front so the command works
/// even before the first `rustio migrate apply`.
async fn user_create_command(
    email: Option<String>,
    password: Option<String>,
    role: Option<String>,
) -> Result<(), String> {
    let email = match email {
        Some(e) => e,
        None => inquire::Text::new("Email:")
            .prompt()
            .map_err(|e| format!("prompt cancelled: {e}"))?,
    };

    let password = match password {
        Some(p) => p,
        None => inquire::Password::new("Password:")
            .with_display_mode(inquire::PasswordDisplayMode::Masked)
            .with_custom_confirmation_message("Confirm password:")
            .with_custom_confirmation_error_message("Passwords don't match.")
            .prompt()
            .map_err(|e| format!("prompt cancelled: {e}"))?,
    };

    let role = match role {
        Some(r) => r,
        None => inquire::Select::new("Role:", vec!["admin", "user"])
            .prompt()
            .map_err(|e| format!("prompt cancelled: {e}"))?
            .to_string(),
    };

    let db = rustio_core::Db::connect(&database_url())
        .await
        .map_err(err_str)?;
    rustio_core::auth::ensure_core_tables(&db)
        .await
        .map_err(err_str)?;

    let user = rustio_core::auth::user::create(&db, &email, &password, &role)
        .await
        .map_err(err_str)?;

    out::success(
        "Created user",
        &format!("{} (role={}, id={})", user.email, user.role, user.id),
    );
    Ok(())
}

/// Parse the args after `rustio ai` into an [`AiCommand`]. Keeps
/// command-string parsing out of `parse_command` so the `ai` subtree
/// can grow independently.
fn parse_ai_command(rest: &[String]) -> Result<Command, String> {
    match rest.first().map(String::as_str) {
        Some("plan") => {
            let prompt = rest[1..].join(" ");
            if prompt.trim().is_empty() {
                return Err("usage: rustio ai plan \"<natural language request>\"".to_string());
            }
            Ok(Command::Ai(AiCommand::Plan(prompt)))
        }
        Some(other) if !other.starts_with("--") => {
            // Back-compat: `rustio ai add foo` (pre-plan syntax) still
            // reaches the informational overview with an "intent"
            // summary so existing muscle memory doesn't break.
            Ok(Command::Ai(AiCommand::Overview(Some(rest.join(" ")))))
        }
        Some(flag) => Err(format!(
            "unknown flag `{flag}` (try `rustio ai plan \"…\"`)"
        )),
        None => Ok(Command::Ai(AiCommand::Overview(None))),
    }
}

fn ai_command(sub: AiCommand) -> Result<(), String> {
    match sub {
        AiCommand::Overview(intent) => ai_overview(intent),
        AiCommand::Plan(prompt) => ai_plan_command(prompt),
    }
}

/// `rustio ai` (no args) — informational summary. No project I/O.
fn ai_overview(intent: Option<String>) -> Result<(), String> {
    out::info("rustio ai — the 0.5.0 AI planning layer.");
    println!();
    if let Some(msg) = intent {
        out::plain(&format!("intent recorded: {msg}"));
        out::plain("(not executed — the AI executor is scheduled for 0.5.x)");
        println!();
    }
    out::plain("The AI planner reads rustio.schema.json and emits a structured");
    out::plain("Plan composed of these primitives:");
    out::plain("  add_model · remove_model · rename_model");
    out::plain("  add_field · remove_field · rename_field");
    out::plain("  change_field_type · change_field_nullability");
    out::plain("  add_relation · remove_relation · update_admin");
    out::plain("Anything that can't be expressed as a primitive is rejected.");
    println!();
    out::hint("rustio ai plan \"Add priority to tasks\"   # try the planner");
    out::hint("rustio schema                              # emit rustio.schema.json");
    Ok(())
}

/// `rustio ai plan "<prompt>"` — the 0.5.0 planning layer.
///
/// Reads (schema, optional context, prompt), produces a validated
/// [`rustio_core::ai::Plan`] + explanation, and prints both a strict
/// JSON object and a human-readable summary. **Does not execute.**
/// Does not touch the filesystem beyond reading the schema/context.
fn ai_plan_command(prompt: String) -> Result<(), String> {
    use rustio_core::ai::generate_plan;
    use rustio_core::ai::planner::{
        render_plan_human, render_plan_json, ContextConfig, PlanRequest,
    };
    use rustio_core::Schema;

    // Schema is required — the planner validates against it.
    let schema_path = Path::new("rustio.schema.json");
    if !schema_path.exists() {
        return Err(
            "rustio.schema.json not found. Run `rustio schema` first to emit it.".to_string(),
        );
    }
    let schema_json = fs::read_to_string(schema_path).map_err(err_str)?;
    let schema: Schema = Schema::parse(&schema_json).map_err(err_str)?;

    // Context is optional — read if present, otherwise plan without it.
    let ctx_path = Path::new("rustio.context.json");
    let context: Option<ContextConfig> = if ctx_path.exists() {
        let raw = fs::read_to_string(ctx_path).map_err(err_str)?;
        Some(ContextConfig::parse(&raw).map_err(|e| e.to_string())?)
    } else {
        None
    };

    let result = match generate_plan(&schema, context.as_ref(), PlanRequest::new(&prompt)) {
        Ok(r) => r,
        Err(e) => {
            // JSON skeleton on stdout so callers piping into `jq`
            // don't crash on empty stdin; friendly error goes to
            // stderr via the caller's Err path.
            let body = serde_json::json!({
                "plan": [],
                "explanation": format!("refused: {e}"),
                "error_kind": error_kind(&e),
            });
            println!("{}", serde_json::to_string_pretty(&body).unwrap());
            return Err(format!("planner refused: {e}"));
        }
    };

    // 1. Strict JSON shape documented for the 0.5.0 planner.
    println!("{}", render_plan_json(&result.plan, &result.explanation));
    // 2. Human-readable block — goes after, separated by a blank line.
    println!();
    print!("{}", render_plan_human(&result.plan, &result.explanation));
    Ok(())
}

/// Short kind label for a `PlanError`. Lets JSON consumers branch on
/// error category without parsing the `Display` string.
fn error_kind(e: &rustio_core::ai::PlanError) -> &'static str {
    use rustio_core::ai::PlanError as E;
    match e {
        E::EmptyPrompt => "empty_prompt",
        E::InvalidIntent(_) => "invalid_intent",
        E::UnknownModel { .. } => "unknown_model",
        E::AmbiguousModel { .. } => "ambiguous_model",
        E::FieldAlreadyExists { .. } => "field_already_exists",
        E::FieldDoesNotExist { .. } => "field_does_not_exist",
        E::DeveloperOnlyRequested(_) => "developer_only",
        E::CoreModelProtected(_) => "core_model_protected",
        E::UnknownType(_) => "unknown_type",
        E::Validation(_) => "validation",
        E::ContextParse(_) => "context_parse",
        // `PlanError` is `#[non_exhaustive]`; a new variant should surface
        // as a generic tag rather than block the CLI from printing.
        _ => "unknown",
    }
}

fn register_app_in_mod(name: &str) -> Result<(), String> {
    let path = Path::new("apps/mod.rs");
    let current = fs::read_to_string(path).map_err(err_str)?;

    let module_line = format!("pub mod {name};\n");
    let admin_install = format!("    admin = {name}::admin::install(admin);\n");
    let view_register = format!("    router = {name}::views::register(router);\n");

    let updated = current
        .replacen(
            "// -- end modules --",
            &format!("{module_line}// -- end modules --"),
            1,
        )
        .replacen(
            "    // -- end admin installs --",
            &format!("{admin_install}    // -- end admin installs --"),
            1,
        )
        .replacen(
            "    // -- end view registrations --",
            &format!("{view_register}    // -- end view registrations --"),
            1,
        );

    if updated == current {
        return Err(
            "apps/mod.rs is missing the expected marker comments — restore them or recreate the file from `rustio new project`"
                .into(),
        );
    }

    fs::write(path, updated).map_err(err_str)?;
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
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

fn singular_capitalize(name: &str) -> String {
    // If the scaffolded name is plural (ends with `s`), strip the `s` so the
    // generated Rust struct is singular. Safe for the common cases; users can
    // rename for edge cases like "news" / "status".
    let base = name.strip_suffix('s').unwrap_or(name);
    let base = if base.is_empty() { name } else { base };
    capitalize(base)
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
//
// Resolution order:
//   1. `RUSTIO_CORE_PATH` env var (explicit override) — path dep.
//   2. A sibling `rustio-core` directory next to the CLI's workspace —
//      auto-detected when running via `cargo run -p rustio-cli` from a
//      checkout. This keeps scaffolded projects in sync with the in-tree
//      code during development, so features merged into `rustio-core`
//      but not yet published to crates.io are available immediately.
//   3. Fall back to the CLI's package version (crates.io).
fn rustio_core_dep() -> String {
    if let Ok(path) = std::env::var("RUSTIO_CORE_PATH") {
        return format!(r#"{{ path = "{path}" }}"#);
    }
    // `CARGO_MANIFEST_DIR` is baked in at build time and points at
    // `…/rustio-cli`. When the binary ships via crates.io the sibling
    // directory won't exist and this check falls through.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let sibling = std::path::Path::new(manifest_dir)
        .parent()
        .map(|p| p.join("rustio-core"));
    if let Some(path) = sibling {
        if path.join("Cargo.toml").is_file() {
            if let Some(s) = path.to_str() {
                return format!(r#"{{ path = "{s}" }}"#);
            }
        }
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
# `chrono` is used for `DateTime<Utc>` model fields. Leave it even if
# your first model only uses primitives — you'll want it the moment you
# add a `created_at` or `published_at` column.
chrono = {{ version = "0.4", default-features = false, features = ["std", "clock"] }}
"#,
        dep = rustio_core_dep(),
    )
}

pub(crate) mod out {
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

A [RustIO](https://github.com/abdulwahed-sweden/rustio) project.

## Run it

    rustio migrate apply      # apply schema changes
    rustio run                # build and start the server on :8000

## Commands

    rustio new app <name>         # scaffold an app inside this project
    rustio migrate generate <n>   # create an empty migration file
    rustio migrate apply [-v]     # apply pending migrations
    rustio migrate status         # show applied + pending
    rustio run                    # build and run the server
    rustio --version              # print CLI version

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
use rustio_core::{Db, Router, Schema, Server};

mod apps;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `rustio schema` invokes this binary with --dump-schema. We emit
    // rustio.schema.json from the in-memory admin registry and exit
    // before doing any I/O — no DB connect, no bound port.
    if std::env::args().any(|a| a == "--dump-schema") {
        let admin = apps::build_admin();
        let schema = Schema::from_admin(&admin);
        schema.write_to(std::path::Path::new("rustio.schema.json"))?;
        eprintln!(
            "wrote rustio.schema.json ({} model{})",
            schema.models.len(),
            if schema.models.len() == 1 { "" } else { "s" },
        );
        return Ok(());
    }

    // Schema is managed by `rustio migrate apply`, which also creates
    // the `rustio_users` / `rustio_sessions` tables auth depends on.
    // Override the database URL with RUSTIO_DATABASE_URL if needed.
    let url = std::env::var("RUSTIO_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());
    let db = Db::connect(&url).await?;

    // Route registration order: the router picks the FIRST match, so
    // register app routes first so they win over framework defaults
    // sharing the same path (e.g. you can override `/` below by adding
    // a handler inside `register_all`).
    //
    // `authenticate(db)` returns a middleware that reads the session
    // cookie on every request, validates it against `rustio_sessions`,
    // and attaches `Identity` to the context when valid.
    let router = Router::new();
    let router = apps::register_all(router, &db);
    let router = with_defaults(router).wrap(authenticate(db.clone()));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8000));
    eprintln!("serving on http://{addr}");
    Server::bind(addr).serve_router(router).await?;
    Ok(())
}
"#;

const APPS_MOD_RS: &str = r#"use rustio_core::admin::Admin;
use rustio_core::{Db, Router};

// -- modules --
// -- end modules --

/// Build the admin registry.
///
/// Split from [`register_all`] so `main.rs --dump-schema` can introspect
/// the admin model list without touching the database or binding a port.
#[allow(unused_mut)]
pub fn build_admin() -> Admin {
    let mut admin = Admin::new();
    // -- admin installs --
    // -- end admin installs --
    admin
}

#[allow(unused_mut, unused_variables)]
pub fn register_all(mut router: Router, db: &Db) -> Router {
    router = build_admin().register(router, db);

    // -- view registrations --
    // -- end view registrations --
    router
}
"#;

const APP_MOD_RS: &str = r#"pub mod admin;
pub mod models;
pub mod views;
"#;

const APP_MODELS_RS: &str = r#"use rustio_core::{Error, Model, Row, RustioAdmin, Value};

/// The {{STRUCT}} model.
///
/// This is a starting point — edit freely. Supported field types are
/// `i32`, `i64`, `String`, `bool`, and `chrono::DateTime<Utc>`. Any of
/// them can be wrapped in `Option<T>` for a nullable column. To add a
/// field:
///
///   1. Add it to the struct below.
///   2. Append its column name to `COLUMNS` (and `INSERT_COLUMNS` if the
///      DB shouldn't autofill it).
///   3. Read it in `from_row` (`row.get_i32`, `row.get_datetime`,
///      `row.get_optional_string`, …) and emit it in `insert_values`.
///   4. Generate a migration to update the table:
///        rustio migrate generate alter_{{TABLE}}
///      then write the `ALTER TABLE ...` SQL and run `rustio migrate apply`.
///
/// If you add a `DateTime<Utc>` field, make sure the project's
/// `Cargo.toml` depends on `chrono` (e.g. `chrono = "0.4"`).
#[derive(Debug, RustioAdmin)]
pub struct {{STRUCT}} {
    pub id: i64,
    pub title: String,
    pub is_active: bool,
    pub priority: i32,
}

impl Model for {{STRUCT}} {
    const TABLE: &'static str = "{{TABLE}}";
    const COLUMNS: &'static [&'static str] = &["id", "title", "is_active", "priority"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "is_active", "priority"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            is_active: row.get_bool("is_active")?,
            priority: row.get_i32("priority")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.is_active.into(),
            self.priority.into(),
        ]
    }
}
"#;

const APP_ADMIN_RS: &str = r#"use rustio_core::admin::Admin;

use super::models::{{STRUCT}};

/// Contribute this app's models to the shared admin index.
pub fn install(admin: Admin) -> Admin {
    admin.model::<{{STRUCT}}>()
}
"#;

const APP_VIEWS_RS: &str = r###"use rustio_core::{html, Error, Response, Router};

/// Tutorial page for the `{{STRUCT}}` app.
///
/// Hitting `GET /{{NAME}}` returns the HTML below so you can confirm the
/// app is wired up. Replace this handler with your real view — this file
/// is yours to edit freely.
pub fn register(router: Router) -> Router {
    router.get("/{{NAME}}", |_req, _params| async {
        Ok::<Response, Error>(html(WELCOME_HTML))
    })
}

const WELCOME_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{{STRUCT}} — RustIO</title>
<style>
  *, *::before, *::after { box-sizing: border-box; }
  html, body { height: 100%; margin: 0; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         background: #fafafa; color: #222; display: flex; align-items: center; justify-content: center; }
  main { max-width: 32rem; padding: 2.5rem; background: white; border-radius: 8px;
         box-shadow: 0 4px 20px rgba(0,0,0,0.05); text-align: left; }
  h1 { margin: 0 0 0.25rem; font-size: 1.5rem; }
  .tag { color: #888; font-size: 0.9rem; margin: 0 0 1.5rem; }
  p { line-height: 1.55; margin: 0.75rem 0; }
  code { background: #f0f0f2; padding: 0.1rem 0.35rem; border-radius: 3px; font-size: 0.9em; }
  a { color: #0366d6; }
  .actions { margin-top: 1.5rem; display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .btn { padding: 0.55rem 1rem; border-radius: 5px; text-decoration: none; font-size: 0.95rem; font-weight: 500; }
  .btn.primary { background: #222; color: white; }
  .btn.secondary { background: #f0f0f2; color: #222; }
</style>
</head>
<body>
<main>
  <h1>It works.</h1>
  <p class="tag">{{STRUCT}} app · RustIO</p>
  <p>Your <code>{{STRUCT}}</code> app is wired up and serving this page at <code>/{{NAME}}</code>.</p>
  <p>To build a real view, edit <code>apps/{{NAME}}/views.rs</code>. The CRUD admin for this model is already generated and ready to use.</p>
  <div class="actions">
    <a class="btn primary" href="/admin/{{TABLE}}">Open admin</a>
    <a class="btn secondary" href="/">Home</a>
  </div>
</main>
</body>
</html>"##;
"###;

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
    fn parse_init_without_args_triggers_wizard() {
        assert_eq!(
            parse_command(&args(&["init"])).unwrap(),
            Command::Init {
                name: None,
                preset: None,
                app: None,
            },
        );
    }

    #[test]
    fn parse_init_with_name_is_non_interactive() {
        assert_eq!(
            parse_command(&args(&["init", "mysite"])).unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: None,
                app: None,
            },
        );
    }

    #[test]
    fn parse_init_with_name_and_preset() {
        assert_eq!(
            parse_command(&args(&["init", "mysite", "--preset", "blog"])).unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: Some(wizard::Preset::Blog),
                app: None,
            },
        );
    }

    #[test]
    fn parse_init_preset_before_name() {
        assert_eq!(
            parse_command(&args(&["init", "--preset", "api", "mysite"])).unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: Some(wizard::Preset::Api),
                app: None,
            },
        );
    }

    #[test]
    fn parse_init_unknown_preset_errors() {
        assert!(parse_command(&args(&["init", "--preset", "nope"])).is_err());
    }

    #[test]
    fn parse_init_db_flag_is_accepted_but_ignored() {
        // `--db sqlite` is reserved for future drivers. Accepting it today
        // means scripts that write it don't start failing when we do add
        // more drivers.
        assert_eq!(
            parse_command(&args(&["init", "mysite", "--db", "sqlite"])).unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: None,
                app: None,
            },
        );
    }

    #[test]
    fn parse_init_rejects_stray_flags() {
        assert!(parse_command(&args(&["init", "--zzz"])).is_err());
    }

    #[test]
    fn parse_init_app_flag() {
        assert_eq!(
            parse_command(&args(&[
                "init", "mysite", "--preset", "blog", "--app", "books",
            ]))
            .unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: Some(wizard::Preset::Blog),
                app: Some(String::from("books")),
            },
        );
    }

    #[test]
    fn parse_init_app_flag_without_preset() {
        // The wizard will default the preset to Basic; `--app` without a
        // `--preset` on Basic is effectively a no-op (Basic ignores it).
        // The parser accepts it either way.
        assert_eq!(
            parse_command(&args(&["init", "mysite", "--app", "books"])).unwrap(),
            Command::Init {
                name: Some(String::from("mysite")),
                preset: None,
                app: Some(String::from("books")),
            },
        );
    }

    #[test]
    fn parse_init_app_flag_requires_value() {
        assert!(parse_command(&args(&["init", "mysite", "--app"])).is_err());
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
    fn singular_capitalize_strips_trailing_s() {
        assert_eq!(singular_capitalize("listings"), "Listing");
        assert_eq!(singular_capitalize("posts"), "Post");
        assert_eq!(singular_capitalize("users"), "User");
    }

    #[test]
    fn singular_capitalize_leaves_singular_alone() {
        assert_eq!(singular_capitalize("blog"), "Blog");
        assert_eq!(singular_capitalize("post"), "Post");
    }

    #[test]
    fn singular_capitalize_keeps_single_s_name_intact() {
        assert_eq!(singular_capitalize("s"), "S");
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
