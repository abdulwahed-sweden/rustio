//! Phase 8.1 — minimal schema-diff for the AI update flow.
//!
//! Used ONLY by the CLI to render a human-readable change-list before
//! the operator confirms a write. Intentionally small: model + field
//! adds / removes, plus a flag when a field gains or loses a `Relation`.
//! No deep structural diff (type changes, nullability flips, rename
//! detection) — keep the surface narrow so the operator can read the
//! whole thing in one screen.

use crate::schema::{Schema, SchemaField, SchemaModel};

/// One human-readable change line between two schemas. Variant order
/// is the print order: model adds first, then per-model field churn,
/// then model removes (least surprising scan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    ModelAdded(String),
    ModelRemoved(String),
    FieldAdded {
        model: String,
        field: String,
        ty: String,
        relation: Option<String>,
    },
    FieldRemoved {
        model: String,
        field: String,
    },
    RelationAdded {
        model: String,
        field: String,
        target: String,
    },
    RelationRemoved {
        model: String,
        field: String,
    },
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelAdded(name) => write!(f, "+ Model added: {name}"),
            Self::ModelRemoved(name) => write!(f, "- Model removed: {name}"),
            Self::FieldAdded { model, field, ty, relation } => {
                if let Some(target) = relation {
                    write!(f, "+ Field added: {model}.{field} ({ty}) → {target}")
                } else {
                    write!(f, "+ Field added: {model}.{field} ({ty})")
                }
            }
            Self::FieldRemoved { model, field } => {
                write!(f, "- Field removed: {model}.{field}")
            }
            Self::RelationAdded { model, field, target } => {
                write!(f, "+ Relation added: {model}.{field} → {target}")
            }
            Self::RelationRemoved { model, field } => {
                write!(f, "- Relation removed: {model}.{field}")
            }
        }
    }
}

/// Compute the change-list between two schemas. Order:
///   1. ModelAdded (new models surface their full field-add list too)
///   2. Per-model field add/remove + relation add/remove for fields
///      that exist in both versions
///   3. ModelRemoved (last so the human eye sees additions before
///      destructive lines)
pub fn diff(old: &Schema, new: &Schema) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();

    // Models added (in new, not in old by name).
    for m in &new.models {
        if !old.models.iter().any(|o| o.name == m.name) {
            out.push(Change::ModelAdded(m.name.clone()));
            // Also surface every field of the new model as added so
            // the operator sees the full shape of what just landed.
            for f in &m.fields {
                out.push(field_added_change(&m.name, f));
            }
        }
    }

    // Per-model field churn for models in both versions.
    for new_model in &new.models {
        let Some(old_model) = old.models.iter().find(|o| o.name == new_model.name) else {
            continue;
        };
        diff_fields(old_model, new_model, &mut out);
    }

    // Models removed (last per print-order rationale above).
    for o in &old.models {
        if !new.models.iter().any(|n| n.name == o.name) {
            out.push(Change::ModelRemoved(o.name.clone()));
        }
    }

    out
}

fn diff_fields(old: &SchemaModel, new: &SchemaModel, out: &mut Vec<Change>) {
    // Field added in new (and not present in old by name).
    for f in &new.fields {
        if !old.fields.iter().any(|of| of.name == f.name) {
            out.push(field_added_change(&new.name, f));
        }
    }
    // Field removed (in old, not in new).
    for of in &old.fields {
        if !new.fields.iter().any(|f| f.name == of.name) {
            out.push(Change::FieldRemoved {
                model: new.name.clone(),
                field: of.name.clone(),
            });
        }
    }
    // Relation churn on fields that exist in both versions: relation
    // gained, relation dropped. (Relation target rename is not
    // reported as a separate event — it surfaces as drop+add in the
    // unlikely case the model rewrites the relation block.)
    for f in &new.fields {
        if let Some(of) = old.fields.iter().find(|of| of.name == f.name) {
            match (&of.relation, &f.relation) {
                (None, Some(rel)) => out.push(Change::RelationAdded {
                    model: new.name.clone(),
                    field: f.name.clone(),
                    target: rel.model.clone(),
                }),
                (Some(_), None) => out.push(Change::RelationRemoved {
                    model: new.name.clone(),
                    field: f.name.clone(),
                }),
                _ => {}
            }
        }
    }
}

fn field_added_change(model: &str, f: &SchemaField) -> Change {
    Change::FieldAdded {
        model: model.to_string(),
        field: f.name.clone(),
        ty: f.ty.clone(),
        relation: f.relation.as_ref().map(|r| r.model.clone()),
    }
}

