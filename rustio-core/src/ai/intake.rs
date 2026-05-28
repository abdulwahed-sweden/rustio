//! Free-text → typed `ProjectSketch` — the entry point of the
//! AI-assisted onboarding flow (`rustio ai start`).
//!
//! ## Why a separate module
//!
//! The existing [`planner`](super::planner) takes a *single change*
//! and emits a `Plan` of primitives. Intake operates one layer up: it
//! takes the user's one-sentence project description and proposes a
//! *starting shape* — two to four models with sensible default fields,
//! built from a curated set of domain templates.
//!
//! ## The hard rule
//!
//! Intake is **deterministic** and **closed**. There is no LLM call,
//! no fuzzy guessing, no free-form generation. Each domain template is
//! a hard-coded `Vec<ModelSketch>`; a description that doesn't match
//! any keyword set returns `None` (the wizard then asks the user to
//! re-phrase or drop down to single-model mode).
//!
//! This matches the wider AI-layer posture: the planner refuses on
//! ambiguity; the executor refuses unknown types; intake refuses
//! unknown domains. Strictness flows top-to-bottom.
//!
//! ## What intake produces
//!
//! A `ProjectSketch` is shape-only — model names, field names, field
//! types from [`VALID_TYPE_NAMES`](crate::schema::VALID_TYPE_NAMES),
//! and a one-line rationale per model. It does **not** produce
//! `Primitive` ops directly; the wizard converts each accepted
//! `ModelSketch` into an `AddModel` primitive at apply time, after
//! the user has accepted it.

use serde::{Deserialize, Serialize};

/// A starting shape proposed to the user. Carries the original
/// description verbatim so the wizard can echo it back and the
/// downstream `PlanDocument` can record what was asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectSketch {
    /// Short domain slug used in CLI output (`clinic`, `blog`, …).
    pub domain: &'static str,
    /// One-line summary of what this template is for.
    pub headline: &'static str,
    /// The user's original description, verbatim.
    pub user_description: String,
    /// 2–4 models in the order they should be introduced. Earlier
    /// models are referenced by later ones via `belongs_to`, so order
    /// matters.
    pub models: Vec<ModelSketch>,
}

/// One proposed model. Field types and admin shape are determined
/// at this layer; the wizard turns this into an `AddModel` primitive
/// without further transformation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSketch {
    /// Rust struct name in PascalCase (`Patient`).
    pub struct_name: &'static str,
    /// Lowercase snake_case plural — the URL slug and the SQLite
    /// table name (`patients`).
    pub table: &'static str,
    pub fields: Vec<FieldSketch>,
    /// One-sentence rationale shown to the user when the model is
    /// proposed. The wizard reads this verbatim — keep it plain.
    pub rationale: &'static str,
}

/// One field on a proposed model. Type strings are constrained to
/// [`VALID_TYPE_NAMES`](crate::schema::VALID_TYPE_NAMES) so the
/// generated primitive validates without a translation step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSketch {
    pub name: &'static str,
    /// `String`, `i64`, `i32`, `bool`, `DateTime`. Anything else is
    /// rejected by the executor — keep this constrained.
    pub ty: &'static str,
    #[serde(default)]
    pub nullable: bool,
    /// When set, the wizard renders this field as a foreign key to
    /// the named model (which must appear earlier in `models`).
    /// `FieldType::ty` stays `i64`; the relation is added on top
    /// when the wizard expands the sketch into a primitive.
    #[serde(default)]
    pub belongs_to: Option<&'static str>,
}

/// Parse a free-text description and return a project shape, or
/// `None` when no domain template matches.
///
/// The matcher is intentionally simple: lowercase the input, look for
/// any of a small set of keywords per domain, return the first
/// match. We accept false positives (e.g. "I want to blog about
/// patients" matches `blog` because it appears first in the order)
/// rather than build a confidence-scoring stack — the user sees the
/// proposal and can reject it.
///
/// Returning `None` is the **right answer** for ambiguous input. The
/// wizard caller handles it with a clear refusal message and a
/// follow-up question, not an apologetic best-effort guess.
pub fn sketch(description: &str) -> Option<ProjectSketch> {
    let lower = description.to_lowercase();
    for (keywords, build) in DOMAIN_TABLE {
        if keywords.iter().any(|k| lower.contains(k)) {
            return Some(build(description.to_string()));
        }
    }
    None
}

