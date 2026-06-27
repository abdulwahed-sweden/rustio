use std::fs;
use std::path::Path;
use std::process::{Command as ProcessCommand, ExitCode};

mod wizard;

// ─────────────────────────────────────────────────────────────────
// Help is split into two surfaces:
//
//   USAGE          — the short, everyday help. ~10 commands, no
//                    section for the scripting pipeline, no environment
//                    variables, no legacy / niche surface.
//
//   ADVANCED_USAGE — the scripting + low-level surface. Reachable
//                    via `rustio help advanced`. Adds the typed
//                    pipeline commands, schema regeneration, the
//                    legacy FK retrofit, context inspection, and
//                    environment variables.
//
// The split is the simplest progressive disclosure we can ship today:
// a new user reading `rustio help` is never confronted with vocabulary
// they don't need on day one. Power users get the full surface one
// keystroke away.
// ─────────────────────────────────────────────────────────────────

const USAGE: &str = r#"rustio — the RustIO framework CLI

USAGE:
    rustio <command> [args...]

If you're new: `rustio init <name>` creates a project and opens the
setup menu — a guided walkthrough that proposes a starting shape.
Run `rustio migrate apply` then `rustio run` to bring it up. To
change something later: `rustio evolve "<what you want>"`. That's
the whole loop.

SCAFFOLD
    init [name]                 Wizard (no name) or non-interactive scaffold
                                  (with name). Options:
                                  --preset <basic|blog|api>, --app <name>.
    start                       Open the setup menu in an existing project —
                                  guided wizard, manual mode, or (soon) import.
    new app <name>              Add a new model to the current project.

RUN
    run                         Build (cargo build) and start the server on :8000.

CHANGE
    evolve "<request>"          Describe a change in plain English. RustIO
                                  proposes the diff, shows you the risk, and
                                  applies only what you accept.
    migrate apply [-v]          Apply all pending migrations (verbose with -v).
    migrate status              Show applied + pending migrations.

USERS
    user create [opts]          Create a user (interactive when flags omitted).

HELP
    (no args)                   Context-aware "what should I do next".
    doctor                      Health-check the current project. Prints
                                  pass/warn/fail with a fix hint per check.
    explain <topic>             Short inline docs on a concept.
    --why                       Append to any command for a one-paragraph
                                  explanation without running it.
                                  Example: `rustio migrate apply --why`.

META
    --help, -h                  Print this help.
    --version, -V               Print the CLI version.

For more commands (scripting, low-level operations, legacy retrofits):
    rustio help advanced

For longer-form docs: https://github.com/abdulwahed-sweden/rustio
"#;

const ADVANCED_USAGE: &str = r#"rustio — advanced commands

For day-to-day work, see `rustio help`. The commands below cover
scripting and CI gates, low-level project operations, and a legacy
retrofit. Most users never need them.

SCRIPTING                                       (composes evolve by hand)
    ai plan "<request>" [--save <path>]
                                Parse a request into a typed plan document
                                  (no execution). The interactive wrapper
                                  is `rustio evolve`.
    ai review <path>            Risk / impact / warnings for a saved plan.
    ai validate <path>          Terse validate-only gate for CI. Exit 0/1.
    ai apply <path> [--yes] [--dry-run] [--force]
                                Apply a reviewed plan (writes files, never
                                  runs migrations). `--force` opens the
                                  destructive gate; Critical / developer-
                                  only / PII refusals stay authoritative.

PROJECT                                                 (rarely needed)
    new project <name>          Non-interactive variant of `rustio init <name>`.
    migrate generate <name>     Write an empty migration file under migrations/.
    schema                      Regenerate rustio.schema.json from the in-memory
                                  admin registry.
    view <model> [opts]         Render a model's default (or saved) view to the
                                  terminal with demo rows. Opts: --layout
                                  <table|list|cards|compact>, --save, --from
                                  <path>, --json.

LEGACY                              (only for projects scaffolded before 0.9.0)
    migrate add-fks [--write]   Retrofit FOREIGN KEY clauses onto an 0.8.x
                                  project. Default is dry-run; --write commits.
"#;

const ADVANCED_USAGE_CONTEXT_TAIL: &str = r#"
CONTEXT                              (only relevant when rustio.context.json exists)
    context show                Show parsed context + inferred region / GDPR /
                                  PII fields / industry conventions.
    context validate            Parse rustio.context.json; exit 0/1.
"#;

