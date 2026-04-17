//! Interactive `rustio init` wizard.
//!
//! ## Why `inquire`?
//!
//! `inquire` is a modern, focused Rust CLI interaction library with clean
//! defaults, unicode/color handling that respects `NO_COLOR`, and a small
//! API. `dialoguer` is the other common option but its component feel is
//! older; for a short linear wizard (name → preset → confirm) `inquire`
//! produces less code and a nicer look out of the box.
//!
//! ## Design principles
//!
//! 1. **Three prompts, no more.** Name, preset, confirm. Every extra prompt
//!    is friction.
//! 2. **Smart defaults.** Enter always accepts. The default project name
//!    is `mysite`; the default preset is `Basic`.
//! 3. **No fake choices.** We only ask about things that actually exist.
//!    RustIO only supports SQLite today, so there is no database prompt.
//!    There is no "enable auth" prompt either — auth is always included;
//!    asking would imply it's optional.
//! 4. **Same validation as flags.** The wizard reuses [`crate::validate_name`]
//!    so project names rejected from the command line are rejected in the
//!    wizard too.
//! 5. **Non-interactive stays primary.** Anything the wizard does must be
//!    reachable from the command line (`rustio init <name> --preset blog`).
//!    The wizard is a surface for the same `Plan`, not a separate path.
//! 6. **Fail fast off-TTY.** If stdin is not a terminal, the wizard cannot
//!    run; we explain this instead of hanging or producing garbage.

use std::io::IsTerminal;
use std::str::FromStr;

use inquire::validator::Validation;
use inquire::{Confirm, InquireError, Select, Text};

use crate::out;

/// A fully-resolved project scaffold plan produced by either the wizard
/// or the non-interactive argument parser.
///
/// `app_name` controls the single app scaffolded under the chosen preset.
/// When `None` the preset's default app is used (see [`Preset::apps`]).
/// `Preset::Basic` ignores `app_name` entirely (no apps are scaffolded).
#[derive(Debug, Clone)]
pub struct Plan {
    pub project_name: String,
    pub preset: Preset,
    pub app_name: Option<String>,
}

impl Plan {
    /// The apps that should be scaffolded for this plan. Honors
    /// `app_name` if set, otherwise falls back to the preset defaults.
    pub fn apps(&self) -> Vec<String> {
        match (&self.app_name, self.preset) {
            (_, Preset::Basic) => Vec::new(),
            (Some(custom), _) => vec![custom.clone()],
            (None, preset) => preset.apps().iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Starter templates. Each preset maps to zero or more apps to scaffold.
///
/// Keeping presets coarse — three choices, one line each. More presets
/// become a catalogue; fewer presets become a non-decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Empty project. Add apps later with `rustio new app <name>`.
    Basic,
    /// Project + a `posts` app: admin CRUD and a placeholder view.
    Blog,
    /// Project + an `items` app: admin CRUD and a placeholder view.
    Api,
}

impl Preset {
    /// Short, human-facing label shown in the picker.
    pub fn label(self) -> &'static str {
        match self {
            Preset::Basic => "Basic — empty project, add apps later",
            Preset::Blog => "Blog — scaffolds a posts app with admin + views",
            Preset::Api => "API — scaffolds an items app with admin + views",
        }
    }

    /// Apps that should be scaffolded for this preset.
    pub fn apps(self) -> &'static [&'static str] {
        match self {
            Preset::Basic => &[],
            Preset::Blog => &["posts"],
            Preset::Api => &["items"],
        }
    }
}

impl FromStr for Preset {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-insensitive on purpose: users sometimes type `Blog` after
        // seeing the label.
        match s.to_ascii_lowercase().as_str() {
            "basic" => Ok(Preset::Basic),
            "blog" => Ok(Preset::Blog),
            "api" => Ok(Preset::Api),
            other => Err(format!(
                "unknown preset `{other}` — expected one of: basic, blog, api"
            )),
        }
    }
}