/// Ordered list of domain templates. First match wins. New domains
/// append to the end. Keep keyword sets disjoint where possible so
/// the order matters less in practice.
type DomainBuilder = fn(String) -> ProjectSketch;
const DOMAIN_TABLE: &[(&[&str], DomainBuilder)] = &[
    (
        &[
            "clinic",
            "patient",
            "doctor",
            "appointment",
            "hospital",
            "medical",
        ],
        clinic_sketch,
    ),
    (
        &["blog", "post", "article", "comment", "publish"],
        blog_sketch,
    ),
    (
        &[
            "shop",
            "store",
            "product",
            "inventory",
            "stock",
            "sku",
            "order",
        ],
        shop_sketch,
    ),
    (
        &[
            "crm",
            "customer",
            "lead",
            "deal",
            "contact",
            "sales pipeline",
        ],
        crm_sketch,
    ),
    (
        &["task", "todo", "project", "ticket", "issue", "kanban"],
        tasks_sketch,
    ),
];

// ---- Domain templates ----------------------------------------------------
//
// Each one is a small Rust function that returns a `ProjectSketch`.
// Hand-crafted, not derived. They live here (not in `industry.rs`)
// because they target the *intake* layer — they describe what to
// build, not what compliance signals to surface.
//
// Constraints:
//   - 2–4 models per template; more becomes overwhelming in the wizard.
//   - Only types from VALID_TYPE_NAMES.
//   - Foreign keys via `belongs_to` reference an earlier model.
//   - Rationale strings are sentence-case, one line, no jargon.