const ADVANCED_USAGE_ENV_TAIL: &str = r#"
ENVIRONMENT
    RUSTIO_DATABASE_URL         Database URL (default: sqlite://app.db?mode=rwc).
    RUSTIO_CORE_PATH            Override the `rustio-core` path dep in generated
                                  Cargo.toml — point at a checkout instead of
                                  crates.io.
    NO_COLOR                    Disable coloured CLI output.
"#;

/// Print `rustio help advanced`. The CONTEXT block only renders when
/// the current directory actually has a `rustio.context.json` — for
/// the 95 % of projects without one, the section is just noise.
fn print_advanced_help() {
    print!("{ADVANCED_USAGE}");
    if Path::new("rustio.context.json").exists() {
        print!("{ADVANCED_USAGE_CONTEXT_TAIL}");
    }
    print!("{ADVANCED_USAGE_ENV_TAIL}");
}

const DEFAULT_DATABASE_URL: &str = "sqlite://app.db?mode=rwc";

#[tokio::main]
async fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().collect();
    // Universal `--why` flag: strip it before parsing the command,
    // then print the command's "why" blurb and exit success without
    // running the action. Helps a new user check "what does this do?"
    // without committing.
    let (args, why_mode) = strip_why_flag(raw);

    let result = match parse_command(&args) {
        Ok(Command::Help) => {
            if why_mode {
                why_for_help();
                Ok(())
            } else {
                print!("{USAGE}");
                Ok(())
            }
        }
        Ok(Command::HelpAdvanced) => {
            if why_mode {
                // Same "why" as `rustio help` — both print docs.
                why_for_help();
                Ok(())
            } else {
                print_advanced_help();
                Ok(())
            }
        }
        Ok(Command::Version) => {
            if why_mode {
                why_for("version");
                Ok(())
            } else {
                println!("rustio {}", env!("CARGO_PKG_VERSION"));
                Ok(())
            }
        }
        Ok(Command::Default) => {
            if why_mode {
                why_for("default");
                Ok(())
            } else {
                default_action()
            }
        }
        Ok(Command::Doctor) => {
            if why_mode {
                why_for("doctor");
                Ok(())
            } else {
                doctor_command()
            }
        }
        Ok(Command::Explain(topic)) => {
            if why_mode {
                why_for("explain");
                Ok(())
            } else {
                explain_command(&topic)
            }
        }
        Ok(Command::Init { name, preset, app }) => {
            if why_mode {
                why_for("init");
                Ok(())
            } else {
                init_command(name, preset, app)
            }
        }
        Ok(Command::NewProject(name)) => {
            if why_mode {
                why_for("new-project");
                Ok(())
            } else {
                new_project(&name)
            }
        }
        Ok(Command::NewApp(name)) => {
            if why_mode {
                why_for("new-app");
                Ok(())
            } else {
                new_app(&name)
            }
        }
        Ok(Command::Run) => {
            if why_mode {
                why_for("run");
                Ok(())
            } else {
                run()
            }
        }
        Ok(Command::Start) => {
            if why_mode {
                why_for("start");
                Ok(())
            } else {
                start_command()
            }
        }
        Ok(Command::MigrateGenerate(name)) => {
            if why_mode {
                why_for("migrate-generate");
                Ok(())
            } else {
                migrate_generate(&name)
            }
        }
        Ok(Command::MigrateApply { verbose }) => {
            if why_mode {
                why_for("migrate-apply");
                Ok(())
            } else {
                migrate_apply(verbose).await
            }
        }
        Ok(Command::MigrateStatus) => {
            if why_mode {
                why_for("migrate-status");
                Ok(())
            } else {
                migrate_status().await
            }
        }
        Ok(Command::MigrateAddFks { write }) => {
            if why_mode {
                why_for("migrate-add-fks");
                Ok(())
            } else {
                migrate_add_fks(write)
            }
        }
        Ok(Command::Schema) => {
            if why_mode {
                why_for("schema");
                Ok(())
            } else {
                schema_command()
            }
        }
        Ok(Command::View {
            model,
            layout,
            save,
            from,
            json,
        }) => {
            if why_mode {
                why_for("view");
                Ok(())
            } else {
                view_command(&model, layout, save, from.as_deref(), json)
            }
        }
        Ok(Command::Ai(sub)) => {
            if why_mode {
                why_for("ai");
                Ok(())
            } else {
                ai_command(sub)
            }
        }
        Ok(Command::Evolve { prompt }) => {
            if why_mode {
                why_for("evolve");
                Ok(())
            } else {
                evolve_command(prompt)
            }
        }
        Ok(Command::Context(sub)) => {
            if why_mode {
                why_for("context");
                Ok(())
            } else {
                context_command(sub)
            }
        }
        Ok(Command::UserCreate {
            email,
            password,
            role,
        }) => {
            if why_mode {
                why_for("user-create");
                Ok(())
            } else {
                user_create_command(email, password, role).await
            }
        }
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
    /// `rustio` with no args — print a context-aware "what should I do next"
    /// for the current directory. Detects project state and suggests
    /// the most useful command; never silently dumps the full help.
    Default,
    /// `rustio init` — interactive wizard when no name is provided,
    /// non-interactive scaffold when a name is given.
    Init {
        name: Option<String>,
        preset: Option<wizard::Preset>,
        app: Option<String>,
    },
    NewProject(String),
    NewApp(String),
    /// `rustio start` — the recommended entry point for new projects.
    /// Opens a small menu (Guided / Manual / Import) and dispatches.
    /// The Guided path is the conversational wizard introduced in
    /// 0.10.x; the underlying machinery is the same module used by
    /// the post-`init` offer.
    Start,
    Run,
    MigrateGenerate(String),
    MigrateApply {
        verbose: bool,
    },
    MigrateStatus,
    /// `rustio migrate add-fks` — 0.9.0 retrofit. Scans `rustio.schema.json`
    /// for belongs_to relations materialised before 0.9.0 (no
    /// `on_delete` metadata) and generates a recreate-table migration
    /// for every affected table. Default is dry-run; `--write` commits
    /// the files to `migrations/`.
    MigrateAddFks {
        write: bool,
    },
    /// Emit `rustio.schema.json` at the project root by running the
    /// built binary with `--dump-schema`.
    Schema,
    /// `rustio view <MODEL> [--layout …] [--save] [--from <path>] [--json]`
    /// — derive (or load) a ViewSpec for one model and render demo rows to
    /// the terminal. Read-only unless `--save` is given; no web layer.
    View {
        model: String,
        layout: Option<rustio_core::viewspec::ViewLayout>,
        save: bool,
        from: Option<String>,
        json: bool,
    },
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
    /// `rustio context show` / `rustio context validate` — 0.6.0.
    /// Inspects `rustio.context.json`.
    Context(ContextCommand),
    /// `rustio doctor` — health check for the current project.
    /// Walks a fixed list of "is this set up correctly?" questions and
    /// prints pass/warn/fail with a fix hint per check.
    Doctor,
    /// `rustio explain <topic>` — inline mini-docs. Prints a short,
    /// jargon-free explanation of a framework concept + a runnable
    /// example. Topics: model, migration, schema, app, admin, route,
    /// ai, context, rbac.
    Explain(String),
    /// `rustio evolve "<request>"` — friendly interactive verb for
    /// changing the schema after the project is up.
    ///
    /// Internally wires together the same `generate_plan` →
    /// `review_plan` → `execute_plan_document` calls the lower-level
    /// `ai plan/review/apply` commands compose, but presents them as
    /// one continuous flow with a blueprint summary and a three-way
    /// choice (Apply / Show technical details / Cancel) — the same
    /// progressive-disclosure UX the setup wizard uses.
    ///
    /// This is what new users see; `ai plan/review/apply` survive as
    /// a scriptable surface for CI gates (documented under
    /// `rustio help advanced`, never named "AI" in user-facing copy).
    Evolve {
        prompt: String,
    },
    Version,
    Help,
    /// `rustio help advanced` — lower-level scripting + niche
    /// commands. Surfaced via a dedicated subcommand so the default
    /// `rustio help` can stay short and focused on the everyday loop.
    HelpAdvanced,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ContextCommand {
    /// `rustio context show` — print the parsed context + inferred
    /// fields (GDPR, region, industry schema conventions).
    Show,
    /// `rustio context validate` — exit 0 if the file parses cleanly,
    /// 1 otherwise.
    Validate,
}

/// `rustio ai …` subcommands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AiCommand {
    /// `rustio ai` — informational summary of the AI boundary. No edits.
    Overview(Option<String>),
    /// `rustio ai plan "<prompt>" [--save <path>]`. Runs the planner.
    /// With `--save`, writes a reviewable [`PlanDocument`] JSON to
    /// `<path>` and prints the review summary. Never executes.
    Plan {
        prompt: String,
        save: Option<String>,
    },
    /// `rustio ai review <path>` — load a saved plan (document or
    /// raw), validate it against the current schema, and print a
    /// human-readable review.
    Review(String),
    /// `rustio ai validate <path>` — terse machine-friendly exit:
    /// 0 if the plan still passes `Plan::validate(&schema)`, 1
    /// otherwise. Designed for CI gates.
    Validate(String),
    /// `rustio ai apply <path> [--yes] [--dry-run]` — apply a reviewed
    /// plan. Prints a preview, prompts, writes files atomically.
    ///
    /// 0.9.1: `--force` unlocks `remove_field` and `remove_relation`
    /// primitives. Critical-risk plans, developer-only primitives, and
    /// PII policy refusals are **not** bypassed by `--force` — those
    /// gates live a layer above the destructive-op gate.
    Apply {
        path: String,
        assume_yes: bool,
        dry_run: bool,
        force: bool,
    },
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args.get(1).map(String::as_str) {
        None => Ok(Command::Default),
        Some("--help") | Some("-h") | Some("help") => {
            // `rustio help` → short, everyday surface.
            // `rustio help advanced` → scripting + low-level + legacy
            //   + (conditionally) context. Kept separate so day-one
            //   users never trip over an "ADVANCED" section in the
            //   default help.
            match args.get(2).map(String::as_str) {
                None => Ok(Command::Help),
                Some("advanced") => {
                    if args.len() > 3 {
                        return Err(format!("unexpected argument `{}`", args[3]));
                    }
                    Ok(Command::HelpAdvanced)
                }
                Some(other) => Err(format!(
                    "unknown help section `{other}` (try `rustio help` or `rustio help advanced`)"
                )),
            }
        }
        Some("--version") | Some("-V") | Some("version") => Ok(Command::Version),
        Some("doctor") => {
            if args.len() > 2 {
                return Err(format!("unexpected argument `{}`", args[2]));
            }
            Ok(Command::Doctor)
        }
        Some("explain") => match args.get(2) {
            Some(topic) => {
                if args.len() > 3 {
                    return Err(format!("unexpected argument `{}`", args[3]));
                }
                Ok(Command::Explain(topic.clone()))
            }
            None => Err(
                "usage: rustio explain <topic>  (try `rustio explain model`, `rustio explain ai`, …)"
                    .into(),
            ),
        },
        Some("run") => {
            if args.len() > 2 {
                return Err(format!("unexpected argument `{}`", args[2]));
            }
            Ok(Command::Run)
        }
        Some("start") => {
            if args.len() > 2 {
                return Err(format!("unexpected argument `{}`", args[2]));
            }
            Ok(Command::Start)
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
        Some("view") => parse_view_args(&args[2..]),
        Some("evolve") => parse_evolve_args(&args[2..]),
        Some("ai") => parse_ai_command(&args[2..]),
        Some("context") => match args.get(2).map(String::as_str) {
            Some("show") => {
                if args.len() > 3 {
                    return Err(format!("unexpected argument `{}`", args[3]));
                }
                Ok(Command::Context(ContextCommand::Show))
            }
            Some("validate") => {
                if args.len() > 3 {
                    return Err(format!("unexpected argument `{}`", args[3]));
                }
                Ok(Command::Context(ContextCommand::Validate))
            }
            Some(other) => Err(format!("unknown subcommand `context {other}`")),
            None => Err("usage: rustio context <show|validate>".into()),
        },
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
            Some("add-fks") => {
                let mut write = false;
                for a in &args[3..] {
                    match a.as_str() {
                        "--write" => write = true,
                        other => return Err(format!("unexpected argument `{other}`")),
                    }
                }
                Ok(Command::MigrateAddFks { write })
            }
            Some(other) => Err(format!("unknown subcommand `migrate {other}`")),
            None => Err("usage: rustio migrate <generate|apply|status|add-fks>".into()),
        },
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

/// Parse arguments to `rustio view`. Accepts one positional `MODEL`
/// name plus the flags `--layout <table|list|cards|compact>`, `--save`,
/// `--from <schema_path>`, and `--json`.
fn parse_view_args(rest: &[String]) -> Result<Command, String> {
    let usage =
        "usage: rustio view <model> [--layout table|list|cards|compact] [--save] [--from <path>] [--json]";
    let mut model: Option<String> = None;
    let mut layout: Option<rustio_core::viewspec::ViewLayout> = None;
    let mut save = false;
    let mut from: Option<String> = None;
    let mut json = false;

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--layout" => {
                let v = rest
                    .get(i + 1)
                    .ok_or("usage: rustio view <model> --layout <table|list|cards|compact>")?;
                layout = Some(parse_layout(v)?);
                i += 2;
            }
            "--from" => {
                let v = rest
                    .get(i + 1)
                    .ok_or("usage: rustio view <model> --from <schema_path>")?;
                from = Some(v.clone());
                i += 2;
            }
            "--save" => {
                save = true;
                i += 1;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            other if other.starts_with('-') => {
                return Err(format!("unexpected argument `{other}`"));
            }
            other if model.is_none() => {
                model = Some(other.to_string());
                i += 1;
            }
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }

    let model = model.ok_or(usage)?;
    Ok(Command::View {
        model,
        layout,
        save,
        from,
        json,
    })
}

/// Map a `--layout` value to a [`rustio_core::viewspec::ViewLayout`].
fn parse_layout(s: &str) -> Result<rustio_core::viewspec::ViewLayout, String> {
    use rustio_core::viewspec::ViewLayout;
    match s {
        "table" => Ok(ViewLayout::Table),
        "list" => Ok(ViewLayout::List),
        "cards" => Ok(ViewLayout::Cards),
        "compact" => Ok(ViewLayout::Compact),
        other => Err(format!(
            "unknown layout `{other}` (expected table, list, cards, or compact)"
        )),
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
    wizard::execute(&plan)?;

    // After the project is scaffolded, offer the AI-assisted wizard.
    // `wizard::execute` only `chdir`s into the new project when it
    // scaffolded an app — otherwise we're still in the parent dir.
    // Always step into the project here so the post-init prompts see
    // the right tree.
    //
    // We skip the offer (and stay silent) when stdin isn't a terminal —
    // CI / piped runs of `rustio init` shouldn't pause for input.
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        if Path::new(&plan.project_name).exists() {
            // The wizard already chdir'd if it ran a per-app scaffold;
            // a second chdir into the same path then fails. Guard by
            // checking whether `apps/mod.rs` is already visible from cwd.
            if !Path::new("apps/mod.rs").exists() {
                std::env::set_current_dir(&plan.project_name)
                    .map_err(|e| format!("failed to enter `{}`: {e}", plan.project_name))?;
            }
        }
        offer_start_menu_after_init()?;
    }
    Ok(())
}

/// Open the post-init menu — same one as `rustio start`, just chained
/// onto the end of `rustio init` so the onboarding is one continuous
/// experience rather than two disjoint commands.
///
/// Forgiving: anything short of stdin EOF is downgraded to a printed
/// hint, so a partially set-up project never blocks the user from
/// getting to the regular `rustio run` path.
fn offer_start_menu_after_init() -> Result<(), String> {
    // The guided path reads `rustio.schema.json`; on a brand-new project
    // it isn't there yet. Generate it up front so both Guided and Manual
    // paths see a consistent project state. First build can be slow on
    // a clean machine — say so plainly.
    if !Path::new("rustio.schema.json").exists() {
        println!();
        out::info("Generating rustio.schema.json (first build can take ~30s) …");
        if let Err(e) = try_dump_schema() {
            println!();
            out::info(&format!("could not generate the schema: {e}"));
            out::hint("rustio schema && rustio start   # try again after the first compile");
            return Ok(());
        }
    }

    match start_command() {
        Ok(()) => Ok(()),
        Err(e) => {
            // Cancellations / interrupts inside the menu shouldn't fail
            // the surrounding `rustio init` — just surface the hint.
            println!();
            out::info(&format!("setup menu exited: {e}"));
            out::hint("rustio start           # open the setup menu any time");
            Ok(())
        }
    }
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

    // Helper / starter files — zero-config knobs the developer can edit:
    //   rustio.design.json  → brand identity (name, logo, colours)
    //   rustio.locale.json  → admin UI translations (add languages here)
    //   DEVELOPMENT.md      → a plain-English guide to everything above
    let display = capitalize(name);
    let initial = display
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "R".to_string());
    fs::write(
        root.join("rustio.design.json"),
        render(DESIGN_JSON, &[("NAME", &display), ("INITIAL", &initial)]),
    )
    .map_err(err_str)?;
    fs::write(root.join("rustio.locale.json"), LOCALE_JSON).map_err(err_str)?;
    fs::write(
        root.join("DEVELOPMENT.md"),
        render(DEVELOPMENT_MD, &[("NAME", &display)]),
    )
    .map_err(err_str)?;

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
            "no Cargo.toml in current directory — this command runs from inside a RustIO \
             project. Start one with `rustio init <name>` or `cd` into an existing project."
                .into(),
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

/// `rustio migrate add-fks` — 0.9.0 retrofit.
///
/// Parses `rustio.schema.json`, finds belongs_to relations lacking
/// `on_delete` metadata, and emits a recreate-table migration per
/// affected table. Default is dry-run — the user has to pass
/// `--write` to actually create files under `migrations/`.
fn migrate_add_fks(write: bool) -> Result<(), String> {
    let path = Path::new("rustio.schema.json");
    let raw = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "cannot read {path}: {e}. Run `rustio schema` first.",
            path = path.display()
        )
    })?;
    let schema: rustio_core::schema::Schema = serde_json::from_str(&raw)
        .map_err(|e| format!("could not parse rustio.schema.json: {e}"))?;

    let report = rustio_core::ai::plan_retrofit_foreign_keys(&schema);

    if report.upgraded.is_empty() {
        out::info("All belongs_to relations already carry FK metadata — nothing to retrofit.");
        return Ok(());
    }

    println!("{}", out::bold("Relations to retrofit:"));
    for (model, field) in &report.upgraded {
        println!("  {} {model}.{field}  →  ON DELETE RESTRICT", out::check());
    }
    println!();
    println!("{}", out::bold("Migrations that would be written:"));
    for (name, _) in &report.migrations {
        println!("  migrations/<NNNN>_{name}.sql");
    }
    println!();

    if !write {
        out::info("Dry run. Re-run with --write to commit the migrations.");
        out::info("Review each SQL file before running `rustio migrate apply`.");
        return Ok(());
    }

    let dir = Path::new("migrations");
    let mut written = Vec::new();
    for (name, sql) in &report.migrations {
        let p = rustio_core::migrations::generate(dir, name, sql).map_err(err_str)?;
        written.push(
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
    }
    println!("{}", out::bold("Wrote:"));
    for f in &written {
        println!("  {} migrations/{f}", out::check());
    }
    println!();
    out::success("Next step", "review the SQL, then `rustio migrate apply`.");
    Ok(())
}

/// `rustio schema` — compile + run the project with `--dump-schema`.
/// The generated `main.rs` watches for that flag, invokes
/// `rustio_core::Schema::from_admin`, and writes `rustio.schema.json`
/// before returning. This CLI command is a thin driver over that.
fn schema_command() -> Result<(), String> {
    if !Path::new("Cargo.toml").exists() {
        return Err(
            "no Cargo.toml in current directory — this command runs from inside a RustIO \
             project. Start one with `rustio init <name>` or `cd` into an existing project."
                .into(),
        );
    }
    try_dump_schema()?;
    out::info("");
    out::info("Next:");
    out::hint("review rustio.schema.json — every external tool reads from this file");
    out::hint("`rustio start` — onboard a new project, or `rustio evolve \"<change>\"` to change this one");
    Ok(())
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
    out::info("");
    out::info("Next:");
    out::hint("`rustio run` — then sign in at http://127.0.0.1:8000/admin");
    Ok(())
}

/// Parse `rustio evolve "<request>"`. The prompt can be supplied as
/// a single quoted token or as a sequence of bare words — we join
/// `rest` with a single space, which handles both shapes.
///
/// An empty prompt is a usage error. We deliberately do not accept a
/// `--save` flag here (the way `rustio ai plan` does): `evolve` is
/// the interactive surface that applies changes immediately, so
/// there's nothing to save.
fn parse_evolve_args(rest: &[String]) -> Result<Command, String> {
    let prompt = rest.join(" ").trim().to_string();
    if prompt.is_empty() {
        return Err("usage: rustio evolve \"<change request>\"".into());
    }
    Ok(Command::Evolve { prompt })
}

/// Parse the args after `rustio ai` into an [`AiCommand`]. Keeps
/// command-string parsing out of `parse_command` so the `ai` subtree
/// can grow independently.
fn parse_ai_command(rest: &[String]) -> Result<Command, String> {
    match rest.first().map(String::as_str) {
        Some("start") => Err(
            "`rustio ai start` was promoted to `rustio start` — same flow, new name.".into(),
        ),
        Some("plan") => parse_ai_plan_args(&rest[1..]),
        Some("review") => {
            let path = rest
                .get(1)
                .ok_or("usage: rustio ai review <path-to-plan.json>")?;
            if rest.len() > 2 {
                return Err(format!("unexpected argument `{}`", rest[2]));
            }
            Ok(Command::Ai(AiCommand::Review(path.clone())))
        }
        Some("validate") => {
            let path = rest
                .get(1)
                .ok_or("usage: rustio ai validate <path-to-plan.json>")?;
            if rest.len() > 2 {
                return Err(format!("unexpected argument `{}`", rest[2]));
            }
            Ok(Command::Ai(AiCommand::Validate(path.clone())))
        }
        Some("apply") => parse_ai_apply_args(&rest[1..]),
        Some(other) if !other.starts_with("--") => {
            // Back-compat: `rustio ai add foo` (pre-plan syntax) still
            // reaches the informational overview with an "intent"
            // summary so existing muscle memory doesn't break.
            Ok(Command::Ai(AiCommand::Overview(Some(rest.join(" ")))))
        }
        Some(flag) => Err(format!(
            "unknown flag `{flag}` (try `rustio ai plan \"…\"`, `rustio ai review <path>`, or `rustio ai validate <path>`)"
        )),
        None => Ok(Command::Ai(AiCommand::Overview(None))),
    }
}

/// Parse `rustio ai plan …` arguments: collects `--save <path>` (or
/// `--save=<path>`) and treats everything else as the prompt.
fn parse_ai_plan_args(rest: &[String]) -> Result<Command, String> {
    let mut save: Option<String> = None;
    let mut prompt_tokens: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(v) = a.strip_prefix("--save=") {
            if v.is_empty() {
                return Err("missing value for --save (expected a file path)".into());
            }
            save = Some(v.to_string());
            i += 1;
        } else if a == "--save" {
            let v = rest
                .get(i + 1)
                .ok_or("missing value for --save (expected a file path)")?;
            save = Some(v.clone());
            i += 2;
        } else {
            prompt_tokens.push(a.clone());
            i += 1;
        }
    }
    let prompt = prompt_tokens.join(" ");
    if prompt.trim().is_empty() {
        return Err(
            "usage: rustio ai plan \"<natural language request>\" [--save <path>]".to_string(),
        );
    }
    Ok(Command::Ai(AiCommand::Plan { prompt, save }))
}

