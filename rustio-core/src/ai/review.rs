//! Plan review. Given a plan and the current schema, produces a
//! deterministic (no randomness, no LLM) report of risk, impact, and
//! warnings. Pure function — same inputs always produce the same
//! output.

use serde::{Deserialize, Serialize};

use crate::ai::primitive::{Plan, Primitive};
use crate::schema::Schema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impact {
    pub models_touched: Vec<String>,
    pub writes_migration: bool,
    pub writes_rust_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub risk: Risk,
    pub impact: Impact,
    pub warnings: Vec<String>,
}

pub fn review(plan: &Plan, schema: &Schema) -> Review {
    let mut warnings = Vec::new();
    let mut models_touched: Vec<String> = Vec::new();
    let mut max_risk = Risk::Low;

    for step in &plan.steps {
        let (step_risk, step_warnings, touched) = score_step(step, schema);
        max_risk = std::cmp::max(max_risk, step_risk);
        warnings.extend(step_warnings);
        for m in touched {
            if !models_touched.contains(&m) {
                models_touched.push(m);
            }
        }
    }

    Review {
        risk: max_risk,
        impact: Impact {
            models_touched,
            writes_migration: true,
            writes_rust_file: true,
        },
        warnings,
    }
}

fn score_step(step: &Primitive, schema: &Schema) -> (Risk, Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    match step {
        Primitive::AddField { model, field } => {
            let touched = vec![model.clone()];
            if schema.model(model).is_none() {
                warnings.push(format!("model `{model}` not found in current schema"));
            } else if schema
                .model(model)
                .map(|m| m.fields.iter().any(|f| f.name == field.name))
                .unwrap_or(false)
            {
                warnings.push(format!("`{}` already has a field named `{}`", model, field.name));
            }
            (Risk::Low, warnings, touched)
        }
        Primitive::RemoveField { model, field } => {
            let touched = vec![model.clone()];
            warnings.push(format!(
                "removing `{field}` from `{model}` is destructive; data in that column will be lost"
            ));
            (Risk::High, warnings, touched)
        }
        Primitive::RenameField { model, from, to } => {
            let touched = vec![model.clone()];
            if let Some(m) = schema.model(model) {
                if !m.fields.iter().any(|f| f.name == *from) {
                    warnings.push(format!("no field `{from}` on `{model}`"));
                }
                if m.fields.iter().any(|f| f.name == *to) {
                    warnings.push(format!("`{model}` already has a field named `{to}`"));
                }
            }
            (Risk::Medium, warnings, touched)
        }
        Primitive::AddRelation { from_model, to_model, via } => {
            let touched = vec![from_model.clone()];
            if schema.model(to_model).is_none() {
                warnings.push(format!("target model `{to_model}` not found"));
            }
            warnings.push(format!(
                "relation materialises as an `{via}` i64 column; no SQL FOREIGN KEY is emitted yet"
            ));
            (Risk::Low, warnings, touched)
        }
        Primitive::RenameModel { from, to } => {
            let touched = vec![from.clone()];
            if schema.model(from).is_none() {
                warnings.push(format!("no model `{from}` in current schema"));
            }
            if schema.model(to).is_some() {
                warnings.push(format!("a model named `{to}` already exists"));
            }
            (Risk::High, warnings, touched)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::primitive::FieldSpec;
    use crate::schema::{Schema, SchemaField, SchemaModel, SCHEMA_VERSION};

    fn empty_schema() -> Schema {
        Schema {
            version: SCHEMA_VERSION,
            rustio_version: "test".into(),
            models: vec![],
        }
    }

    #[test]
    fn add_field_on_missing_model_warns() {
        let plan = Plan::new("x").step(Primitive::AddField {
            model: "NotHere".into(),
            field: FieldSpec {
                name: "x".into(),
                field_type: "String".into(),
                nullable: false,
            },
        });
        let review = review(&plan, &empty_schema());
        assert_eq!(review.risk, Risk::Low);
        assert!(!review.warnings.is_empty());
    }

    #[test]
    fn remove_field_is_high_risk() {
        let plan = Plan::new("x").step(Primitive::RemoveField {
            model: "Post".into(),
            field: "title".into(),
        });
        let review = review(&plan, &empty_schema());
        assert_eq!(review.risk, Risk::High);
    }

    #[test]
    fn duplicate_add_field_warns() {
        let schema = Schema {
            version: SCHEMA_VERSION,
            rustio_version: "test".into(),
            models: vec![SchemaModel {
                name: "Post".into(),
                table: "posts".into(),
                admin_name: "posts".into(),
                display_name: "Posts".into(),
                singular_name: "Post".into(),
                core: false,
                fields: vec![SchemaField {
                    name: "title".into(),
                    field_type: "String".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                }],
            }],
        };
        let plan = Plan::new("x").step(Primitive::AddField {
            model: "Post".into(),
            field: FieldSpec {
                name: "title".into(),
                field_type: "String".into(),
                nullable: false,
            },
        });
        let review = review(&plan, &schema);
        assert!(review.warnings.iter().any(|w| w.contains("already")));
    }
}
