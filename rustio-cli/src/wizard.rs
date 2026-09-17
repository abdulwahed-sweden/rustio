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
//! 1. **One prompt.** The project name, and nothing else. What goes
//!    *inside* the project is the setup menu's question (Empty or a
//!    template), asked once, right after the scaffold lands.
//! 2. **Smart defaults.** Enter always accepts. The default project name
//!    is `mysite`; the default preset is `Basic` (empty).
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
use inquire::{InquireError, Text};

use crate::out;

/// A fully-resolved project scaffold plan produced by either the wizard
/// or the non-interactive argument parser.
///
/// `model_name` controls the single model scaffolded under the chosen
/// preset. When `None` the preset's default is used (see
/// [`Preset::models`]). `Preset::Basic` ignores it entirely (it
/// scaffolds no models).
#[derive(Debug, Clone)]
pub struct Plan {
    pub project_name: String,
    pub preset: Preset,
    pub model_name: Option<String>,
}

impl Plan {
    /// The models that should be scaffolded for this plan. Honors
    /// `model_name` if set, otherwise falls back to the preset defaults.
    pub fn models(&self) -> Vec<String> {
        match (&self.model_name, self.preset) {
            (_, Preset::Basic) => Vec::new(),
            (Some(custom), _) => vec![custom.clone()],
            (None, preset) => preset.models().iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Starter templates. Each preset maps to zero or more models to scaffold.
///
/// Keeping presets coarse — three choices, one line each. More presets
/// become a catalogue; fewer presets become a non-decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Empty project. Add models later with `rustio add model <name>`.
    Basic,
    /// Project + a `posts` model: admin CRUD and a placeholder view.
    Blog,
    /// Project + an `items` model: admin CRUD and a placeholder view.
    Api,
}

impl Preset {
    /// Short, human-facing label for the preset. Used by the tests
    /// that pin each preset to the models it scaffolds; kept as the one
    /// place a preset describes itself in prose.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn label(self) -> &'static str {
        match self {
            Preset::Basic => "Basic — empty project, add models later",
            Preset::Blog => "Blog — scaffolds a posts model with admin + views",
            Preset::Api => "API — scaffolds an items model with admin + views",
        }
    }

    /// Models that should be scaffolded for this preset.
    pub fn models(self) -> &'static [&'static str] {
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
/// `default_model_name` names the model a non-Basic preset scaffolds.
/// Both come from flags; the wizard itself asks only for the project
/// name.
pub fn run(
    default_preset: Option<Preset>,
    default_model_name: Option<String>,
) -> Result<Plan, String> {
    // Prompt libraries need a real terminal to draw on. In CI or when
    // stdin is piped from another program, the wizard cannot function —
    // direct the user at the non-interactive form instead of hanging.
    if !std::io::stdin().is_terminal() {
        return Err(
            "`rustio init` without a name needs an interactive terminal.\n \
             Try: rustio init <name> [--preset basic|blog|api] [--model <name>]"
                .into(),
        );
    }

    banner();

    // One question: the name. What to put *in* the project is the next
    // screen's job — `init` ends on the setup menu (Empty / Template),
    // and asking "which preset?" here would ask the same thing twice in
    // two vocabularies.
    //
    // `--preset` / `--model` still work alongside a nameless `init`: a
    // flag is an explicit answer, so we take it and skip the menu's
    // version of the question.
    let project_name = prompt_name()?;
    let preset = default_preset.unwrap_or(Preset::Basic);
    let model_name = if preset == Preset::Basic {
        None
    } else {
        default_model_name
    };

    println!();

    Ok(Plan {
        project_name,
        preset,
        model_name,
    })
}

/// Execute a plan: create the project, `cd` into it, scaffold any models
/// the preset requested, and print a single consolidated next-steps hint.
///
/// Reuses [`crate::new_project`] and [`crate::add_model`] verbatim so the
/// wizard and non-interactive paths produce byte-identical output on disk.
pub fn execute(plan: &Plan) -> Result<(), String> {
    // Step 1: create the project directory and its files.
    crate::new_project(&plan.project_name)?;

    // Step 2: scaffold the preset's model(s) inside the new project.
    //
    // `add_model` looks at the current working directory (specifically
    // for `models/mod.rs`), so we have to chdir into the generated
    // project. This only affects the running CLI process — the user's
    // shell is unchanged.
    let models = plan.models();
    if !models.is_empty() {
        std::env::set_current_dir(&plan.project_name)
            .map_err(|e| format!("failed to enter `{}`: {e}", plan.project_name))?;
        for model in &models {
            crate::add_model(model)?;
        }
    }

    // No "Next:" block here. `init_command` owns the closing screen —
    // either the setup menu (which ends on one) or, off a terminal,
    // the same block printed directly. Printing it here too would
    // show it twice, with different advice each time.
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
    fn preset_models_match_labels() {
        assert!(Preset::Basic.models().is_empty());
        assert_eq!(Preset::Blog.models(), &["posts"]);
        assert_eq!(Preset::Api.models(), &["items"]);
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
    fn plan_models_uses_override_when_present() {
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Blog,
            model_name: Some("books".into()),
        };
        assert_eq!(plan.models(), vec!["books".to_string()]);
    }

    #[test]
    fn plan_models_falls_back_to_preset_default() {
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Blog,
            model_name: None,
        };
        assert_eq!(plan.models(), vec!["posts".to_string()]);

        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Api,
            model_name: None,
        };
        assert_eq!(plan.models(), vec!["items".to_string()]);
    }

    #[test]
    fn plan_models_basic_is_empty_even_with_override() {
        // Basic explicitly means "no models" — even if the caller sets an
        // model_name, we honor the preset's intent.
        let plan = Plan {
            project_name: "x".into(),
            preset: Preset::Basic,
            model_name: Some("ignored".into()),
        };
        assert!(plan.models().is_empty());
    }
}