fn ai_command(sub: AiCommand) -> Result<(), String> {
    match sub {
        AiCommand::Overview(intent) => ai_overview(intent),
        AiCommand::Plan { prompt, save } => ai_plan_command(prompt, save),
        AiCommand::Review(path) => ai_review_command(&path),
        AiCommand::Validate(path) => ai_validate_command(&path),
        AiCommand::Apply {
            path,
            assume_yes,
            dry_run,
            force,
        } => ai_apply_command(&path, assume_yes, dry_run, force),
    }
}

/// Parse `rustio ai apply …`: one positional path, optional `--yes`
/// (skip confirmation), `--dry-run` (print preview only), and
/// `--force` (0.9.1 — unlocks destructive primitives `remove_field` /
/// `remove_relation`). Critical / developer-only / PII gates are
/// **not** bypassed by `--force`.
fn parse_ai_apply_args(rest: &[String]) -> Result<Command, String> {
    let mut path: Option<String> = None;
    let mut assume_yes = false;
    let mut dry_run = false;
    let mut force = false;
    for a in rest {
        match a.as_str() {
            "--yes" | "-y" => assume_yes = true,
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown flag `{other}`"));
            }
            other => {
                if path.is_some() {
                    return Err(format!("unexpected argument `{other}`"));
                }
                path = Some(other.to_string());
            }
        }
    }
    let path =
        path.ok_or("usage: rustio ai apply <path-to-plan.json> [--yes] [--dry-run] [--force]")?;
    Ok(Command::Ai(AiCommand::Apply {
        path,
        assume_yes,
        dry_run,
        force,
    }))
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
fn ai_plan_command(prompt: String, save: Option<String>) -> Result<(), String> {
    use rustio_core::ai::generate_plan;
    use rustio_core::ai::planner::{
        render_plan_human, render_plan_json, ContextConfig, PlanRequest,
    };
    use rustio_core::ai::review::{
        build_plan_document, render_plan_document_json, render_review_human, review_plan,
        ReviewHeader,
    };

    let schema = load_project_schema()?;

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

    // --save bypasses the plain JSON shape; it writes a reviewable
    // PlanDocument and prints the review. Keeps stdout useful for
    // operators rather than noisy.
    if let Some(path) = save {
        let doc = build_plan_document(&schema, &prompt, &result, context.as_ref())
            .map_err(|e| format!("could not build reviewable plan document: {e}"))?;
        let json = render_plan_document_json(&doc)
            .map_err(|e| format!("could not serialise plan document: {e}"))?;
        write_atomically(Path::new(&path), json.as_bytes())
            .map_err(|e| format!("could not write `{path}`: {e}"))?;
        // Second pass: review the saved plan so the operator sees the
        // same risk/impact/warnings they will see on `ai review`.
        let review = review_plan(&schema, &doc.plan, context.as_ref())
            .map_err(|e| format!("could not review saved plan: {e}"))?;
        let header = ReviewHeader {
            prompt: Some(doc.prompt.clone()),
            explanation: Some(doc.explanation.clone()),
            source: Some(path.clone()),
        };
        print!("{}", render_review_human(&review, Some(&header)));
        out::success("saved", &format!("plan document → {path}"));
        return Ok(());
    }

    // 1. Strict JSON shape documented for the 0.5.0 planner.
    println!("{}", render_plan_json(&result.plan, &result.explanation));
    // 2. Human-readable block — goes after, separated by a blank line.
    println!();
    print!("{}", render_plan_human(&result.plan, &result.explanation));
    Ok(())
}

/// `rustio ai review <path>` — load a saved plan (document or raw
/// plan), validate it against the current schema, and print an
/// operator-friendly review. Never executes.
fn ai_review_command(path: &str) -> Result<(), String> {
    use rustio_core::ai::review::{
        load_plan, render_review_human, review_plan, LoadedPlan, ReviewHeader,
    };

    let schema = load_project_schema()?;
    let json =
        fs::read_to_string(Path::new(path)).map_err(|e| format!("could not read `{path}`: {e}"))?;
    let loaded = load_plan(&json).map_err(|e| format!("could not parse `{path}`: {e}"))?;

    let (plan_ref, header) = match &loaded {
        LoadedPlan::Document(doc) => (
            &doc.plan,
            ReviewHeader {
                prompt: Some(doc.prompt.clone()),
                explanation: Some(doc.explanation.clone()),
                source: Some(format!("{path} (document v{})", doc.version)),
            },
        ),
        LoadedPlan::RawPlan(plan) => (
            plan,
            ReviewHeader {
                prompt: None,
                explanation: None,
                source: Some(format!("{path} (raw plan)")),
            },
        ),
    };

    let context = load_project_context()?;
    let review = review_plan(&schema, plan_ref, context.as_ref())
        .map_err(|e| format!("review failed: {e}"))?;
    print!("{}", render_review_human(&review, Some(&header)));

    // Exit non-zero when validation fails — `review` is a gate, not
    // just an informational dump. Same-shape command chains (`ai review
    // foo.json && rustio migrate …`) should halt on stale plans.
    if !review.validation.is_valid() {
        return Err("plan is invalid or stale against the current schema".to_string());
    }
    Ok(())
}

/// `rustio ai validate <path>` — terse, CI-shaped gate. Exit 0 if
/// the plan validates; exit 1 with a short reason otherwise. No
/// narrative output.
fn ai_validate_command(path: &str) -> Result<(), String> {
    use rustio_core::ai::review::{load_plan, review_plan, ValidationOutcome};

    let schema = load_project_schema()?;
    let json =
        fs::read_to_string(Path::new(path)).map_err(|e| format!("could not read `{path}`: {e}"))?;
    let loaded = load_plan(&json).map_err(|e| format!("could not parse `{path}`: {e}"))?;
    let plan = loaded.plan();
    let context = load_project_context()?;
    let review =
        review_plan(&schema, plan, context.as_ref()).map_err(|e| format!("review failed: {e}"))?;
    match review.validation {
        ValidationOutcome::Valid => {
            println!(
                "ok: {} step(s) valid against the current schema",
                plan.steps.len()
            );
            Ok(())
        }
        ValidationOutcome::Invalid { step, reason } => {
            Err(format!("invalid at step {step}: {reason}"))
        }
    }
}

/// `rustio ai apply <path> [--yes] [--dry-run]` — the Safe Executor.
///
/// Flow:
///   1. Load schema + plan document (or raw plan).
///   2. Re-review against the current schema (executor re-runs this
///      internally too; doing it here lets us print the review to the
///      operator before the confirmation prompt).
///   3. Build an `ExecutionPreview` (pure) and print it as the "Plan
///      to apply" block.
///   4. On `--dry-run`: stop here.
///   5. If `--yes` skip the prompt; else require an interactive `yes`.
///   6. Call `execute_plan_document` which commits atomically.
///   7. Print the post-apply summary and the `rustio migrate apply`
///      hint — we never run migrations ourselves.
fn ai_apply_command(
    path: &str,
    assume_yes: bool,
    dry_run: bool,
    force: bool,
) -> Result<(), String> {
    use std::io::{BufRead, IsTerminal};

    use rustio_core::ai::executor::{
        execute_plan_document, plan_execution, render_preview_human, ExecuteOptions, ProjectView,
    };
    use rustio_core::ai::review::{load_plan, review_plan, LoadedPlan};

    let schema = load_project_schema()?;
    let json =
        fs::read_to_string(Path::new(path)).map_err(|e| format!("could not read `{path}`: {e}"))?;
    let loaded = load_plan(&json).map_err(|e| format!("could not parse `{path}`: {e}"))?;
    let (plan_doc, source_label) = match loaded {
        LoadedPlan::Document(doc) => (doc, format!("{path} (document v{})", 1)),
        LoadedPlan::RawPlan(_) => {
            return Err(format!(
                "`{path}` is a raw plan. `rustio ai apply` needs a saved PlanDocument — run `rustio ai plan \"…\" --save <path>` first."
            ));
        }
    };

    let context = load_project_context()?;

    // Independent review so the operator sees the same risk/impact the
    // saved document claims — drift between document.risk and live
    // review.risk is itself a warning sign.
    let review = review_plan(&schema, &plan_doc.plan, context.as_ref())
        .map_err(|e| format!("review failed: {e}"))?;

    // 0.9.1 — `--force` sets `allow_destructive` for both the dry-run
    // preview and the real apply. The flag never bypasses Critical,
    // developer-only, or PII refusals; those live in `plan_execution`
    // outside `ExecuteOptions`.
    let opts = ExecuteOptions {
        allow_destructive: force,
    };

    // Pure dry-run against the live project.
    let project = ProjectView::from_dir(Path::new(".")).map_err(|e| format!("{e}"))?;
    let preview = plan_execution(&schema, &project, &plan_doc, &opts, context.as_ref())
        .map_err(|e| format!("{e}"))?;

    // Preview + risk tag on stdout.
    print!("{}", render_preview_human(&preview, review.risk));
    println!("\nSource:\n  {source_label}");
    if force {
        println!("  (destructive gate open: --force)");
    }
    if !review.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &review.warnings {
            println!("  - {w}");
        }
    }

    if dry_run {
        println!("\n(dry run — no files written)");
        return Ok(());
    }

    // Confirmation.
    if !assume_yes {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Err(
                "stdin is not a terminal; re-run with --yes to apply non-interactively".to_string(),
            );
        }
        println!("\nProceed? (yes/no)");
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| format!("could not read confirmation: {e}"))?;
        let answer = line.trim().to_lowercase();
        if answer != "yes" && answer != "y" {
            return Err("aborted by user".to_string());
        }
    }

    // Commit.
    let result = execute_plan_document(Path::new("."), &plan_doc, &opts, context.as_ref())
        .map_err(|e| format!("{e}"))?;

    println!();
    out::success(
        "applied",
        &format!(
            "{} step{}",
            result.applied_steps,
            if result.applied_steps == 1 { "" } else { "s" }
        ),
    );
    for f in &result.generated_files {
        out::success("wrote", f);
    }
    println!();
    out::hint("rustio migrate apply   # run the new migration against your DB");
    out::hint("rustio schema          # regenerate rustio.schema.json");
    Ok(())
}

/// `rustio start` — onboarding entry point.
///
/// Shows a small three-way menu (Guided / Manual / Import) and
/// dispatches. The guided path is the conversational wizard; the
/// manual path drops out with a one-line hint pointing at
/// `rustio new app <name>`; the import path is reserved for a future
/// release that reads a `rustio.schema.json` from disk and rebuilds
/// matching `apps/<x>/models.rs` files.
///
/// This is the **canonical first command** for a new project — the
/// rest of the AI vocabulary (`ai plan` / `review` / `apply`) is an
/// advanced surface for evolving an existing schema, not a first
/// impression.
fn start_command() -> Result<(), String> {
    if !Path::new("apps/mod.rs").exists() {
        return Err(
            "not inside a RustIO project — run `rustio init <name>` first, or `cd` into an existing project.".into(),
        );
    }

    println!();
    println!("Welcome.");
    println!();
    println!("  How would you like to begin?");
    println!();

    let choice = inquire::Select::new(
        "Pick one",
        vec![
            "Guided — I'll propose a starting shape and walk it with you",
            "Manual — I'll get out of the way; you add models one at a time",
            "Import — read an existing rustio.schema.json (coming soon)",
        ],
    )
    .with_starting_cursor(0)
    .prompt()
    .map_err(|e| format!("{e}"))?;

    if choice.starts_with("Guided") {
        guided_wizard_command()
    } else if choice.starts_with("Manual") {
        println!();
        println!("  Got it. Build at your own pace:");
        println!();
        out::hint("rustio new app <name>   # one model + admin entry + migration stub");
        out::hint("rustio migrate apply    # apply the migration to the DB");
        out::hint("rustio run              # start the server on :8000");
        println!();
        out::hint("rustio start            # come back to this menu any time");
        Ok(())
    } else {
        println!();
        println!("  Importing from an existing schema isn't wired up yet — it's");
        println!("  the next thing on this front. For now, `rustio start` →");
        println!("  Guided will walk you through a fresh shape.");
        Ok(())
    }
}

