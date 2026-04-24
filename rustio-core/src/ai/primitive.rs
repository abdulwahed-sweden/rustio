//! The closed vocabulary of things the AI layer can propose. Adding a
//! new primitive is a four-step process: add it here, teach the
//! planner how to parse it, teach the reviewer how to score it, teach
//! the executor how to apply it. The `#[non_exhaustive]` attribute
//! forces external matchers to handle unknown variants explicitly.

use serde::{Deserialize, Serialize};

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Primitive {
    AddField {
        model: String,
        field: FieldSpec,
    },
    RemoveField {
        model: String,
        field: String,
    },
    RenameField {
        model: String,
        from: String,
        to: String,
    },
    AddRelation {
        from_model: String,
        to_model: String,
        via: String,
    },
    RenameModel {
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub prompt: String,
    pub steps: Vec<Primitive>,
}

impl Plan {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            steps: Vec::new(),
        }
    }

    pub fn step(mut self, p: Primitive) -> Self {
        self.steps.push(p);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_serde_round_trip() {
        let plan = Plan::new("add email to users").step(Primitive::AddField {
            model: "User".into(),
            field: FieldSpec {
                name: "email".into(),
                field_type: "String".into(),
                nullable: false,
            },
        });
        let json = serde_json::to_string(&plan).unwrap();
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan, back);
    }

    #[test]
    fn unknown_keys_rejected() {
        let bad = r#"{"prompt":"x","steps":[],"extra":"nope"}"#;
        assert!(serde_json::from_str::<Plan>(bad).is_err());
    }
}