/// Run the interactive wizard and return the chosen plan.
///
/// `default_preset` seeds the preset picker's highlight and
/// `default_app_name` seeds the app-name prompt. Both are otherwise only
/// used if the user chooses Enter-to-accept.
pub fn run(
    default_preset: Option<Preset>,
    default_app_name: Option<String>,
) -> Result<Plan, String> {
    // Prompt libraries need a real terminal to draw on. In CI or when
    // stdin is piped from another program, the wizard cannot function —
    // direct the user at the non-interactive form instead of hanging.
    if !std::io::stdin().is_terminal() {
        return Err(
            "`rustio init` without a name needs an interactive terminal.\n \
             Try: rustio init <name> [--preset basic|blog|api] [--app <name>]"
                .into(),
        );
    }

    banner();

    let project_name = prompt_name()?;
    let preset = prompt_preset(default_preset)?;

    // Only ask for an app name when the preset actually scaffolds one.
    // Basic has no app, so an app-name prompt would be a dead question.
    let app_name = if preset == Preset::Basic {
        None
    } else {
        Some(prompt_app_name(preset, default_app_name)?)
    };

    let plan = Plan {
        project_name,
        preset,
        app_name,
    };
    print_summary(&plan);

    if !confirm_proceed()? {
        return Err("cancelled".into());
    }
    // Blank line between the confirm prompt and the scaffolding output
    // so the sections read as distinct.
    println!();

    Ok(plan)
}

/// Execute a plan: create the project, `cd` into it, scaffold any apps
/// the preset requested, and print a single consolidated next-steps hint.
///
/// Reuses [`crate::new_project`] and [`crate::new_app`] verbatim so the
/// wizard and non-interactive paths produce byte-identical output on disk.
pub fn execute(plan: &Plan) -> Result<(), String> {
    // Step 1: create the project directory and its files.
    crate::new_project(&plan.project_name)?;

    // Step 2: scaffold the chosen app(s) inside the new project.
    //
    // `new_app` looks at the current working directory (specifically for
    // `apps/mod.rs`), so we have to chdir into the generated project. This
    // only affects the running CLI process — the user's shell is unchanged.
    let apps = plan.apps();
    if !apps.is_empty() {
        std::env::set_current_dir(&plan.project_name)
            .map_err(|e| format!("failed to enter `{}`: {e}", plan.project_name))?;
        for app in &apps {
            crate::new_app(app)?;
        }
    }

    // Step 3: consolidated next-steps.
    println!();
    println!("{}", out::bold("Next:"));
    if apps.is_empty() {
        out::hint(&format!("cd {}", plan.project_name));
        out::hint("rustio new app <name>");
        out::hint("rustio run");
    } else {
        out::hint(&format!("cd {}", plan.project_name));
        out::hint("rustio migrate apply");
        out::hint("rustio run");
    }
    Ok(())
}

fn banner() {
    // Kept deliberately small. Framework CLIs tend to over-welcome.
    println!();
    println!("  {}", out::bold("RustIO"));
    println!("  Let's set up your project.");
    println!();
}

fn prompt_name() -> Result<String, String> {
    // `mysite` matches the README's quick-start example.
    Text::new("Project name:")
        .with_default("mysite")
        .with_help_message("lowercase letters, digits, and underscores")
        .with_validator(name_validator)
        .prompt()
        .map_err(translate_prompt_error)
}

fn prompt_app_name(preset: Preset, default: Option<String>) -> Result<String, String> {
    // Seed the prompt with a preset-appropriate default so Enter always
    // yields a sensible value. The user can type their own to customize
    // the struct name, table, and `/admin/<table>` URL in one shot.
    let fallback = match preset {
        Preset::Blog => "posts",
        Preset::Api => "items",
        Preset::Basic => "posts", // unreachable in practice — caller skips
    };
    let seed = default.unwrap_or_else(|| fallback.to_string());
    Text::new("What should your first model track?")
        .with_default(&seed)
        .with_help_message("used as the struct / table / admin URL — e.g. books, tasks, links")
        .with_validator(name_validator)
        .prompt()
        .map_err(translate_prompt_error)
}