/// The conversational wizard itself — formerly `rustio ai start`.
///
/// Reads a single-sentence project description, maps it deterministically
/// to a starter shape via the `intake` module, walks each proposed model
/// with the user one at a time, then runs the resulting plan through the
/// standard review path before materialising files.
///
/// Constraints enforced top-to-bottom:
///   - intake refuses on ambiguous input (no fuzzy guessing).
///   - the wizard prompts per model — accept / skip — so the developer
///     is always the final decider.
///   - the resulting Plan flows through `review_plan` so the user sees
///     the same risk/impact gate every `rustio ai apply` would show.
fn guided_wizard_command() -> Result<(), String> {
    use rustio_core::ai::intake;

    let schema = load_project_schema()?;
    let context = load_project_context()?;

    println!();
    println!("Let's shape your project together.");
    println!();
    println!("  Tell me what you're building. One sentence is enough — I'll");
    println!("  propose a starting shape and walk it with you, one model at");
    println!("  a time. You decide what lands.");
    println!();

    let description = inquire::Text::new("What are you building?")
        .with_help_message("e.g. \"a small clinic with patients and appointments\"")
        .prompt()
        .map_err(|e| format!("{e}"))?;

    let description = description.trim().to_string();
    if description.is_empty() {
        return Err("no description given — re-run when you're ready.".into());
    }

    let Some(sketch) = intake::sketch(&description) else {
        println!();
        println!("  I don't recognise a clear domain in that description.");
        println!("  I can start from these shapes today:");
        println!();
        println!("    · clinic   — patients, doctors, appointments");
        println!("    · blog     — authors, posts");
        println!("    · shop     — products, orders");
        println!("    · crm      — companies, contacts, deals");
        println!("    · tasks    — projects, tasks");
        println!();
        println!("  Try again with one of those words in your sentence, or");
        println!("  add models one at a time with `rustio new app <name>`.");
        return Ok(());
    };

    println!();
    println!("  I read this as a `{}` project.", sketch.domain);
    println!("  {}", sketch.headline);
    println!();
    println!("  Here's what I'd suggest:");
    println!();
    for (i, m) in sketch.models.iter().enumerate() {
        let field_summary: Vec<String> = m.fields.iter().map(|f| f.name.to_string()).collect();
        println!(
            "    {}.  {:<14}  {}",
            i + 1,
            m.struct_name,
            field_summary.join(", ")
        );
    }
    println!();

    let go = inquire::Confirm::new("Walk through these with me?")
        .with_default(true)
        .with_help_message("I'll ask you about each one in turn. You can skip any of them.")
        .prompt()
        .map_err(|e| format!("{e}"))?;
    if !go {
        println!();
        println!("  No problem. Run `rustio start` again whenever you're ready.");
        return Ok(());
    }

    // Walk each model. Accepted ones are accumulated; skipped ones are
    // dropped (and their downstream references would be a `belongs_to`
    // dangle — we refuse to proceed past that to keep the plan valid).
    let mut accepted: Vec<intake::ModelSketch> = Vec::new();
    let mut accepted_names: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for (i, model) in sketch.models.iter().enumerate() {
        println!();
        println!(
            "  ─── {} of {} · {} ──────────",
            i + 1,
            sketch.models.len(),
            model.struct_name
        );
        println!("  {}", model.rationale);
        println!();
        for f in &model.fields {
            let mut line = format!("    · {:<22} {}", f.name, f.ty);
            if f.nullable {
                line.push_str("  (optional)");
            }
            if let Some(target) = f.belongs_to {
                line.push_str(&format!("  → {target}"));
            }
            println!("{line}");
        }
        println!();

        let choice = inquire::Select::new(
            "What should I do?",
            vec!["add — include this model", "skip — leave it out"],
        )
        .with_starting_cursor(0)
        .prompt()
        .map_err(|e| format!("{e}"))?;

        if choice.starts_with("skip") {
            // A skipped parent breaks any downstream `belongs_to`. Rather
            // than silently dropping fields, refuse to continue and explain.
            for later in &sketch.models[i + 1..] {
                for f in &later.fields {
                    if let Some(target) = f.belongs_to {
                        if target == model.struct_name {
                            println!();
                            println!(
                                "  Skipping `{}` would leave `{}.{}` pointing nowhere.",
                                model.struct_name, later.struct_name, f.name
                            );
                            println!("  Stopping here — apply what you've already accepted with `rustio ai apply`,");
                            println!("  or re-run `rustio start` to walk the whole shape again.");
                            // Fall through to the "build a plan from what we have so far" path.
                            return finalise_wizard(
                                &schema,
                                context.as_ref(),
                                &accepted,
                                &description,
                            );
                        }
                    }
                }
            }
            println!("    skipped.");
            continue;
        }

        accepted.push(model.clone());
        accepted_names.insert(model.struct_name);
        println!("    queued.");
    }

    finalise_wizard(&schema, context.as_ref(), &accepted, &description)
}

/// Build a `Plan` from the accepted sketches, review it, and apply it.
/// Factored out so the early-exit "skipping a parent" path can call it
/// with whatever was accepted so far.
fn finalise_wizard(
    schema: &rustio_core::Schema,
    context: Option<&rustio_core::ai::ContextConfig>,
    accepted: &[rustio_core::ai::ModelSketch],
    description: &str,
) -> Result<(), String> {
    use rustio_core::ai::intake;
    use rustio_core::ai::review::review_plan;
    let _ = description; // recorded into the run banner above; keep for symmetry

    if accepted.is_empty() {
        println!();
        println!("  Nothing queued — exiting without changes.");
        return Ok(());
    }

    let plan = intake::plan_for(accepted);

    // Run the review layer so we have risk + warnings to show if the
    // user opts into the technical view. The summary itself stays
    // intentionally non-technical until they ask.
    let review = review_plan(schema, &plan, context).map_err(|e| format!("review failed: {e}"))?;

    // Counts for the blueprint. `relationships` = AddRelation primitives
    // in the plan; the rest are inherent properties of the resulting
    // admin (every model gets list/search/filters/pagination for free),
    // so we state them as guarantees, not counts.
    use rustio_core::ai::Primitive;
    let n_models = accepted.len();
    let n_relations = plan
        .steps
        .iter()
        .filter(|p| matches!(p, Primitive::AddRelation(_)))
        .count();
    let n_migrations = accepted.len(); // one CREATE TABLE per accepted model
    let model_names: Vec<&str> = accepted.iter().map(|m| m.struct_name).collect();

    show_blueprint(&model_names, n_models, n_relations, n_migrations);

    // Three-way choice. Apply lands the files; details opens the
    // technical view (plan ops, risk, warnings) and then re-asks;
    // cancel exits without changes.
    loop {
        let choice = inquire::Select::new(
            "Ready?",
            vec![
                "Apply — write the files",
                "Show technical details — plan, risk, warnings",
                "Cancel — don't change anything",
            ],
        )
        .with_starting_cursor(0)
        .prompt()
        .map_err(|e| format!("{e}"))?;

        if choice.starts_with("Apply") {
            break;
        } else if choice.starts_with("Show") {
            show_technical_details(&plan, &review, accepted);
            // Loop back to the choice menu so the user can apply or
            // cancel after reading the details.
            continue;
        } else {
            println!();
            println!("  No changes written.");
            return Ok(());
        }
    }

    // The AI executor refuses `AddModel` by design — model scaffolding
    // is the wizard's job, not the executor's. We've already shown the
    // user the reviewed plan + risk; now we materialise each accepted
    // model by writing the scaffold directly. The plan itself is kept
    // in memory for the prompt + explanation strings that go into the
    // CLI output, mirroring the `rustio ai apply` summary shape.
    let _ = (schema, plan); // referenced for clarity; not handed downstream

    let mut applied: usize = 0;
    let mut wrote_paths: Vec<String> = Vec::new();
    for model in accepted {
        // Map (column, target_struct) → (column, target_table) for the
        // FK clause. Target table is looked up by struct name across
        // the accepted set; if the target wasn't accepted we already
        // bailed earlier.
        let belongs_to: Vec<(String, String)> = model
            .fields
            .iter()
            .filter_map(|f| {
                f.belongs_to.and_then(|target_struct| {
                    accepted
                        .iter()
                        .find(|m| m.struct_name == target_struct)
                        .map(|m| (f.name.to_string(), m.table.to_string()))
                })
            })
            .collect();

        let fields: Vec<rustio_core::ai::FieldSpec> = model
            .fields
            .iter()
            .map(|f| rustio_core::ai::FieldSpec {
                name: f.name.to_string(),
                ty: f.ty.to_string(),
                nullable: f.nullable,
                editable: true,
            })
            .collect();

        let migration = scaffold_app_with_fields(
            model.table,
            model.struct_name,
            model.table,
            &fields,
            &belongs_to,
        )?;
        out::success("created", &format!("app `{}`", model.table));
        wrote_paths.push(format!("apps/{}/models.rs", model.table));
        wrote_paths.push(migration.display().to_string());
        applied += 1;
    }

    println!();
    out::success(
        "applied",
        &format!("{} model{}", applied, if applied == 1 { "" } else { "s" }),
    );
    for p in &wrote_paths {
        out::success("wrote", p);
    }
    println!();
    println!("  Next:");
    out::hint("rustio migrate apply   # actually create the tables in the DB");
    out::hint("rustio schema          # regenerate rustio.schema.json");
    out::hint("rustio run             # start the server on :8000");
    out::hint("                       # then open http://127.0.0.1:8000/admin");
    Ok(())
}

/// The system-blueprint summary shown after the user finishes the
/// walkthrough. Frames the outcome in terms of *what RustIO is about
/// to build*, not what primitives the plan contains.
///
/// The five lines below are deliberate:
///   - models / relationships are **counts** (they change per project).
///   - admin screens / search-filters-pagination / migrations are
///     **guarantees** — they hold for every model the framework lays
///     down, so the wording is positive and unconditional.
///
/// Power users who want to see the plan ops + risk + warnings reach
/// them through the "Show technical details" option in the prompt
/// that follows this view.
fn show_blueprint(model_names: &[&str], n_models: usize, n_relations: usize, n_migrations: usize) {
    println!();
    println!("  RustIO is ready to create:");
    println!();
    println!(
        "    ✓  {} connected model{} — {}",
        n_models,
        if n_models == 1 { "" } else { "s" },
        model_names.join(", "),
    );
    println!(
        "    ✓  {} relationship{}",
        n_relations,
        if n_relations == 1 { "" } else { "s" },
    );
    println!("    ✓  Admin screens for every model");
    println!("    ✓  Search, filters, and pagination");
    println!(
        "    ✓  {} starter migration{}",
        n_migrations,
        if n_migrations == 1 { "" } else { "s" },
    );
    println!();
}

/// Behind the "Show technical details" toggle. This is where the
/// review-layer vocabulary (plan operations, risk, warnings) lives —
/// available to anyone who asks, never the first impression.
fn show_technical_details(
    plan: &rustio_core::ai::Plan,
    review: &rustio_core::ai::PlanReview,
    accepted: &[rustio_core::ai::ModelSketch],
) {
    use rustio_core::ai::Primitive;

    println!();
    println!("  Technical details");
    println!("  ─────────────────");
    println!();
    println!("  Plan operations ({}):", plan.steps.len());
    for (i, step) in plan.steps.iter().enumerate() {
        let label = match step {
            Primitive::AddModel(m) => {
                format!("add_model     {} ({} fields)", m.name, m.fields.len())
            }
            Primitive::AddRelation(r) => {
                format!("add_relation  {}.{} → {}", r.from, r.via, r.to)
            }
            other => format!("{other:?}"),
        };
        println!("    {}. {}", i + 1, label);
    }
    println!();
    println!("  Risk classification : {:?}", review.risk);
    if review.warnings.is_empty() {
        println!("  Warnings            : none");
    } else {
        println!("  Warnings            :");
        for w in &review.warnings {
            println!("    - {w}");
        }
    }
    println!();
    println!("  Migrations to be written:");
    for m in accepted {
        println!("    migrations/<next>_create_{}.sql", m.table);
    }
    println!();
    let _ = accepted; // referenced above; reserved for future per-model detail
}

// ─────────────────────────────────────────────────────────────────
// `rustio evolve "<request>"` — friendly interactive verb over the
// typed plan/review/apply pipeline.
//
// The composition is intentionally thin: each step calls the same
// rustio_core API the scriptable `ai plan / review / apply` commands
// call. Everything user-facing happens in *this* function — the
// pipeline stays headless and reusable. From the user's perspective:
//
//   $ rustio evolve "add a status field to tasks"
//
//   RustIO is ready to make this change:
//     · add task.status (String)
//
//   ? Ready?
//     › Apply — write the files
//       Show technical details — plan, risk, warnings
//       Cancel — don't change anything
//
// No mention of "AI" anywhere; "plan" / "review" / "apply" are
// internal implementation labels the user never reads.
// ─────────────────────────────────────────────────────────────────

/// Top-level handler for `rustio evolve "<request>"`.
///
/// Reads the project schema + (optional) context, asks the planner to
/// parse the request into a typed `Plan`, reviews it for risk and
/// warnings, presents a one-screen blueprint, and on Apply hands the
/// reviewed PlanDocument to the standard atomic executor. The three-
/// way choice (Apply / Show technical details / Cancel) is the same
/// progressive-disclosure pattern the setup wizard uses — the user
/// only sees primitive-level vocabulary when they ask for it.
fn evolve_command(prompt: String) -> Result<(), String> {
    use rustio_core::ai::executor::{execute_plan_document, ExecuteOptions};
    use rustio_core::ai::review::{build_plan_document, review_plan};
    use rustio_core::ai::{generate_plan, PlanRequest};

    let schema = load_project_schema()?;
    let context = load_project_context()?;

    println!();
    println!("  Working on it…");
    println!();

    // Step 1 — plan. The planner is closed-vocabulary, so an
    // unparseable request returns an error rather than a guess. We
    // surface that to the user as a friendly refusal: better to admit
    // a limit than fake a result.
    let result = match generate_plan(&schema, context.as_ref(), PlanRequest::new(&prompt)) {
        Ok(r) => r,
        Err(e) => {
            println!("  I can't make that change cleanly.");
            println!("    {e}");
            println!();
            println!("  Try a more specific phrasing. RustIO works inside a fixed set of");
            println!("  changes (add field, rename field, add relation, change type, …);");
            println!("  if a request can't fit, it's better to be told than guessed at.");
            return Ok(());
        }
    };

    // Step 2 — review. Risk classification + warnings come from the
    // same review path `rustio ai review` would print, but we only
    // surface them in the technical-details view.
    let review = review_plan(&schema, &result.plan, context.as_ref())
        .map_err(|e| format!("could not review the change: {e}"))?;

    show_evolve_blueprint(&result.plan);

    // Step 3 — three-way interactive choice. Loop so the user can
    // peek at technical details and then come back to apply.
    loop {
        let choice = inquire::Select::new(
            "Ready?",
            vec![
                "Apply — write the files",
                "Show technical details — plan, risk, warnings",
                "Cancel — don't change anything",
            ],
        )
        .with_starting_cursor(0)
        .prompt()
        .map_err(|e| format!("{e}"))?;

        if choice.starts_with("Apply") {
            break;
        } else if choice.starts_with("Show") {
            show_evolve_technical_details(&result.plan, &review);
            continue;
        } else {
            println!();
            println!("  No changes written.");
            return Ok(());
        }
    }

    // Step 4 — apply. Wrap the plan in a `PlanDocument` (same shape
    // the executor accepts from `rustio ai apply`) and hand it to the
    // atomic file-write path. Destructive primitives stay refused
    // here; users who really need them go through the lower-level
    // `ai apply --force` flow with documented review.
    let doc = build_plan_document(&schema, &prompt, &result, context.as_ref())
        .map_err(|e| format!("could not build the change document: {e}"))?;
    let opts = ExecuteOptions {
        allow_destructive: false,
    };
    let exec = execute_plan_document(Path::new("."), &doc, &opts, context.as_ref())
        .map_err(|e| format!("{e}"))?;

    println!();
    out::success(
        "applied",
        &format!(
            "{} step{}",
            exec.applied_steps,
            if exec.applied_steps == 1 { "" } else { "s" }
        ),
    );
    for f in &exec.generated_files {
        out::success("wrote", f);
    }
    println!();
    out::hint("rustio migrate apply   # apply the new migration to your DB");
    out::hint("rustio run             # if the server isn't already up");
    Ok(())
}