/// Pretty-print a change list to a single string. Empty list yields
/// the literal string `"(no changes)"` so the CLI can show *something*
/// rather than printing a blank section.
pub fn render(changes: &[Change]) -> String {
    if changes.is_empty() {
        return "(no changes)".to_string();
    }
    let mut out = String::new();
    for (i, c) in changes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&c.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Relation, RelationKind, Schema, SchemaField, SchemaModel, SCHEMA_VERSION};

    /// Hand-build a minimal schema fixture. Models / fields are slice
    /// of (name, ty) pairs; the helper fills the boilerplate
    /// (table = name lowercased, admin/display/singular = name).
    fn schema_of(models: &[(&str, &[(&str, &str)])]) -> Schema {
        Schema {
            version: SCHEMA_VERSION,
            rustio_version: "1.0.0".into(),
            models: models
                .iter()
                .map(|(name, fields)| SchemaModel {
                    name: (*name).to_string(),
                    table: name.to_lowercase(),
                    admin_name: name.to_lowercase(),
                    display_name: (*name).to_string(),
                    singular_name: (*name).to_string(),
                    fields: fields
                        .iter()
                        .map(|(fname, ty)| SchemaField {
                            name: (*fname).to_string(),
                            ty: (*ty).to_string(),
                            nullable: false,
                            editable: true,
                            relation: None,
                        })
                        .collect(),
                    relations: vec![],
                    core: false,
                })
                .collect(),
        }
    }

    /// Phase 8.1 — adding a brand-new model surfaces both the model
    /// add AND each of its fields as adds, so the operator sees the
    /// full landed shape on screen.
    #[test]
    fn diff_reports_added_model_with_fields() {
        let old = schema_of(&[("Post", &[("id", "i64"), ("title", "String")])]);
        let new = schema_of(&[
            ("Post", &[("id", "i64"), ("title", "String")]),
            ("Tag", &[("id", "i64"), ("label", "String")]),
        ]);

        let changes = diff(&old, &new);
        assert!(changes.contains(&Change::ModelAdded("Tag".into())));
        assert!(changes.iter().any(|c| matches!(c,
            Change::FieldAdded { model, field, ty, .. }
                if model == "Tag" && field == "label" && ty == "String"
        )));
    }

    /// Phase 8.1 — fields that survive a diff must not surface as
    /// added or removed. Locks the preserve-by-default contract on
    /// the diff side (the prompt enforces it on the model side).
    #[test]
    fn diff_preserves_existing_fields() {
        let old = schema_of(&[("Post", &[("id", "i64"), ("title", "String"), ("body", "String")])]);
        let new = schema_of(&[("Post", &[("id", "i64"), ("title", "String"), ("body", "String"), ("status", "String")])]);

        let changes = diff(&old, &new);
        // No FieldRemoved for any of {id, title, body}.
        for surviving in ["id", "title", "body"] {
            assert!(
                !changes.iter().any(|c| matches!(c,
                    Change::FieldRemoved { field, .. } if field == surviving
                )),
                "preserved field {surviving} surfaced as removed: {changes:?}"
            );
        }
        // status is the only addition.
        assert_eq!(
            changes.iter().filter(|c| matches!(c, Change::FieldAdded { .. })).count(),
            1
        );
    }

    /// Phase 8.1 — render() output is deterministic and matches the
    /// CLI display. Locks the spec example shape:
    ///   + Model added: <Name>
    ///   + Field added: <Model>.<field> (<type>)
    ///   - Field removed: <Model>.<field>
    ///   + Relation added: <Model>.<field> → <target>
    #[test]
    fn diff_output_correct() {
        let mut old = schema_of(&[
            ("Post", &[("id", "i64"), ("title", "String"), ("summary", "String")]),
        ]);
        let mut new = schema_of(&[
            ("Post", &[("id", "i64"), ("title", "String"), ("status", "String"), ("author_id", "i64")]),
            ("Tag", &[("id", "i64")]),
        ]);

        // Add a relation block on Post.author_id in `new` so the
        // FieldAdded line includes the "→ User" target arrow.
        if let Some(p) = new.models.iter_mut().find(|m| m.name == "Post") {
            if let Some(f) = p.fields.iter_mut().find(|f| f.name == "author_id") {
                f.relation = Some(Relation {
                    model: "User".into(),
                    field: "id".into(),
                    kind: RelationKind::BelongsTo,
                    display_field: None,
                    required: None,
                    on_delete: None,
                });
            }
        }
        // Add a field that exists in both versions but gains a
        // relation in `new`. Pre-existing field: Post.title was
        // present in both — but a String field can't gain a relation,
        // so use a synthetic id field (Post.editor_id) added to old
        // first, then carry it into new with a relation block.
        old.models[0].fields.push(SchemaField {
            name: "editor_id".into(),
            ty: "i64".into(),
            nullable: false,
            editable: true,
            relation: None,
        });
        new.models[0].fields.push(SchemaField {
            name: "editor_id".into(),
            ty: "i64".into(),
            nullable: false,
            editable: true,
            relation: Some(Relation {
                model: "User".into(),
                field: "id".into(),
                kind: RelationKind::BelongsTo,
                display_field: None,
                required: None,
                on_delete: None,
            }),
        });

        let changes = diff(&old, &new);
        let rendered = render(&changes);

        // Spec example shape — at least these load-bearing lines must
        // be present (order matches the diff() print rationale).
        assert!(
            rendered.contains("+ Model added: Tag"),
            "missing model-added line; got:\n{rendered}"
        );
        assert!(
            rendered.contains("+ Field added: Post.status (String)"),
            "missing simple field-added line; got:\n{rendered}"
        );
        assert!(
            rendered.contains("+ Field added: Post.author_id (i64) → User"),
            "missing field-added-with-relation arrow; got:\n{rendered}"
        );
        assert!(
            rendered.contains("- Field removed: Post.summary"),
            "missing field-removed line; got:\n{rendered}"
        );
        assert!(
            rendered.contains("+ Relation added: Post.editor_id → User"),
            "missing relation-added line; got:\n{rendered}"
        );
    }

    /// Empty diff renders a placeholder string instead of nothing —
    /// the CLI prints under a "Changes:" header and a blank section
    /// would look like a UI bug.
    #[test]
    fn empty_diff_renders_placeholder() {
        let s = schema_of(&[("Post", &[("id", "i64")])]);
        assert_eq!(render(&diff(&s, &s)), "(no changes)");
    }
}