fn clinic_sketch(description: String) -> ProjectSketch {
    ProjectSketch {
        domain: "clinic",
        headline: "A small clinic — patients, doctors, appointments.",
        user_description: description,
        models: vec![
            ModelSketch {
                struct_name: "Patient",
                table: "patients",
                rationale: "Each person you treat. Name is required; date of birth is useful for the chart, phone for reminders.",
                fields: vec![
                    FieldSketch { name: "name",          ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "date_of_birth", ty: "DateTime", nullable: true,  belongs_to: None },
                    FieldSketch { name: "phone",         ty: "String",   nullable: true,  belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Doctor",
                table: "doctors",
                rationale: "The staff who see patients. Specialty helps when scheduling.",
                fields: vec![
                    FieldSketch { name: "name",      ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "specialty", ty: "String", nullable: true,  belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Appointment",
                table: "appointments",
                rationale: "A scheduled visit — links a patient to a doctor with a time.",
                fields: vec![
                    FieldSketch { name: "patient_id", ty: "i64",      nullable: false, belongs_to: Some("Patient") },
                    FieldSketch { name: "doctor_id",  ty: "i64",      nullable: false, belongs_to: Some("Doctor")  },
                    FieldSketch { name: "scheduled_for", ty: "DateTime", nullable: false, belongs_to: None },
                    FieldSketch { name: "notes",      ty: "String",   nullable: true,  belongs_to: None },
                ],
            },
        ],
    }
}

fn blog_sketch(description: String) -> ProjectSketch {
    ProjectSketch {
        domain: "blog",
        headline: "A blog — authors and posts.",
        user_description: description,
        models: vec![
            ModelSketch {
                struct_name: "Author",
                table: "authors",
                rationale: "The people who write. Name is required; bio is optional.",
                fields: vec![
                    FieldSketch {
                        name: "name",
                        ty: "String",
                        nullable: false,
                        belongs_to: None,
                    },
                    FieldSketch {
                        name: "bio",
                        ty: "String",
                        nullable: true,
                        belongs_to: None,
                    },
                ],
            },
            ModelSketch {
                struct_name: "Post",
                table: "posts",
                rationale: "One article. Title, body, and a publication timestamp.",
                fields: vec![
                    FieldSketch {
                        name: "author_id",
                        ty: "i64",
                        nullable: false,
                        belongs_to: Some("Author"),
                    },
                    FieldSketch {
                        name: "title",
                        ty: "String",
                        nullable: false,
                        belongs_to: None,
                    },
                    FieldSketch {
                        name: "body",
                        ty: "String",
                        nullable: false,
                        belongs_to: None,
                    },
                    FieldSketch {
                        name: "published_at",
                        ty: "DateTime",
                        nullable: true,
                        belongs_to: None,
                    },
                ],
            },
        ],
    }
}

fn shop_sketch(description: String) -> ProjectSketch {
    ProjectSketch {
        domain: "shop",
        headline: "A small shop — products and orders.",
        user_description: description,
        models: vec![
            ModelSketch {
                struct_name: "Product",
                table: "products",
                rationale: "What you sell. SKU is the unique identifier; stock is what's on hand.",
                fields: vec![
                    FieldSketch { name: "sku",       ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "name",      ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "price_cents",ty: "i64",   nullable: false, belongs_to: None },
                    FieldSketch { name: "stock",     ty: "i64",    nullable: false, belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Order",
                table: "orders",
                rationale: "A single transaction. Carries the buyer's email so you can reach them without a separate Customer table on day one.",
                fields: vec![
                    FieldSketch { name: "product_id",  ty: "i64",      nullable: false, belongs_to: Some("Product") },
                    FieldSketch { name: "quantity",    ty: "i64",      nullable: false, belongs_to: None },
                    FieldSketch { name: "buyer_email", ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "placed_at",   ty: "DateTime", nullable: false, belongs_to: None },
                ],
            },
        ],
    }
}

fn crm_sketch(description: String) -> ProjectSketch {
    ProjectSketch {
        domain: "crm",
        headline: "A small CRM — companies, contacts, deals.",
        user_description: description,
        models: vec![
            ModelSketch {
                struct_name: "Company",
                table: "companies",
                rationale: "An organisation you might sell to.",
                fields: vec![
                    FieldSketch { name: "name",    ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "website", ty: "String", nullable: true,  belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Contact",
                table: "contacts",
                rationale: "A person at a company. Belongs to one Company.",
                fields: vec![
                    FieldSketch { name: "company_id", ty: "i64",    nullable: false, belongs_to: Some("Company") },
                    FieldSketch { name: "name",       ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "email",      ty: "String", nullable: true,  belongs_to: None },
                    FieldSketch { name: "phone",      ty: "String", nullable: true,  belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Deal",
                table: "deals",
                rationale: "An opportunity. Linked to a Contact; status tracks stage; amount is in cents to keep arithmetic clean.",
                fields: vec![
                    FieldSketch { name: "contact_id",  ty: "i64",      nullable: false, belongs_to: Some("Contact") },
                    FieldSketch { name: "title",       ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "status",      ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "amount_cents",ty: "i64",      nullable: true,  belongs_to: None },
                    FieldSketch { name: "closed_at",   ty: "DateTime", nullable: true,  belongs_to: None },
                ],
            },
        ],
    }
}

fn tasks_sketch(description: String) -> ProjectSketch {
    ProjectSketch {
        domain: "tasks",
        headline: "A task tracker — projects and tasks.",
        user_description: description,
        models: vec![
            ModelSketch {
                struct_name: "Project",
                table: "projects",
                rationale: "A container for related tasks.",
                fields: vec![
                    FieldSketch { name: "name",        ty: "String", nullable: false, belongs_to: None },
                    FieldSketch { name: "description", ty: "String", nullable: true,  belongs_to: None },
                ],
            },
            ModelSketch {
                struct_name: "Task",
                table: "tasks",
                rationale: "One thing to do. Status moves from todo → in_progress → done; priority is a small integer.",
                fields: vec![
                    FieldSketch { name: "project_id", ty: "i64",      nullable: false, belongs_to: Some("Project") },
                    FieldSketch { name: "title",      ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "status",     ty: "String",   nullable: false, belongs_to: None },
                    FieldSketch { name: "priority",   ty: "i64",      nullable: true,  belongs_to: None },
                    FieldSketch { name: "due_at",     ty: "DateTime", nullable: true,  belongs_to: None },
                ],
            },
        ],
    }
}

// ---- Expansion to a `Plan` -----------------------------------------------

use crate::ai::{AddModel, AddRelation, FieldSpec, Plan, Primitive, RelationKind};

/// Expand a single `ModelSketch` into the primitives that create it:
/// one `AddModel` for the table, then one `AddRelation` per `belongs_to`
/// field. The wizard calls this once per *accepted* model so primitives
/// from skipped models never reach the plan.
///
/// All `AddRelation`s are emitted as `belongs_to`; the relation layer
/// (0.9.x) enforces them at SQL FK level once the model lands.
pub fn primitives_for(model: &ModelSketch) -> Vec<Primitive> {
    let fields: Vec<FieldSpec> = model
        .fields
        .iter()
        .map(|f| FieldSpec {
            name: f.name.to_string(),
            ty: f.ty.to_string(),
            nullable: f.nullable,
            editable: true,
        })
        .collect();

    let mut out: Vec<Primitive> = Vec::with_capacity(1 + model.fields.len());
    out.push(Primitive::AddModel(AddModel {
        name: model.struct_name.to_string(),
        table: model.table.to_string(),
        fields,
    }));
    for f in &model.fields {
        if let Some(target) = f.belongs_to {
            // The wizard always introduces relations as `belongs_to` with
            // the default `ON DELETE RESTRICT` posture. Fancier modes
            // (Cascade, SetNull, required=true on top of an existing
            // table) are out of scope for intake — they belong to a
            // later `rustio ai plan` invocation if the user wants them.
            out.push(Primitive::AddRelation(AddRelation {
                from: model.struct_name.to_string(),
                kind: RelationKind::BelongsTo,
                to: target.to_string(),
                via: f.name.to_string(),
                required: false,
                on_delete: Default::default(),
            }));
        }
    }
    out
}

/// Convenience: build a `Plan` from a list of accepted models. The
/// caller is responsible for ordering — `belongs_to` targets must be
/// added before the model that references them. The domain templates
/// in this module already encode the right order.
pub fn plan_for(accepted: &[ModelSketch]) -> Plan {
    let mut steps: Vec<Primitive> = Vec::new();
    for m in accepted {
        steps.extend(primitives_for(m));
    }
    Plan { steps }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clinic_keyword_yields_clinic_sketch() {
        let s = sketch("a small clinic with patients and appointments").unwrap();
        assert_eq!(s.domain, "clinic");
        let names: Vec<&str> = s.models.iter().map(|m| m.struct_name).collect();
        assert_eq!(names, vec!["Patient", "Doctor", "Appointment"]);
    }

    #[test]
    fn ambiguous_input_refuses() {
        assert!(sketch("I want to build something").is_none());
        assert!(sketch("").is_none());
    }

    #[test]
    fn shop_template_uses_only_valid_types() {
        use crate::schema::VALID_TYPE_NAMES;
        let s = sketch("a shop with products and orders").unwrap();
        for m in &s.models {
            for f in &m.fields {
                assert!(
                    VALID_TYPE_NAMES.contains(&f.ty),
                    "field {}.{} has invalid type `{}`",
                    m.struct_name,
                    f.name,
                    f.ty
                );
            }
        }
    }

    #[test]
    fn belongs_to_targets_an_earlier_model() {
        for descr in [
            "clinic",
            "blog",
            "shop with products",
            "crm with deals",
            "tasks",
        ] {
            let s = sketch(descr).unwrap();
            let mut seen: Vec<&str> = Vec::new();
            for m in &s.models {
                for f in &m.fields {
                    if let Some(target) = f.belongs_to {
                        assert!(
                            seen.contains(&target),
                            "{}.{} → `{}` references a model not yet introduced",
                            m.struct_name,
                            f.name,
                            target
                        );
                    }
                }
                seen.push(m.struct_name);
            }
        }
    }

    #[test]
    fn primitives_for_emits_add_model_then_relations() {
        let s = sketch("clinic").unwrap();
        let appointment = s
            .models
            .iter()
            .find(|m| m.struct_name == "Appointment")
            .unwrap();
        let ops = primitives_for(appointment);
        // Exactly one AddModel + one AddRelation per belongs_to field
        // (Appointment has two: patient_id, doctor_id).
        assert!(matches!(ops.first(), Some(Primitive::AddModel(_))));
        let n_relations = ops
            .iter()
            .filter(|p| matches!(p, Primitive::AddRelation(_)))
            .count();
        assert_eq!(n_relations, 2);
    }

    #[test]
    fn plan_for_full_sketch_validates_against_empty_schema() {
        use crate::schema::{Schema, SCHEMA_VERSION};
        // No `Schema::empty()` helper exists — build the minimal valid
        // shape inline. `models: []` is the canonical empty schema; the
        // planner / executor treat it as "fresh project, no tables yet."
        let empty = Schema {
            version: SCHEMA_VERSION,
            rustio_version: env!("CARGO_PKG_VERSION").to_string(),
            models: vec![],
        };
        let sk = sketch("a small clinic").unwrap();
        let plan = plan_for(&sk.models);
        // The plan should simulate cleanly against a fresh schema —
        // belongs_to targets are added before referrers thanks to the
        // template ordering.
        plan.validate(&empty)
            .expect("clinic sketch should simulate cleanly against empty schema");
    }
}