/// Render the change set as a small system-blueprint block — one
/// line per change, plain English, no primitive vocabulary. The
/// goal is for the user to read the screen once and know what
/// `Apply` will do without a manual.
fn show_evolve_blueprint(plan: &rustio_core::ai::Plan) {
    use rustio_core::ai::Primitive;

    println!();
    println!("  RustIO is ready to make this change:");
    println!();

    // `evolve` plans are usually 1–3 steps. We render them as
    // bullets in the order the executor will apply them.
    for step in &plan.steps {
        let line = match step {
            Primitive::AddField(a) => {
                let kind = if a.field.nullable {
                    "optional"
                } else {
                    "required"
                };
                format!(
                    "    · add {}.{}  ({}, {})",
                    a.model, a.field.name, a.field.ty, kind
                )
            }
            Primitive::RemoveField(r) => format!("    · remove {}.{}", r.model, r.field),
            Primitive::RenameField(r) => {
                format!("    · rename {}.{} → {}", r.model, r.from, r.to)
            }
            Primitive::ChangeFieldType(c) => format!(
                "    · change type of {}.{} → {}",
                c.model, c.field, c.new_type
            ),
            Primitive::ChangeFieldNullability(c) => {
                let to = if c.nullable { "optional" } else { "required" };
                format!("    · {}.{} now {}", c.model, c.field, to)
            }
            Primitive::AddModel(a) => {
                format!("    · add model {} ({} fields)", a.name, a.fields.len())
            }
            Primitive::RemoveModel(r) => format!("    · remove model {}", r.name),
            Primitive::RenameModel(r) => format!("    · rename {} → {}", r.from, r.to),
            Primitive::AddRelation(r) => {
                format!("    · link {}.{} → {}", r.from, r.via, r.to)
            }
            Primitive::RemoveRelation(r) => {
                format!("    · unlink {}.{}", r.from, r.via)
            }
            Primitive::UpdateAdmin(u) => {
                format!("    · admin update on {}", u.model)
            }
            // Catch-all for primitives we haven't tailored copy for
            // yet. Falls back to the Debug repr so the user still
            // sees something concrete; covers future variants
            // gracefully without a panic.
            other => format!("    · {other:?}"),
        };
        println!("{line}");
    }
    println!();
}

/// Behind the "Show technical details" choice. Plan operations,
/// risk classification, warnings — the same fields `rustio ai review`
/// prints, just labelled in plain English. Available to anyone who
/// asks; never the first impression.
fn show_evolve_technical_details(
    plan: &rustio_core::ai::Plan,
    review: &rustio_core::ai::PlanReview,
) {
    println!();
    println!("  Technical details");
    println!("  ─────────────────");
    println!();
    println!("  Operations ({}):", plan.steps.len());
    for (i, step) in plan.steps.iter().enumerate() {
        println!("    {}. {:?}", i + 1, step);
    }
    println!();
    println!("  Risk classification : {:?}", review.risk);
    if review.warnings.is_empty() {
        println!("  Warnings            : none");
    } else {
        println!("  Warnings            :");
        for w in &review.warnings {
            println!("    - {w}");
        }
    }
    println!();
}

/// Shared schema loader for the AI subcommands — every one of them
/// refuses to proceed without a committed `rustio.schema.json`.
fn load_project_schema() -> Result<rustio_core::Schema, String> {
    let schema_path = Path::new("rustio.schema.json");
    if !schema_path.exists() {
        return Err(
            "rustio.schema.json not found. Run `rustio schema` first to emit it.".to_string(),
        );
    }
    let schema_json = fs::read_to_string(schema_path).map_err(err_str)?;
    rustio_core::Schema::parse(&schema_json).map_err(err_str)
}

/// Shared context loader — reads `rustio.context.json` if present,
/// otherwise returns `Ok(None)` so commands fall back to the no-
/// context codepath unchanged.
fn load_project_context() -> Result<Option<rustio_core::ai::ContextConfig>, String> {
    let ctx_path = Path::new("rustio.context.json");
    if !ctx_path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(ctx_path).map_err(err_str)?;
    let ctx = rustio_core::ai::ContextConfig::parse(&raw).map_err(|e| e.to_string())?;
    Ok(Some(ctx))
}

fn context_command(sub: ContextCommand) -> Result<(), String> {
    match sub {
        ContextCommand::Show => context_show_command(),
        ContextCommand::Validate => context_validate_command(),
    }
}

// ─────────────────────────────────────────────────────────────────
// `rustio view` — derive/load a ViewSpec and render demo rows to the
// terminal. The whole chain (schema → derive → save → load → render)
// lives in rustio-core; the CLI only resolves files, synthesises demo
// rows (no DB yet), and turns the structured RenderedView into text.
// ─────────────────────────────────────────────────────────────────

/// Which ViewSpec the `view` command ended up using, for the status line.
#[derive(Debug, PartialEq)]
enum ViewSource {
    /// A pre-existing `<model>.view.json` was loaded (source of truth).
    SavedLoaded(String),
    /// `--save` wrote a fresh `<model>.view.json` (the derived default).
    Wrote(String),
    /// No saved file and no `--save` — the derived default was used.
    Derived,
}

fn view_command(
    model_name: &str,
    layout: Option<rustio_core::viewspec::ViewLayout>,
    save: bool,
    from: Option<&str>,
    json: bool,
) -> Result<(), String> {
    // 1. Resolve + parse the schema.
    let schema_path: &Path = match from {
        Some(p) => Path::new(p),
        None => Path::new("rustio.schema.json"),
    };
    if !schema_path.exists() {
        return Err(format!(
            "{} not found. Run `rustio schema` first to emit it.",
            schema_path.display()
        ));
    }
    let raw = fs::read_to_string(schema_path).map_err(err_str)?;
    let schema = rustio_core::Schema::parse(&raw).map_err(err_str)?;

    // 2. Find the requested model; on miss, list what's available.
    let model = schema
        .models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| {
            let names: Vec<&str> = schema.models.iter().map(|m| m.name.as_str()).collect();
            format!(
                "model `{model_name}` not found in {}. Available models: {}",
                schema_path.display(),
                names.join(", ")
            )
        })?;

    // 3. Resolve the ViewSpec (saved file wins; --save writes the default).
    let (spec, source) = resolve_view_spec(Path::new("."), model, save)?;
    match &source {
        ViewSource::Wrote(file) => out::success("wrote", &format!("{file} (derived default)")),
        ViewSource::SavedLoaded(file) => out::info(&format!("using saved view: {file}")),
        ViewSource::Derived => out::info("no saved view — using derived default"),
    }

    // 4. Layout: explicit --layout overrides the spec's own default.
    let layout = layout.unwrap_or(spec.layout);

    // 5. Synthesise deterministic demo rows (no DB yet) and render.
    let rows = synth_demo_rows(model);
    let view =
        rustio_core::viewspec::render::RenderedView::render_with_layout(&spec, layout, &rows);

    // 6. Output: structured JSON for scripting, else aligned terminal text.
    if json {
        let pretty = serde_json::to_string_pretty(&view).map_err(err_str)?;
        println!("{pretty}");
    } else {
        print!("{}", render_terminal(&view));
    }
    Ok(())
}

/// Resolve the ViewSpec for `model` relative to `dir` (directory-parameterised
/// so tests don't have to `chdir`).
///
/// - With `save`: refuse if `<model>.view.json` already exists (never
///   overwrite); otherwise derive the default and write it.
/// - Without `save`: load the saved file if present (it is the source of
///   truth), else derive the default in memory.
fn resolve_view_spec(
    dir: &Path,
    model: &rustio_core::schema::SchemaModel,
    save: bool,
) -> Result<(rustio_core::viewspec::ViewSpec, ViewSource), String> {
    use rustio_core::viewspec::ViewSpec;

    let filename = format!("{}.view.json", to_snake_case(&model.name));
    let path = dir.join(&filename);
    let exists = path.exists();

    if save {
        if exists {
            return Err(format!(
                "{filename} already exists — refusing to overwrite. \
                 Delete it or edit it by hand, then re-run."
            ));
        }
        let spec = ViewSpec::from_schema_model(model);
        spec.write_to(&path).map_err(err_str)?;
        return Ok((spec, ViewSource::Wrote(filename)));
    }

    if exists {
        let raw = fs::read_to_string(&path).map_err(err_str)?;
        let spec = ViewSpec::parse(&raw).map_err(err_str)?;
        Ok((spec, ViewSource::SavedLoaded(filename)))
    } else {
        Ok((ViewSpec::from_schema_model(model), ViewSource::Derived))
    }
}

/// `CamelCase` model name → `snake_case` file stem. `Customer` →
/// `customer`, `BlogPost` → `blog_post`. Deterministic.
fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Synthesise three deterministic demo rows from a model's schema fields.
/// The CLI has no database yet; these placeholders exist only so the
/// developer can SEE the layout shape. Values are field-name-based and
/// fixed per row index, so output never varies between runs:
/// `String → "sample <field> <n>"`, integers → `1/2/3`, `bool` →
/// `true`/`false` alternating, `DateTime` → a fixed ISO string per row.
fn synth_demo_rows(
    model: &rustio_core::schema::SchemaModel,
) -> Vec<rustio_core::viewspec::render::Row> {
    use rustio_core::viewspec::render::{Row, RowValue};

    const ISO: [&str; 3] = [
        "2026-06-25T14:30:00Z",
        "2025-01-02T09:05:00Z",
        "2024-11-15T23:59:00Z",
    ];

    (0..3)
        .map(|i| {
            let mut row = Row::new();
            for f in &model.fields {
                let value = match f.ty.as_str() {
                    "i32" | "i64" => RowValue::Int((i + 1) as i64),
                    "bool" => RowValue::Bool(i % 2 == 0),
                    "DateTime" => RowValue::Text(ISO[i].to_string()),
                    _ => RowValue::Text(format!("sample {} {}", f.name, i + 1)),
                };
                row.insert(f.name.clone(), value);
            }
            row
        })
        .collect()
}

/// Turn a [`RenderedView`](rustio_core::viewspec::render::RenderedView)
/// into aligned terminal text. Pure (returns a `String`) so it is
/// testable and deterministic. Respects the renderer's cells exactly —
/// Hidden fields are already absent and nothing here re-reads the schema.
fn render_terminal(view: &rustio_core::viewspec::render::RenderedView) -> String {
    use rustio_core::viewspec::ViewLayout;

    let mut out = String::new();
    out.push_str(&format!(
        "View: {}  ·  layout: {}  ·  rows: {}  (demo data)\n",
        view.model,
        layout_word(view.layout),
        view.rows.len(),
    ));

    if view.rows.is_empty() || view.rows.iter().all(|r| r.cells.is_empty()) {
        out.push_str("\n(nothing to render)\n");
        return out;
    }

    match view.layout {
        ViewLayout::Table => render_table(view, &mut out),
        _ => render_blocks(view, &mut out),
    }
    out
}

/// Aligned columns; cell labels become the header row.
fn render_table(view: &rustio_core::viewspec::render::RenderedView, out: &mut String) {
    let headers: Vec<&str> = view.rows[0]
        .cells
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    let ncol = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in &view.rows {
        for (i, cell) in row.cells.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.value.chars().count());
            }
        }
    }

    let join_cols = |parts: Vec<String>| -> String {
        let mut line = String::new();
        for (i, p) in parts.iter().enumerate() {
            line.push_str(p);
            if i + 1 < ncol {
                line.push_str("  ");
            }
        }
        line.trim_end().to_string()
    };

    out.push('\n');
    out.push_str(&join_cols(
        headers
            .iter()
            .enumerate()
            .map(|(i, h)| pad(h, widths[i]))
            .collect(),
    ));
    out.push('\n');
    out.push_str(&join_cols(widths.iter().map(|w| "-".repeat(*w)).collect()));
    out.push('\n');
    for row in &view.rows {
        out.push_str(&join_cols(
            row.cells
                .iter()
                .enumerate()
                .map(|(i, c)| pad(&c.value, widths[i]))
                .collect(),
        ));
        out.push('\n');
    }
}

/// One role-labeled block per row, for List / Cards / Compact.
fn render_blocks(view: &rustio_core::viewspec::render::RenderedView, out: &mut String) {
    let role_w = view
        .rows
        .iter()
        .flat_map(|r| r.cells.iter())
        .map(|c| role_word(c.role).chars().count())
        .max()
        .unwrap_or(0);
    for (idx, row) in view.rows.iter().enumerate() {
        out.push_str(&format!("\nRow {}\n", idx + 1));
        for cell in &row.cells {
            out.push_str(&format!(
                "  {}  {}\n",
                pad(role_word(cell.role), role_w),
                cell.value
            ));
        }
    }
}

/// Right-pad `s` with spaces to `width` (by character count).
fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

/// Stable lowercase word for a layout (for the header line).
fn layout_word(layout: rustio_core::viewspec::ViewLayout) -> &'static str {
    use rustio_core::viewspec::ViewLayout;
    match layout {
        ViewLayout::Table => "table",
        ViewLayout::List => "list",
        ViewLayout::Cards => "cards",
        ViewLayout::Compact => "compact",
        _ => "unknown",
    }
}

/// Stable label for a view field role (for block rendering).
fn role_word(role: rustio_core::viewspec::FieldRole) -> &'static str {
    use rustio_core::viewspec::FieldRole;
    match role {
        FieldRole::Title => "Title",
        FieldRole::Subtitle => "Subtitle",
        FieldRole::Badge => "Badge",
        FieldRole::Timestamp => "Timestamp",
        FieldRole::Meta => "Meta",
        FieldRole::Hidden => "Hidden",
        _ => "Field",
    }
}

