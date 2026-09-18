//! Interactive `rustio init` wizard.
//!
//! ## Why `inquire`?
//!
//! `inquire` is a modern, focused Rust CLI interaction library with clean
//! defaults, unicode/color handling that respects `NO_COLOR`, and a small
//! API. `dialoguer` is the other common option but its component feel is
//! older; for a one-question wizard (the project name) `inquire`
//! produces less code and a nicer look out of the box.
//!
//! ## Design principles
//!
//! 1. **One prompt.** The project name, and nothing else. What goes
//!    *inside* the project is the setup menu's question (Empty or a
//!    template), asked once, right after the scaffold lands.
//! 2. **Smart defaults.** Enter always accepts. The default project name
//!    is `mysite`. `init` always creates an empty project.
//! 3. **No fake choices.** We only ask about things that actually exist.
//!    RustIO only supports SQLite today, so there is no database prompt.
//!    There is no "enable auth" prompt either — auth is always included;
//!    asking would imply it's optional.
//! 4. **Same validation as flags.** The wizard reuses [`crate::validate_name`]
//!    so project names rejected from the command line are rejected in the
//!    wizard too.
//! 5. **Non-interactive stays primary.** Anything the wizard does must be
//!    reachable from the command line (`rustio init <name> --model books`).
//!    The wizard is a surface for the same `Plan`, not a separate path.
//! 6. **Fail fast off-TTY.** If stdin is not a terminal, the wizard cannot
//!    run; we explain this instead of hanging or producing garbage.

use std::io::IsTerminal;

use inquire::validator::Validation;
use inquire::{InquireError, Text};

use crate::out;

/// A fully-resolved project scaffold plan produced by either the wizard
/// or the non-interactive argument parser.
///
/// `model_name` is the single model to scaffold, from `--model`.
/// `None` — the default — creates an empty project.
#[derive(Debug, Clone)]
pub struct Plan {
    pub project_name: String,
    /// The single model to scaffold, from `--model`. `None` — the
    /// default — creates an empty project.
    pub model_name: Option<String>,
}

impl Plan {
    /// The models that should be scaffolded for this plan. Honors
    pub fn models(&self) -> Vec<String> {
        self.model_name.iter().cloned().collect()
    }
}

/// Run the interactive wizard and return the chosen plan.
///
/// `default_model_name` comes from `--model`; the wizard itself asks
/// only for the project name.
pub fn run(default_model_name: Option<String>) -> Result<Plan, String> {
    // Prompt libraries need a real terminal to draw on. In CI or when
    // stdin is piped from another program, the wizard cannot function —
    // direct the user at the non-interactive form instead of hanging.
    if !std::io::stdin().is_terminal() {
        return Err(
            "`rustio init` without a name needs an interactive terminal.\n \
             Try: rustio init <name> [--model <name>]"
                .into(),
        );
    }

    banner();

    // One question: the name. `init` creates an empty project, so
    // there is nothing else to ask.
    let project_name = prompt_name()?;

    println!();

    Ok(Plan {
        project_name,
        model_name: default_model_name,
    })
}

/// Execute a plan: create the project, `cd` into it, scaffold any models
/// the model `--model` named, if any.
///
/// Reuses [`crate::new_project`] and [`crate::add_model`] verbatim so the
/// wizard and non-interactive paths produce byte-identical output on disk.
pub fn execute(plan: &Plan) -> Result<(), String> {
    // Step 1: create the project directory and its files.
    crate::new_project(&plan.project_name)?;

    // Step 2: scaffold the model `--model` named, if any.
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
mod tests {}
