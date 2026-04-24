//! Rule-based planner. Takes a natural-language prompt and parses it
//! into a `Plan`. If the grammar cannot match, we refuse — we never
//! guess. This is the whole reason RustIO's AI layer can be trusted.

use crate::ai::primitive::{FieldSpec, Plan, Primitive};

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("could not understand prompt: {0}")]
    Unparseable(String),
}

pub fn plan(prompt: &str) -> Result<Plan, PlanError> {
    let normalised = prompt.trim().to_ascii_lowercase();
    let plan = Plan::new(prompt);

    if let Some(step) = try_add_field(&normalised) {
        return Ok(plan.step(step));
    }
    if let Some(step) = try_remove_field(&normalised) {
        return Ok(plan.step(step));
    }
    if let Some(step) = try_rename_field(&normalised) {
        return Ok(plan.step(step));
    }
    if let Some(step) = try_add_relation(&normalised) {
        return Ok(plan.step(step));
    }
    if let Some(step) = try_rename_model(&normalised) {
        return Ok(plan.step(step));
    }

    // NOTE: we intentionally do *not* fall back to a generic "add unknown
    // field" primitive. If the user's sentence doesn't match a rule, we
    // tell them — it's safer than producing a plan they didn't ask for.
    Err(PlanError::Unparseable(prompt.to_string()))
}

// ---- grammar rules -------------------------------------------------------

// "add <field> [as <type>] to <model>"
fn try_add_field(s: &str) -> Option<Primitive> {
    let rest = s.strip_prefix("add ")?;
    let (left, model) = rest.split_once(" to ")?;
    let (name, type_hint) = match left.split_once(" as ") {
        Some((n, t)) => (n.trim(), Some(t.trim())),
        None => (left.trim(), None),
    };
    if name.is_empty() || model.is_empty() {
        return None;
    }
    // naming heuristic (the same kind of rule the 0.7.2 update shipped):
    // fields that look like money end up as i64, not i32.
    let inferred = infer_type(name, type_hint);
    Some(Primitive::AddField {
        model: model_name(model),
        field: FieldSpec {
            name: name.to_string(),
            field_type: inferred,
            nullable: false,
        },
    })
}

// "remove <field> from <model>"
fn try_remove_field(s: &str) -> Option<Primitive> {
    let rest = s.strip_prefix("remove ")?;
    let (field, model) = rest.split_once(" from ")?;
    Some(Primitive::RemoveField {
        model: model_name(model.trim()),
        field: field.trim().to_string(),
    })
}

// "rename <from> to <to> in <model>"
fn try_rename_field(s: &str) -> Option<Primitive> {
    let rest = s.strip_prefix("rename ")?;
    let (from, tail) = rest.split_once(" to ")?;
    let (to, model) = tail.split_once(" in ")?;
    Some(Primitive::RenameField {
        model: model_name(model.trim()),
        from: from.trim().to_string(),
        to: to.trim().to_string(),
    })
}

// "link <from> to <to>" / "connect <from> to <to>" / "add relation from <from> to <to>"
fn try_add_relation(s: &str) -> Option<Primitive> {
    let (from, to) = if let Some(r) = s.strip_prefix("link ") {
        r.split_once(" to ")?
    } else if let Some(r) = s.strip_prefix("connect ") {
        r.split_once(" to ")?
    } else if let Some(r) = s.strip_prefix("add relation from ") {
        r.split_once(" to ")?
    } else {
        return None;
    };
    let from_model = model_name(from.trim());
    let to_model = model_name(to.trim());
    let via = format!("{}_id", to_model.to_ascii_lowercase());
    Some(Primitive::AddRelation {
        from_model,
        to_model,
        via,
    })
}

// "rename model <from> to <to>"
fn try_rename_model(s: &str) -> Option<Primitive> {
    let rest = s.strip_prefix("rename model ")?;
    let (from, to) = rest.split_once(" to ")?;
    Some(Primitive::RenameModel {
        from: model_name(from.trim()),
        to: model_name(to.trim()),
    })
}

// ---- heuristics ----------------------------------------------------------

fn infer_type(field_name: &str, explicit: Option<&str>) -> String {
    if let Some(t) = explicit {
        return canonical_type(t);
    }
    let lower = field_name.to_ascii_lowercase();
    if lower.ends_with("_at") || lower.ends_with("_date") || lower == "date" {
        return "DateTime".into();
    }
    if lower == "is_active" || lower.starts_with("is_") || lower.starts_with("has_") {
        return "bool".into();
    }
    // Money-ish names overflow i32 quickly, so use i64.
    if lower.ends_with("_amount")
        || lower.ends_with("_price")
        || lower.ends_with("_income")
        || lower.ends_with("_total")
        || lower == "balance"
        || lower == "amount"
        || lower == "price"
    {
        return "i64".into();
    }
    if lower.ends_with("_count") || lower == "count" || lower == "priority" || lower == "score" {
        return "i32".into();
    }
    "String".into()
}

fn canonical_type(s: &str) -> String {
    match s.trim().to_ascii_lowercase().as_str() {
        "int" | "integer" | "i32" => "i32".into(),
        "long" | "i64" | "bigint" => "i64".into(),
        "bool" | "boolean" => "bool".into(),
        "text" | "string" | "str" => "String".into(),
        "datetime" | "timestamp" => "DateTime".into(),
        other => other.to_string(),
    }
}

fn model_name(s: &str) -> String {
    // "posts" → "Post". Crude, but predictable. The executor will
    // re-check against the actual schema before writing.
    let t = s.trim().trim_end_matches('s');
    let mut chars = t.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_add_field() {
        let p = plan("add email to users").unwrap();
        assert_eq!(p.steps.len(), 1);
        assert!(matches!(&p.steps[0], Primitive::AddField { model, .. } if model == "User"));
    }

    #[test]
    fn add_datetime_name_gets_datetime_type() {
        let p = plan("add created_at to posts").unwrap();
        if let Primitive::AddField { field, .. } = &p.steps[0] {
            assert_eq!(field.field_type, "DateTime");
        } else {
            panic!("expected AddField");
        }
    }

    #[test]
    fn money_names_get_i64() {
        let p = plan("add annual_income to users").unwrap();
        if let Primitive::AddField { field, .. } = &p.steps[0] {
            assert_eq!(field.field_type, "i64");
        } else {
            panic!("expected AddField");
        }
    }

    #[test]
    fn gibberish_is_refused() {
        let err = plan("please make it fast").unwrap_err();
        assert!(matches!(err, PlanError::Unparseable(_)));
    }

    #[test]
    fn parses_link_as_relation() {
        let p = plan("link posts to authors").unwrap();
        match &p.steps[0] {
            Primitive::AddRelation { from_model, to_model, via } => {
                assert_eq!(from_model, "Post");
                assert_eq!(to_model, "Author");
                assert_eq!(via, "author_id");
            }
            _ => panic!("expected AddRelation"),
        }
    }

    #[test]
    fn rename_field_parses() {
        let p = plan("rename name to full_name in users").unwrap();
        match &p.steps[0] {
            Primitive::RenameField { model, from, to } => {
                assert_eq!(model, "User");
                assert_eq!(from, "name");
                assert_eq!(to, "full_name");
            }
            _ => panic!("expected RenameField"),
        }
    }
}