/// `rustio context show` — pretty-print the loaded context plus
/// everything the project derives from it. Helps operators verify
/// that their country / industry selection is doing what they expect.
fn context_show_command() -> Result<(), String> {
    let ctx_path = Path::new("rustio.context.json");
    if !ctx_path.exists() {
        out::info("rustio.context.json not found — the project is running without context.");
        out::hint(
            "Create rustio.context.json with { \"country\": \"SE\", \"industry\": \"housing\" } to opt in.",
        );
        return Ok(());
    }
    let ctx = load_project_context()?.expect("exists check above");
    if ctx.is_empty() {
        out::info("rustio.context.json is present but empty — no signals active.");
        return Ok(());
    }
    println!("Context:");
    if let Some(c) = &ctx.country {
        println!("  country:      {c}");
    }
    match (&ctx.region, ctx.effective_region()) {
        (Some(r), _) => println!("  region:       {r}"),
        (None, Some(inferred)) => println!("  region:       {inferred} (inferred)"),
        _ => {}
    }
    if let Some(i) = &ctx.industry {
        println!("  industry:     {i}");
    }
    if !ctx.compliance.is_empty() {
        println!("  compliance:   {}", ctx.compliance.join(", "));
    }
    if ctx.requires_gdpr() {
        let explicit = ctx
            .compliance
            .iter()
            .any(|c| c.eq_ignore_ascii_case("GDPR"));
        let tag = if explicit {
            ""
        } else {
            " (inferred from EU region)"
        };
        println!("  gdpr:         applies{tag}");
    }
    let pii = ctx.pii_fields();
    if !pii.is_empty() {
        println!("\nPII field names the review layer will escalate:");
        for f in &pii {
            println!("  - {f}");
        }
    }
    if let Some(schema) = ctx.industry_schema() {
        println!("\nIndustry conventions:");
        for line in &schema.conventions {
            println!("  - {line}");
        }
        if !schema.required_fields.is_empty() {
            println!("\nRequired fields (removal warned):");
            for f in &schema.required_fields {
                println!("  - {f}");
            }
        }
    }
    Ok(())
}

/// `rustio context validate` — parse-only check with a minimal
/// one-line result. Exit code is the signal.
fn context_validate_command() -> Result<(), String> {
    let ctx_path = Path::new("rustio.context.json");
    if !ctx_path.exists() {
        println!("ok: no rustio.context.json (running without context)");
        return Ok(());
    }
    let ctx = load_project_context()?.expect("exists check above");
    if ctx.is_empty() {
        println!("ok: file parses and is empty (no signals)");
    } else {
        println!(
            "ok: file parses (country={:?}, industry={:?}, gdpr={})",
            ctx.country,
            ctx.industry,
            ctx.requires_gdpr(),
        );
    }
    Ok(())
}

/// Write `contents` to `path` atomically via a sibling tempfile
/// rename. Matches the pattern `Schema::write_to` uses so both
/// artefacts have the same crash-safety guarantee: a partial write
/// can never be observed by a concurrent reader.
fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    // Best-effort cleanup from a previous aborted run. `write` will
    // surface any real permission problem.
    let _ = fs::remove_file(&tmp);
    fs::write(&tmp, contents).map_err(err_str)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.to_string());
    }
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

/// Scaffold an app with an explicit field set, used by the
/// `rustio ai start` wizard. Same layout as [`new_app`] (mod.rs +
/// models.rs + admin.rs + views.rs + a CREATE TABLE migration) but
/// every Rust file is rendered from the sketch's fields rather than
/// the default `title / is_active / priority` template.
///
/// The function is intentionally permissive about its inputs because
/// the wizard already validated them upstream: every name is a known
/// snake_case identifier and every type is in `VALID_TYPE_NAMES`.
///
/// `belongs_to` carries `(column_name, target_table)` pairs so we can
/// emit a SQL `FOREIGN KEY` clause on a *fresh* table — referential
/// integrity is otherwise blocked until 0.9.0 `migrate add-fks`,
/// but a brand-new table has no pre-existing rows to break.
pub(crate) fn scaffold_app_with_fields(
    app_name: &str,
    struct_name: &str,
    table: &str,
    fields: &[rustio_core::ai::FieldSpec],
    belongs_to: &[(String, String)],
) -> Result<std::path::PathBuf, String> {
    validate_name(app_name)?;
    if !Path::new("apps/mod.rs").exists() {
        return Err(
            "not inside a RustIO project — expected apps/mod.rs in the current directory".into(),
        );
    }
    let app_dir = Path::new("apps").join(app_name);
    if app_dir.exists() {
        return Err(format!("app `{app_name}` already exists"));
    }

    fs::create_dir_all(&app_dir).map_err(err_str)?;
    fs::write(app_dir.join("mod.rs"), APP_MOD_RS).map_err(err_str)?;
    fs::write(
        app_dir.join("models.rs"),
        render_models_rs_with_fields(struct_name, table, fields),
    )
    .map_err(err_str)?;
    fs::write(
        app_dir.join("admin.rs"),
        render(APP_ADMIN_RS, &[("STRUCT", struct_name)]),
    )
    .map_err(err_str)?;
    fs::write(
        app_dir.join("views.rs"),
        render(
            APP_VIEWS_RS,
            &[
                ("NAME", app_name),
                ("STRUCT", struct_name),
                ("TABLE", table),
            ],
        ),
    )
    .map_err(err_str)?;

    register_app_in_mod(app_name)?;

    let create_sql = render_create_table_sql(table, fields, belongs_to);
    let migration_path = rustio_core::migrations::generate(
        Path::new("migrations"),
        &format!("create_{table}"),
        &create_sql,
    )
    .map_err(err_str)?;

    Ok(migration_path)
}