fn prompt_preset(default: Option<Preset>) -> Result<Preset, String> {
    // Three options, one line each. Order Basic → Blog → API so the
    // safest default sits at the top of the list.
    let options = [Preset::Basic, Preset::Blog, Preset::Api];
    let labels: Vec<&'static str> = options.iter().map(|p| p.label()).collect();

    let starting = default
        .and_then(|d| options.iter().position(|p| *p == d))
        .unwrap_or(0);

    let picked = Select::new("Choose a starting preset:", labels)
        .with_starting_cursor(starting)
        .with_help_message("↑/↓ to move, Enter to select")
        .prompt()
        .map_err(translate_prompt_error)?;

    // Map the label back to a Preset. We look up by identity on the
    // `&'static str` returned by `label()` rather than string-matching a
    // copy, which also makes the mapping total.
    options
        .iter()
        .copied()
        .find(|p| p.label() == picked)
        .ok_or_else(|| "internal: unrecognised preset label".to_string())
}

fn print_summary(plan: &Plan) {
    // A quiet summary instead of a full box-drawing preview — easy to
    // skim at any terminal width, and keeps focus on the confirm prompt.
    println!();
    println!("  {}", out::bold("Ready to generate:"));
    println!("    {:<9} {}", out::dim("name"), plan.project_name);
    println!("    {:<9} {}", out::dim("preset"), plan.preset.label());
    for app in plan.apps() {
        println!("    {:<9} {app}", out::dim("app"));
    }
    println!();
}

fn confirm_proceed() -> Result<bool, String> {
    Confirm::new("Proceed?")
        .with_default(true)
        .prompt()
        .map_err(translate_prompt_error)
}

/// Reuse the non-interactive project-name validator so both entry points
/// enforce the same rules. `inquire` expects `Result<Validation, _>`.
fn name_validator(input: &str) -> Result<Validation, Box<dyn std::error::Error + Send + Sync>> {
    match crate::validate_name(input) {
        Ok(()) => Ok(Validation::Valid),
        Err(msg) => Ok(Validation::Invalid(msg.into())),
    }
}

/// Map `InquireError` to the `String` contract used by the rest of the CLI.
/// Ctrl-C / ESC collapse to a short "cancelled" so the main error printer
/// doesn't show a scary stack.
fn translate_prompt_error(e: InquireError) -> String {
    match e {
        InquireError::OperationCanceled | InquireError::OperationInterrupted => {
            "cancelled".to_string()
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_from_str_accepts_known_names() {
        assert_eq!("basic".parse::<Preset>().unwrap(), Preset::Basic);
        assert_eq!("blog".parse::<Preset>().unwrap(), Preset::Blog);
        assert_eq!("api".parse::<Preset>().unwrap(), Preset::Api);
    }

    #[test]
    fn preset_from_str_is_case_insensitive() {
        assert_eq!("BLOG".parse::<Preset>().unwrap(), Preset::Blog);
        assert_eq!("Basic".parse::<Preset>().unwrap(), Preset::Basic);
    }

    #[test]
    fn preset_from_str_rejects_unknown() {
        let err = "nope".parse::<Preset>().unwrap_err();
        assert!(err.contains("nope"));
        assert!(err.contains("basic"));
    }

    #[test]
    fn preset_apps_match_labels() {
        assert!(Preset::Basic.apps().is_empty());
        assert_eq!(Preset::Blog.apps(), &["posts"]);
        assert_eq!(Preset::Api.apps(), &["items"]);
    }

    #[test]
    fn preset_labels_are_unique() {
        // We rely on labels being unique in `prompt_preset` to map a
        // picked label back to a Preset. Guard that invariant here.
        let labels = [
            Preset::Basic.label(),
            Preset::Blog.label(),
            Preset::Api.label(),
        ];
        let mut sorted = labels.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), labels.len());
    }

    #[test]
    fn plan_apps_uses_override_when_present() {
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Blog,
            app_name: Some("books".into()),
        };
        assert_eq!(plan.apps(), vec!["books".to_string()]);
    }

    #[test]
    fn plan_apps_falls_back_to_preset_default() {
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Blog,
            app_name: None,
        };
        assert_eq!(plan.apps(), vec!["posts".to_string()]);

        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Api,
            app_name: None,
        };
        assert_eq!(plan.apps(), vec!["items".to_string()]);
    }

    #[test]
    fn plan_apps_basic_is_empty_even_with_app_override() {
        // Basic explicitly means "no app" — even if the caller sets an
        // app_name, we honor the preset's intent.
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Basic,
            app_name: Some("ignored".into()),
        };
        assert!(plan.apps().is_empty());
    }
}