/// Render an `apps/<x>/models.rs` from a custom field list. Mirrors
/// the shape of [`APP_MODELS_RS`] but every column comes from the
/// supplied `FieldSpec`s.
fn render_models_rs_with_fields(
    struct_name: &str,
    table: &str,
    fields: &[rustio_core::ai::FieldSpec],
) -> String {
    let struct_fields = fields
        .iter()
        .map(|f| {
            format!(
                "    pub {}: {},",
                f.name,
                rust_field_type(&f.ty, f.nullable)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let columns_csv: Vec<String> = std::iter::once("\"id\"".to_string())
        .chain(fields.iter().map(|f| format!("\"{}\"", f.name)))
        .collect();
    let insert_csv: Vec<String> = fields.iter().map(|f| format!("\"{}\"", f.name)).collect();

    let from_row = fields
        .iter()
        .map(|f| {
            format!(
                "            {}: row.{}(\"{}\")?,",
                f.name,
                row_getter(&f.ty, f.nullable),
                f.name,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let insert_values = fields
        .iter()
        .map(|f| {
            format!(
                "            {},",
                insert_value_expr(&f.name, &f.ty, f.nullable)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"use rustio_core::{{Error, Model, Row, RustioAdmin, Value}};

/// The {struct_name} model — generated by `rustio start`. Edit freely.
#[derive(Debug, RustioAdmin)]
pub struct {struct_name} {{
    pub id: i64,
{struct_fields}
}}

impl Model for {struct_name} {{
    const TABLE: &'static str = "{table}";
    const COLUMNS: &'static [&'static str] = &[{columns}];
    const INSERT_COLUMNS: &'static [&'static str] = &[{inserts}];

    fn id(&self) -> i64 {{
        self.id
    }}

    fn from_row(row: Row<'_>) -> Result<Self, Error> {{
        Ok(Self {{
            id: row.get_i64("id")?,
{from_row}
        }})
    }}

    fn insert_values(&self) -> Vec<Value> {{
        vec![
{insert_values}
        ]
    }}
}}
"#,
        struct_fields = struct_fields,
        columns = columns_csv.join(", "),
        inserts = insert_csv.join(", "),
        from_row = from_row,
        insert_values = insert_values,
    )
}

fn rust_field_type(ty: &str, nullable: bool) -> String {
    let base = match ty {
        "String" => "String",
        "i32" => "i32",
        "i64" => "i64",
        "bool" => "bool",
        "DateTime" => "chrono::DateTime<chrono::Utc>",
        other => other,
    };
    if nullable {
        format!("Option<{base}>")
    } else {
        base.to_string()
    }
}

fn row_getter(ty: &str, nullable: bool) -> &'static str {
    match (ty, nullable) {
        ("String", false) => "get_string",
        ("String", true) => "get_optional_string",
        ("i32", false) => "get_i32",
        ("i32", true) => "get_optional_i32",
        ("i64", false) => "get_i64",
        ("i64", true) => "get_optional_i64",
        ("bool", false) => "get_bool",
        ("bool", true) => "get_optional_bool",
        ("DateTime", false) => "get_datetime",
        ("DateTime", true) => "get_optional_datetime",
        _ => "get_string",
    }
}

fn insert_value_expr(name: &str, ty: &str, _nullable: bool) -> String {
    // `Value: From<T>` covers every supported type; `Value: From<Option<T>>`
    // covers the optional variants — same expression in both cases.
    // `String` and `DateTime` need a `clone()` so the model stays usable
    // after `insert_values` consumes its fields.
    let needs_clone = ty == "String" || ty == "DateTime";
    if needs_clone {
        format!("self.{name}.clone().into()")
    } else {
        format!("self.{name}.into()")
    }
}

/// Build the `CREATE TABLE` SQL for a wizard-scaffolded model.
fn render_create_table_sql(
    table: &str,
    fields: &[rustio_core::ai::FieldSpec],
    belongs_to: &[(String, String)],
) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(2 + fields.len());
    lines.push("    id INTEGER PRIMARY KEY AUTOINCREMENT,".to_string());
    for f in fields {
        let sqlite_ty = match f.ty.as_str() {
            "String" | "DateTime" => "TEXT",
            "i32" | "i64" | "bool" => "INTEGER",
            _ => "TEXT",
        };
        let null = if f.nullable { "" } else { " NOT NULL" };
        lines.push(format!("    {} {}{},", f.name, sqlite_ty, null));
    }
    for (col, target_table) in belongs_to {
        // `ON DELETE RESTRICT` mirrors the AI executor's default and
        // keeps fresh tables on the same posture the 0.9.x retrofit
        // emits for older projects.
        lines.push(format!(
            "    FOREIGN KEY ({col}) REFERENCES {target_table}(id) ON DELETE RESTRICT,"
        ));
    }
    // Drop the trailing comma on the last entry.
    if let Some(last) = lines.last_mut() {
        if last.ends_with(',') {
            last.pop();
        }
    }
    format!("CREATE TABLE {table} (\n{}\n);\n", lines.join("\n"))
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

// ---------------------------------------------------------------------------
// Beginner-friendly CLI surface (`rustio` no-args / `doctor` / `explain` /
// `--why`).
// ---------------------------------------------------------------------------

/// Strip a leading `--why` from anywhere in the arg list and return
/// `(remaining_args, why_mode_was_set)`. Lets every command be invoked
/// as `rustio <cmd> --why` to print "what does this do" without running
/// the action. We strip pre-parse so the subcommand parsers don't have
/// to know about the flag.
fn strip_why_flag(mut args: Vec<String>) -> (Vec<String>, bool) {
    let mut why = false;
    args.retain(|a| {
        if a == "--why" {
            why = true;
            false
        } else {
            true
        }
    });
    (args, why)
}

/// Snapshot of what the CLI can see about the current directory.
struct ProjectState {
    in_project: bool,
    has_apps: bool,
    has_migrations_dir: bool,
    has_db: bool,
    has_schema: bool,
}

impl ProjectState {
    fn detect() -> Self {
        let in_project = Path::new("Cargo.toml").exists()
            && Path::new("main.rs").exists()
            && Path::new("apps").is_dir();
        let has_apps = Path::new("apps").is_dir()
            && Path::new("apps")
                .read_dir()
                .map(|d| {
                    d.flatten().any(|e| {
                        e.path().is_dir()
                            && e.file_name().to_str().is_some_and(|n| !n.starts_with('.'))
                    })
                })
                .unwrap_or(false);
        let has_migrations_dir = Path::new("migrations").is_dir();
        let has_db = Path::new("app.db").exists();
        let has_schema = Path::new("rustio.schema.json").exists();
        Self {
            in_project,
            has_apps,
            has_migrations_dir,
            has_db,
            has_schema,
        }
    }
}

/// `rustio` (no args) — print a one-screen, context-aware "what should
/// I do next" instead of dumping the full help. Always shows
/// `rustio help` as a fallback at the bottom.
fn default_action() -> Result<(), String> {
    let s = ProjectState::detect();
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| ".".into());

    println!("rustio {}", env!("CARGO_PKG_VERSION"));

    if !s.in_project {
        println!();
        println!("You're not inside a RustIO project right now.");
        println!();
        println!("To start a new project:");
        out::hint("rustio init <name>          (e.g. `rustio init mysite`)");
        out::hint("rustio init                 (interactive wizard)");
        println!();
        out::info("Or run `rustio help` to see every command.");
        return Ok(());
    }

    println!();
    println!("You're in a RustIO project: {cwd}");

    // Detect the most useful next thing in priority order.
    if !s.has_apps {
        println!();
        println!("This project has no apps yet. An app = one model (e.g. `notes`).");
        out::hint("rustio new app <name>       create your first model");
        out::hint("rustio explain app          if you're not sure what an app is");
        return Ok(());
    }

    if !s.has_db || !s.has_migrations_dir {
        println!();
        println!("Your database hasn't been set up yet.");
        out::hint("rustio migrate apply        create tables + run pending migrations");
        out::hint("rustio explain migration    what a migration is");
        return Ok(());
    }

    if !s.has_schema {
        println!();
        println!("`rustio.schema.json` is missing. The AI layer + external tools read it.");
        out::hint("rustio schema               regenerate it from your models");
        return Ok(());
    }

    // All set — suggest the daily-driver commands.
    println!();
    println!("Looks set up. Common next moves:");
    out::hint("rustio run                  start the server on :8000");
    out::hint("rustio migrate status       see what's applied / pending");
    out::hint("rustio doctor               full health check");
    out::hint("rustio new app <name>       add another model");
    println!();
    out::info(
        "Run `rustio help` to see every command, or `rustio explain <topic>` for inline docs.",
    );
    Ok(())
}

/// `rustio doctor` — health check. Walks a fixed list of "is the
/// project set up correctly?" questions and prints pass / warn / fail
/// with a fix hint per item. Never fails (exit 0) even when checks
/// warn — the goal is to surface fixes, not gate.
fn doctor_command() -> Result<(), String> {
    println!("{} Checking your RustIO setup …", out::dot());
    println!();

    let mut warnings = 0u32;
    let mut failures = 0u32;

    // Toolchain
    match ProcessCommand::new("rustc").arg("--version").output() {
        Ok(out_) if out_.status.success() => {
            let v = String::from_utf8_lossy(&out_.stdout);
            let trimmed = v.trim();
            doctor_pass("Rust toolchain", trimmed);
        }
        _ => {
            doctor_fail(
                "Rust toolchain",
                "not found",
                "install Rust from https://rustup.rs/",
            );
            failures += 1;
        }
    }

    // Are we in a project?
    let s = ProjectState::detect();
    if s.in_project {
        doctor_pass("Project structure", "Cargo.toml + main.rs + apps/ present");
    } else {
        doctor_fail(
            "Project structure",
            "no Cargo.toml / main.rs / apps/ here",
            "run `rustio init <name>` to scaffold, or `cd` into an existing project",
        );
        // Without a project the rest of the checks are moot.
        println!();
        println!("Stopped after the project-structure check — nothing else to verify.");
        return Ok(());
    }

    // Apps registered
    if s.has_apps {
        doctor_pass("Apps registered", "at least one app exists under apps/");
    } else {
        doctor_warn(
            "Apps registered",
            "no apps yet",
            "run `rustio new app <name>` to create your first model",
        );
        warnings += 1;
    }

    // Migrations directory
    if s.has_migrations_dir {
        doctor_pass("Migrations directory", "migrations/ exists");
    } else {
        doctor_warn(
            "Migrations directory",
            "no migrations/",
            "run `rustio new app <name>` (creates the directory) or add an empty one",
        );
        warnings += 1;
    }

    // Database
    if s.has_db {
        doctor_pass("Database file", "app.db exists");
    } else {
        doctor_warn(
            "Database file",
            "no app.db",
            "run `rustio migrate apply` to create it",
        );
        warnings += 1;
    }

    // Schema export
    if s.has_schema {
        doctor_pass("Schema export", "rustio.schema.json present");
    } else {
        doctor_warn(
            "Schema export",
            "no rustio.schema.json",
            "run `rustio schema` (the AI layer + external tools read it)",
        );
        warnings += 1;
    }

    // Summary
    println!();
    if failures == 0 && warnings == 0 {
        out::success("All checks pass", "you're good to go.");
        out::hint("rustio run                  start the server on :8000");
    } else if failures == 0 {
        out::info(&format!(
            "{} warning{} — your project still works, but the items above can be tightened up.",
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    } else {
        out::info(&format!(
            "{} failure{}, {} warning{} — fix the failures first.",
            failures,
            if failures == 1 { "" } else { "s" },
            warnings,
            if warnings == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

fn doctor_pass(name: &str, detail: &str) {
    println!("  {} {name}  {}", out::check(), out::dim(detail));
}

fn doctor_warn(name: &str, detail: &str, fix: &str) {
    println!("  {} {name}  {}", out::dot(), out::dim(detail));
    println!("      {} {fix}", out::dim("→"));
}

fn doctor_fail(name: &str, detail: &str, fix: &str) {
    println!("  {} {name}  {}", out::cross(), out::dim(detail));
    println!("      {} {fix}", out::dim("→"));
}

/// `rustio explain <topic>` — short inline mini-docs. Saves the new
/// dev from opening a browser to figure out what a "migration" or a
/// "schema" actually is. Topic content lives in [`EXPLAIN_TOPICS`].
fn explain_command(topic: &str) -> Result<(), String> {
    let normalized = topic.trim().to_lowercase();
    let entry = EXPLAIN_TOPICS
        .iter()
        .find(|(name, _)| *name == normalized.as_str());
    match entry {
        Some((_, body)) => {
            println!("{body}");
            Ok(())
        }
        None => {
            let known: Vec<&str> = EXPLAIN_TOPICS.iter().map(|(n, _)| *n).collect();
            Err(format!(
                "no explainer for `{topic}` — try one of: {}",
                known.join(", ")
            ))
        }
    }
}

const EXPLAIN_TOPICS: &[(&str, &str)] = &[
    (
        "model",
        "A model is a Rust struct that describes one \"thing\" in your project — a Note, a\n\
         Customer, an Order. The struct is the source of truth: RustIO derives the admin UI,\n\
         the database schema, and the JSON schema export from it.\n\
         \n\
         Example (apps/notes/models.rs):\n\
         \n\
         \x20\x20#[derive(RustioAdmin)]\n\
         \x20\x20pub struct Note {\n\
         \x20\x20    pub id: i64,\n\
         \x20\x20    pub title: String,\n\
         \x20\x20    pub body: String,\n\
         \x20\x20    pub created_at: DateTime<Utc>,\n\
         \x20\x20}\n\
         \n\
         Run `rustio new app <name>` to scaffold the struct + the matching migration in one\n\
         step. Then edit the struct to add the fields you actually want.",
    ),
    (
        "migration",
        "A migration is one `.sql` file that changes the database schema. Filenames are\n\
         numbered (0001_create_notes.sql, 0002_add_title_to_notes.sql) and RustIO applies\n\
         them in order, remembering which ones already ran.\n\
         \n\
         You can write them by hand, or let the AI layer generate them:\n\
         \n\
         \x20\x20rustio migrate generate alter_notes        # creates an empty file\n\
         \x20\x20$EDITOR migrations/000N_alter_notes.sql    # write the ALTER TABLE\n\
         \x20\x20rustio migrate apply                       # actually run it\n\
         \n\
         `rustio migrate status` shows which migrations are applied vs pending.",
    ),
    (
        "schema",
        "`rustio.schema.json` is a JSON file at your project root that lists every model,\n\
         every field, every type, and every relation. RustIO regenerates it on every\n\
         `rustio migrate apply` (or by hand with `rustio schema`).\n\
         \n\
         It's the **only** contract external tools (including the AI layer) are allowed to\n\
         use. Stable across patch releases — if you build something that reads it, your\n\
         tool keeps working across upgrades.",
    ),
    (
        "app",
        "An app is one folder inside `apps/` — usually one model + one matching admin\n\
         registration + one migration + a (probably empty) views file for public routes.\n\
         \n\
         You create one with:\n\
         \n\
         \x20\x20rustio new app notes\n\
         \n\
         That writes apps/notes/models.rs, apps/notes/admin.rs, apps/notes/views.rs, and\n\
         migrations/000N_create_notes.sql. The app is registered in apps/mod.rs\n\
         automatically.",
    ),
    (
        "admin",
        "The admin is the auto-generated web UI at /admin. RustIO renders it from your\n\
         model structs — every field becomes a form input, every model becomes a sidebar\n\
         entry, every row gets edit + delete buttons.\n\
         \n\
         Sign in: open http://127.0.0.1:8000/admin after starting the server. If you\n\
         haven't created a user yet:\n\
         \n\
         \x20\x20rustio user create --email you@example.com --password secret --role admin\n\
         \n\
         The admin has RBAC built in: SuperAdmin / Admin / Editor / Viewer roles, each\n\
         with per-model view/create/edit/delete permissions.",
    ),
    (
        "route",
        "A route is one URL path + HTTP method + handler function. RustIO registers admin\n\
         routes automatically (GET /admin, GET /admin/:model, etc.). You add your own\n\
         public routes inside `apps/<app>/views.rs`:\n\
         \n\
         \x20\x20pub fn register(router: Router) -> Router {\n\
         \x20\x20    router.get(\"/notes\", |_req, _params| async move {\n\
         \x20\x20        Ok::<Response, Error>(http::html(\"<h1>hello</h1>\"))\n\
         \x20\x20    })\n\
         \x20\x20}",
    ),
    (
        "ai",
        "The AI layer turns plain-English schema changes into typed file edits. Three\n\
         steps, each refusal-first:\n\
         \n\
         \x20\x201. rustio ai plan \"add email to notes\" --save plan.json\n\
         \x20\x202. rustio ai review plan.json     (risk / impact / warnings, no execution)\n\
         \x20\x203. rustio ai apply  plan.json     (writes models.rs + a migration)\n\
         \n\
         If the request can't be expressed inside the fixed primitive vocabulary\n\
         (AddField, RenameField, AddRelation, ChangeFieldType, etc.) the planner refuses\n\
         instead of guessing.",
    ),
    (
        "context",
        "`rustio.context.json` is a small file at the project root that carries country,\n\
         industry, and compliance flags (e.g. `{\"country\":\"SE\",\"industry\":\"healthcare\"}`).\n\
         \n\
         When present, the AI review layer picks up PII rules (personnummer is opaque,\n\
         patient_id must be a String, monetary fields are i64 minor units) and refuses\n\
         destructive operations on flagged fields. Optional — most projects don't need it.\n\
         \n\
         Inspect a context with:  rustio context show\n\
         Validate it with:        rustio context validate",
    ),
    (
        "rbac",
        "Role-Based Access Control. RustIO ships four roles (SuperAdmin / Admin / Editor /\n\
         Viewer) and per-model view / create / edit / delete permissions.\n\
         \n\
         Examples (read top-down — first match wins):\n\
         \x20\x20- A Viewer doesn't see the `+ Add` button anywhere; opening /admin/X/new\n\
         \x20\x20  returns the framework 403 page.\n\
         \x20\x20- An Editor can edit existing rows but not delete them.\n\
         \x20\x20- An Admin can do everything except manage roles.\n\
         \x20\x20- A SuperAdmin can do everything.\n\
         \n\
         Assign a role on user creation:  rustio user create --role editor",
    ),
];

/// `--why` blurbs — short "what does this command do" notes printed
/// when a user passes `--why` to any command. Keep each one ≤ 5 lines.
fn why_for(name: &str) {
    let body = match name {
        "default" => {
            "`rustio` with no args prints a context-aware suggestion for what to do next in\n\
             the current directory. It detects whether you're in a project, whether the DB\n\
             is set up, and whether models are registered, then prints the most useful\n\
             single next command.\n\
             \n\
             Run it without --why to actually see the suggestion."
        }
        "doctor" => {
            "`rustio doctor` runs a health check on the current project: Rust toolchain,\n\
             project structure, registered apps, migrations directory, database file, and\n\
             schema export. Each check prints pass / warn / fail + a fix hint. Never fails\n\
             the process even when checks warn — the goal is to surface fixes.\n\
             \n\
             Run it without --why to actually run the checks."
        }
        "explain" => {
            "`rustio explain <topic>` prints a short inline explanation of a framework\n\
             concept + a runnable example. Topics: model, migration, schema, app, admin,\n\
             route, ai, context, rbac.\n\
             \n\
             Run it without --why to actually read an explainer."
        }
        "init" => {
            "`rustio init <name>` scaffolds a new RustIO project: Cargo.toml, main.rs,\n\
             apps/mod.rs, migrations/, the standard auth tables. With no name it starts an\n\
             interactive wizard.\n\
             \n\
             Run it without --why to actually create the project."
        }
        "new-project" => {
            "`rustio new project <name>` creates a new project non-interactively. Same\n\
             result as `rustio init <name>` but never prompts.\n\
             \n\
             Run it without --why to create the project."
        }
        "new-app" => {
            "`rustio new app <name>` adds a new app inside the current project: a model\n\
             stub, an admin registration, an empty views file, and a matching migration.\n\
             Updates apps/mod.rs to register it. Each app is usually one model.\n\
             \n\
             Run it without --why to create the app."
        }
        "run" => {
            "`rustio run` is `cargo run` for your RustIO project: build the binary and\n\
             start the server on :8000. First run takes ~1 minute (downloads + compiles\n\
             dependencies). Subsequent runs are instant.\n\
             \n\
             Run it without --why to start the server."
        }
        "start" => {
            "`rustio start` opens the setup menu — guided wizard (recommended), manual\n\
             mode, or (soon) import an existing schema. It's the recommended first\n\
             command on a fresh project. The guided path asks one question, proposes\n\
             a starting shape, and walks each model with you; you decide what lands.\n\
             \n\
             Run it without --why to open the menu."
        }
        "migrate-generate" => {
            "`rustio migrate generate <name>` writes an empty SQL file under migrations/\n\
             with the next sequential number. You fill in the CREATE TABLE / ALTER TABLE,\n\
             then run `rustio migrate apply`.\n\
             \n\
             Run it without --why to create the file."
        }
        "migrate-apply" => {
            "`rustio migrate apply` runs every pending migration against your database in\n\
             filename order, inside a transaction per file. Already-applied migrations are\n\
             skipped (RustIO remembers them in a tracking table). After success, it\n\
             regenerates rustio.schema.json.\n\
             \n\
             Run it without --why to apply pending migrations."
        }
        "migrate-status" => {
            "`rustio migrate status` lists every migration file under migrations/ and shows\n\
             which ones are applied vs pending. Useful right before a deploy.\n\
             \n\
             Run it without --why to see the status."
        }
        "migrate-add-fks" => {
            "`rustio migrate add-fks` retrofits SQL FOREIGN KEY clauses onto an existing\n\
             0.8.x project. Default is dry-run; pass --write to actually commit the\n\
             generated migrations. Idempotent — running it on an already-retrofitted\n\
             project is a no-op.\n\
             \n\
             Run it without --why to see the preview."
        }
        "schema" => {
            "`rustio schema` regenerates rustio.schema.json from your compiled admin. It's\n\
             the only file external tools (including the AI layer) are allowed to read, so\n\
             keep it in sync with your code.\n\
             \n\
             Run it without --why to regenerate the file."
        }
        "view" => {
            "`rustio view <model>` renders a model's view to the terminal: it derives a\n\
             default ViewSpec from rustio.schema.json (or loads <model>.view.json when\n\
             you've saved one), then prints demo rows in the chosen layout. Read-only\n\
             unless you pass --save. Layouts: table, list, cards, compact. --json dumps\n\
             the structured RenderedView for scripting.\n\
             \n\
             Run it without --why to render the view."
        }
        "ai" => {
            "`rustio ai <plan|review|apply>` is the scripting / CI surface for the\n\
             typed change pipeline. Three steps:\n\
             \x20\x201. plan — parse plain English into a typed change document.\n\
             \x20\x202. review — risk / impact / warnings, no execution.\n\
             \x20\x203. apply — atomic file writes; never runs migrations itself.\n\
             \n\
             For an interactive flow, `rustio evolve \"<request>\"` is friendlier.\n\
             Run a subcommand without --why for actual usage."
        }
        "evolve" => {
            "`rustio evolve \"<request>\"` is the friendly verb for changing your\n\
             schema after the project is up. Describe the change in plain English;\n\
             RustIO proposes the diff, shows you the risk, and applies only what\n\
             you accept. Same three-way choice the setup wizard uses:\n\
             Apply / Show technical details / Cancel.\n\
             \n\
             Run it without --why to actually start a change."
        }
        "context" => {
            "`rustio context <show|validate>` inspects rustio.context.json — the optional\n\
             country / industry / compliance file that drives PII detection and policy\n\
             refusals in the AI layer.\n\
             \n\
             Run a subcommand without --why to actually inspect or validate."
        }
        "user-create" => {
            "`rustio user create` adds a row to rustio_users with an argon2-hashed\n\
             password and a role (SuperAdmin / Admin / Editor / Viewer). Without args,\n\
             prompts interactively for email, password, and role.\n\
             \n\
             Run it without --why to actually create the user."
        }
        "version" => {
            "Prints the CLI's version. RustIO follows semver; the CLI and rustio-core\n\
             ship in lockstep until 1.0.0."
        }
        _ => "No explanation available for this command.",
    };
    println!("{body}");
}

fn why_for_help() {
    println!(
        "`rustio help` prints the full command list, grouped by purpose. For a one-line\n\
         explanation of any individual command, append --why (e.g. `rustio migrate apply\n\
         --why`)."
    );
}

// ---------------------------------------------------------------------------
// Output helpers.
// ---------------------------------------------------------------------------

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

    pub fn cross() -> String {
        colored("✗", "31")
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

/// Brand identity. Drives the admin (sidebar/login wordmark + accent) and the
/// public landing page at `/` (brand + the project name shown in its terminal).
/// Only these keys are allowed — unknown keys are rejected, so the page falls
/// back to defaults rather than half-applying.
const DESIGN_JSON: &str = r##"{
  "project_name": "{{NAME}}",
  "logo_initial": "{{INITIAL}}",
  "primary_color": "#2B54E0",
  "accent_color": "#2B54E0"
}
"##;

/// Admin UI translations. Swedish ships built-in; this file overrides or
/// extends it and is where you add new languages. The `_comment` key is
/// ignored, so it's safe to keep as inline documentation.
const LOCALE_JSON: &str = r#"{
  "_comment": "Translate the admin UI. Keys are the exact English text shown in the admin; values are the translation for that language code. Swedish (sv) ships built-in — entries here override or extend it. Add any language code (de, fr, ar, ...); right-to-left languages (ar, fa, ur) mirror the layout automatically. Record DATA is never translated here — field and value labels live in the admin's view editor.",
  "sv": {
    "Recent actions": "Senaste händelser"
  }
}
"#;

/// A plain-English developer guide written into every new project.
const DEVELOPMENT_MD: &str = r##"# Developing {{NAME}}

A short, practical guide to everything you can change — no deep framework
knowledge required.

## Run it

    rustio migrate apply                 # apply schema changes to the database
    rustio user create --email you@example.com --password secret --role admin
    rustio run                           # build + serve on http://127.0.0.1:8000

Then open <http://127.0.0.1:8000> for the landing page, and
<http://127.0.0.1:8000/admin> to sign in.

## Project layout

- `main.rs` — entry point (mostly boilerplate; add your own routes here)
- `apps/<name>/` — one folder per model: `models.rs` (the struct = source of
  truth), `admin.rs`, `views.rs`
- `migrations/` — SQL files, applied in filename order
- `templates/`, `static/` — your public assets (RustIO stays out of these)
- `rustio.design.json`, `rustio.locale.json` — the two config files below
- `app.db` — SQLite database (gitignored)

## Add a model

    rustio new app customers             # scaffolds apps/customers/ + a migration
    # edit apps/customers/models.rs to add fields, then:
    rustio migrate apply

Or describe the change in plain English and let RustIO write the diff:

    rustio evolve "add email and date_of_birth to customers"

## Branding — `rustio.design.json`

Change how the admin and landing page look without touching any code:

    {
      "project_name": "{{NAME}}",   // shown in the sidebar, title, and landing page
      "logo_initial": "{{NAME}}"[0],// the single letter in the square logo
      "primary_color": "#2B54E0",   // primary button + logo background
      "accent_color":  "#2B54E0"    // focus rings + links
    }

## Languages — `rustio.locale.json`

The admin UI translates itself. **Swedish ships built-in.** To add or change a
translation, edit `rustio.locale.json`: the key is the exact English text, the
value is your translation. Add any language by adding its code:

    { "de": { "Add": "Hinzufügen", "Save": "Speichern" } }

- A language you add becomes selectable from the switcher in the admin top bar.
- Right-to-left languages (`ar`, `fa`, `ur`, …) mirror the whole layout
  automatically (`dir="rtl"`).
- Missing translations fall back to English — never blank.
- Record **data** is never translated here; per-field and per-value labels live
  in the admin's **Edit view** (composition editor).

## The home page (`/`)

The landing page is served for you and already shows your `project_name`. It is
a **developer page — replace it before production.** Two ways:

1. **Rebrand instantly:** just edit `rustio.design.json` (above).
2. **Full control:** create `templates/home.html` — it replaces the built-in
   page. You may use `__PROJECT_NAME__`, `__PROJECT_INITIAL__`,
   `__PROJECT_SLUG__`, and `__RUSTIO_VERSION__` placeholders.

## The admin (`/admin`)

Framework-owned. Every model you register gets list/create/edit/delete screens,
search, filters, foreign-key links, RBAC, and an audit log — generated from
your structs. You don't build it; you just register models.

## Going further

- Docs: <https://docs.rs/rustio-core>
- Run `rustio` with no arguments for a context-aware "what next" hint, or
  `rustio doctor` to health-check the project.
"##;

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
    fn parse_no_args_is_default_action() {
        // `rustio` with no args routes to the context-aware default
        // helper, NOT the full --help dump. Explicit `help` still maps
        // to Command::Help (covered by `parse_help_flag`).
        assert_eq!(parse_command(&args(&[])).unwrap(), Command::Default);
    }

    #[test]
    fn parse_doctor_command() {
        assert_eq!(parse_command(&args(&["doctor"])).unwrap(), Command::Doctor);
    }

    // -- `rustio view` ------------------------------------------------------

    fn view_customer_model() -> rustio_core::schema::SchemaModel {
        use rustio_core::schema::{SchemaField, SchemaModel};
        let f = |name: &str, ty: &str| SchemaField {
            name: name.to_string(),
            ty: ty.to_string(),
            nullable: false,
            editable: true,
            relation: None,
        };
        SchemaModel {
            name: "Customer".into(),
            table: "customers".into(),
            admin_name: "customers".into(),
            display_name: "Customers".into(),
            singular_name: "Customer".into(),
            fields: vec![
                f("id", "i64"),
                f("name", "String"),
                f("email", "String"),
                f("status", "String"),
                f("created_at", "DateTime"),
                f("password_hash", "String"),
                f("notes", "String"),
            ],
            relations: vec![],
            core: false,
        }
    }

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn parse_view_command() {
        use rustio_core::viewspec::ViewLayout;
        assert_eq!(
            parse_command(&args(&["view", "Customer"])).unwrap(),
            Command::View {
                model: "Customer".into(),
                layout: None,
                save: false,
                from: None,
                json: false,
            }
        );
        assert_eq!(
            parse_command(&args(&[
                "view", "Customer", "--layout", "table", "--save", "--from", "x.json", "--json",
            ]))
            .unwrap(),
            Command::View {
                model: "Customer".into(),
                layout: Some(ViewLayout::Table),
                save: true,
                from: Some("x.json".into()),
                json: true,
            }
        );
        assert!(parse_command(&args(&["view"])).is_err());
        assert!(parse_command(&args(&["view", "Customer", "--layout", "bogus"])).is_err());
        assert!(parse_command(&args(&["view", "Customer", "--bogus"])).is_err());
    }

    #[test]
    fn render_includes_title_value_and_omits_hidden() {
        use rustio_core::viewspec::render::RenderedView;
        use rustio_core::viewspec::{ViewLayout, ViewSpec};

        let model = view_customer_model();
        let spec = ViewSpec::from_schema_model(&model);
        let rows = synth_demo_rows(&model);
        // Check every layout: a Hidden field must never surface anywhere.
        for layout in [
            ViewLayout::Table,
            ViewLayout::List,
            ViewLayout::Cards,
            ViewLayout::Compact,
        ] {
            let view = RenderedView::render_with_layout(&spec, layout, &rows);
            let text = render_terminal(&view);
            // `name` is the Title; its demo value must appear.
            assert!(
                text.contains("sample name 1"),
                "title demo value missing in {layout:?}:\n{text}"
            );
            // `password_hash` is Hidden — neither its name, its label, nor
            // its demo value may appear.
            assert!(
                !text.contains("password_hash"),
                "hidden field name leaked in {layout:?}:\n{text}"
            );
            assert!(
                !text.contains("Password Hash"),
                "hidden field label leaked in {layout:?}:\n{text}"
            );
            assert!(
                !text.contains("sample password_hash"),
                "hidden field value leaked in {layout:?}:\n{text}"
            );
        }
    }

    #[test]
    fn save_writes_then_refuses_overwrite() {
        let dir = unique_temp_dir("rustio-view-save");
        std::fs::create_dir_all(&dir).unwrap();
        let model = view_customer_model();

        // First --save writes the derived default.
        let (_, src) = resolve_view_spec(&dir, &model, true).unwrap();
        assert_eq!(src, ViewSource::Wrote("customer.view.json".into()));
        assert!(dir.join("customer.view.json").exists());

        // Second --save refuses, and the error names the next step.
        let err = resolve_view_spec(&dir, &model, true).unwrap_err();
        assert!(err.contains("already exists"), "got: {err}");
        assert!(
            err.contains("re-run"),
            "overwrite error must suggest a next step, got: {err}"
        );

        // Without --save, the saved file is now loaded as source of truth.
        let (_, src2) = resolve_view_spec(&dir, &model, false).unwrap();
        assert_eq!(src2, ViewSource::SavedLoaded("customer.view.json".into()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_saved_file_uses_derived_default() {
        let dir = unique_temp_dir("rustio-view-derived");
        std::fs::create_dir_all(&dir).unwrap();
        let model = view_customer_model();
        let (_, src) = resolve_view_spec(&dir, &model, false).unwrap();
        assert_eq!(src, ViewSource::Derived);
        assert!(!dir.join("customer.view.json").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn render_terminal_is_deterministic() {
        use rustio_core::viewspec::render::RenderedView;
        use rustio_core::viewspec::{ViewLayout, ViewSpec};

        let model = view_customer_model();
        let spec = ViewSpec::from_schema_model(&model);
        let rows = synth_demo_rows(&model);
        let a = render_terminal(&RenderedView::render_with_layout(
            &spec,
            ViewLayout::Cards,
            &rows,
        ));
        let b = render_terminal(&RenderedView::render_with_layout(
            &spec,
            ViewLayout::Cards,
            &rows,
        ));
        assert_eq!(a, b);
    }

    #[test]
    fn to_snake_case_handles_camel() {
        assert_eq!(to_snake_case("Customer"), "customer");
        assert_eq!(to_snake_case("BlogPost"), "blog_post");
    }

    #[test]
    fn parse_explain_requires_topic() {
        assert!(parse_command(&args(&["explain"])).is_err());
        assert_eq!(
            parse_command(&args(&["explain", "migration"])).unwrap(),
            Command::Explain("migration".to_string())
        );
    }

    #[test]
    fn strip_why_flag_pulls_it_anywhere() {
        let (rest, why) = strip_why_flag(vec![
            "rustio".into(),
            "migrate".into(),
            "apply".into(),
            "--why".into(),
        ]);
        assert!(why);
        assert_eq!(rest, vec!["rustio", "migrate", "apply"]);

        let (rest2, why2) = strip_why_flag(vec!["rustio".into(), "doctor".into()]);
        assert!(!why2);
        assert_eq!(rest2, vec!["rustio", "doctor"]);
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

    // 0.9.1 — `ai apply --force` argument parsing.

    #[test]
    fn ai_apply_parses_force_flag() {
        match parse_command(&args(&["ai", "apply", "plan.json", "--force"])).unwrap() {
            Command::Ai(AiCommand::Apply { force, .. }) => assert!(force),
            other => panic!("expected AiCommand::Apply, got {other:?}"),
        }
    }

    #[test]
    fn ai_apply_force_defaults_to_false() {
        match parse_command(&args(&["ai", "apply", "plan.json"])).unwrap() {
            Command::Ai(AiCommand::Apply { force, .. }) => assert!(!force),
            other => panic!("expected AiCommand::Apply, got {other:?}"),
        }
    }

    #[test]
    fn ai_apply_force_composes_with_yes_and_dry_run() {
        match parse_command(&args(&[
            "ai",
            "apply",
            "plan.json",
            "--yes",
            "--dry-run",
            "--force",
        ]))
        .unwrap()
        {
            Command::Ai(AiCommand::Apply {
                assume_yes,
                dry_run,
                force,
                ..
            }) => {
                assert!(assume_yes);
                assert!(dry_run);
                assert!(force);
            }
            other => panic!("expected AiCommand::Apply, got {other:?}"),
        }
    }
}
