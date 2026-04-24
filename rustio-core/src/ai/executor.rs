//! Safe Executor — 0.5.2.
//!
//! The layer that turns a reviewed [`PlanDocument`] into deterministic
//! on-disk changes. It behaves like *a cautious senior engineer applying
//! changes*: if anything is uncertain, it refuses.
//!
//! ## Posture
//!
//! - **Refusal-first.** Every primitive outside a tight safe subset is
//!   rejected with a named [`ExecutionError`] variant. The supported set
//!   for 0.5.2 is small by design (`add_field`, `rename_field`); the list
//!   grows only as each primitive's edit + migration story is proven safe.
//! - **All-or-nothing.** A partial apply is worse than no apply. The
//!   executor builds the full change set first (dry-run), verifies every
//!   precondition, then commits atomically — writing every target to a
//!   sibling `.tmp` and renaming on success. A mid-flight failure rolls
//!   back everything touched.
//! - **Never writes arbitrary SQL.** The only SQL that can land on disk
//!   comes from `build_migration_sql`, whose shape is entirely determined
//!   by the primitive being applied. There is no path from an AI prompt
//!   to a hand-written SQL statement.
//! - **Every check runs twice.** The executor re-runs `Plan::validate`
//!   and [`review_plan`] before touching anything, even though the
//!   document was validated when saved. Schemas drift, and this layer
//!   is the last place to catch that drift before a migration is written.
//!
//! ## What 0.5.2 supports
//!
//! - [`Primitive::AddField`] — adds a column via `ALTER TABLE … ADD
//!   COLUMN …` and patches the generated `apps/<app>/models.rs`
//!   (`struct`, `COLUMNS`, `INSERT_COLUMNS`, `from_row`, `insert_values`).
//!   Adds `use chrono::{DateTime, Utc};` if the new field needs it and
//!   the file doesn't already import it.
//! - [`Primitive::RenameField`] — `ALTER TABLE … RENAME COLUMN` plus a
//!   scoped rename inside the same models.rs.
//!
//! ## What 0.5.2 refuses
//!
//! Every other primitive returns [`ExecutionError::UnsupportedPrimitive`]
//! with a one-line reason. These land in later pull requests, not silent
//! "best effort" writes:
//!
//! - `add_model`, `remove_model`, `rename_model` — require cross-file
//!   scaffolding (apps tree, migrations, admin + views updates).
//! - `remove_field`, `remove_relation` — destructive; gated on a
//!   `--force` style flag that doesn't ship in 0.5.2.
//! - `change_field_type`, `change_field_nullability` — require SQLite
//!   table-recreation migrations which need their own review pass.
//! - `add_relation`, `update_admin` — out of scope for 0.5.2.
//! - `create_migration` — developer-only; refused at the
//!   [`ExecutionError::DeveloperOnlyForbidden`] gate before it ever
//!   reaches the dispatch.
//!
//! ## Testability
//!
//! The core logic ([`plan_execution`]) is pure: it takes a
//! [`ProjectView`] (in-memory snapshot of the files it cares about) and
//! returns an [`ExecutionPreview`]. No filesystem I/O. The impure entry
//! [`execute_plan_document`] wraps it with disk reads, the
//! confirmation-friendly preview, and atomic writes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::planner::ContextConfig;
use super::review::{
    review_plan, PlanDocument, RiskLevel, ValidationOutcome, PLAN_DOCUMENT_VERSION,
};
use super::{
    AddField, AddRelation, ChangeFieldNullability, ChangeFieldType, FieldSpec, Primitive,
    RemoveField, RemoveRelation, RenameField, RenameModel,
};
use crate::schema::{Schema, SchemaField};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Reported after a successful apply. Filenames are relative to the
/// project root passed to [`execute_plan_document`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    pub applied_steps: usize,
    pub generated_files: Vec<String>,
    pub summary: String,
}

/// Dry-run output. Produced by [`plan_execution`] before any write, and
/// displayed by the CLI as the "Plan to apply" preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPreview {
    pub applied_steps: usize,
    pub file_changes: Vec<PlannedFileChange>,
    pub summary: String,
}

/// One file the executor will write. `Create` expects the file to not
/// exist; `Update` expects it to match `expected_current_contents` byte
/// for byte — any mismatch means a human touched the file after the
/// plan was reviewed, which is a [`ExecutionError::FileConflict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFileChange {
    pub path: PathBuf,
    pub kind: FileChangeKind,
    pub new_contents: String,
    pub expected_current_contents: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Create,
    Update,
}

/// Knobs for `execute_plan_document`. Kept as a struct so the executor
/// can grow flags without a breaking signature change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecuteOptions {
    /// 0.9.1 — set to `true` to allow destructive primitives that would
    /// otherwise produce [`ExecutionError::DestructiveWithoutConfirmation`]:
    /// `remove_field`, `remove_relation`.
    ///
    /// `remove_model` stays refused with
    /// [`ExecutionError::UnsupportedPrimitive`] regardless of this flag
    /// until 0.9.2 — dropping a struct + its admin registration + every
    /// downstream FK is its own scope.
    ///
    /// **Never** bypasses the Critical-risk gate, the developer-only
    /// gate, or the PII policy gate — those live one layer up in
    /// `plan_execution` and are unaffected by this field.
    pub allow_destructive: bool,
}

/// Parsed view of the project files the executor cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectView {
    pub root: PathBuf,
    /// Parsed `apps/<app>/models.rs` files, keyed by app directory name.
    pub models_files: BTreeMap<String, ParsedModelsFile>,
    /// Filenames (not full paths) of files in `migrations/`.
    pub existing_migrations: Vec<String>,
    /// Contents of every migration file, keyed by filename. Populated by
    /// [`ProjectView::from_dir`]; tests constructing a `ProjectView`
    /// directly may leave it empty, in which case FK detection returns
    /// `false` (no constraint known) — the caller is responsible for
    /// seeding this map when simulating a project that has FKs.
    pub migration_sources: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedModelsFile {
    pub path: PathBuf,
    pub source: String,
    /// Every `pub struct X` declared in this file. Used to locate the
    /// app that owns a given model name.
    pub struct_names: Vec<String>,
}

/// Every way the executor can refuse. All variants are
/// refusal-first: nothing has been written when one of these is
/// returned.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// Re-validation against the current schema failed. Carries the
    /// human-readable reason so the caller can print it verbatim.
    ValidationFailed(String),
    /// The document's risk classifier reached `Critical`. Executing a
    /// plan at that level requires a reviewer to regenerate the plan
    /// under a changed posture — the executor will not do it.
    CriticalRiskNotAllowed,
    /// The plan contains a developer-only primitive (e.g.
    /// `CreateMigration`). These must never flow through the AI path.
    DeveloperOnlyForbidden,
    /// The plan was valid at save time but the current schema has
    /// drifted. The message names the step and the primitive error.
    SchemaMismatch(String),
    /// The executor was about to write a file that no longer matches
    /// the content recorded during the dry-run (or that exists when
    /// it shouldn't). Never silently overwrite.
    FileConflict { path: String, reason: String },
    /// The primitive is valid in principle but not wired up in 0.5.2.
    UnsupportedPrimitive {
        op: &'static str,
        reason: &'static str,
    },
    /// A destructive primitive was requested without `allow_destructive`.
    /// Reserved for 0.5.3+; 0.5.2 refuses destructive ops regardless.
    DestructiveWithoutConfirmation { op: &'static str },
    /// Expected project scaffolding isn't present (`apps/<x>/models.rs`
    /// missing for a model, `migrations/` directory missing, …).
    ProjectStructure(String),
    /// Filesystem error during read or write. Carries the OS message
    /// plus the offending path.
    IoError { path: String, message: String },
    /// The plan violates a policy derived from the project's
    /// [`ContextConfig`] — for example, a `remove_field` targeting a
    /// field flagged as personally-identifying under GDPR, or a
    /// `change_field_type` on a regulated column. Refused up-front;
    /// the operator can edit the context file or the plan and re-run.
    PolicyViolation { reason: String },
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValidationFailed(msg) => write!(f, "plan failed validation: {msg}"),
            Self::CriticalRiskNotAllowed => write!(
                f,
                "plan risk is Critical — the safe executor refuses to apply it"
            ),
            Self::DeveloperOnlyForbidden => write!(
                f,
                "plan contains a developer-only primitive — the safe executor refuses to apply it"
            ),
            Self::SchemaMismatch(msg) => write!(f, "plan is stale against the current schema: {msg}"),
            Self::FileConflict { path, reason } => {
                write!(f, "refusing to write `{path}`: {reason}")
            }
            Self::UnsupportedPrimitive { op, reason } => write!(
                f,
                "primitive `{op}` is not supported by the 0.5.2 safe executor: {reason}"
            ),
            Self::DestructiveWithoutConfirmation { op } => write!(
                f,
                "primitive `{op}` is destructive — re-run `rustio ai apply` with `--force` to open the destructive gate"
            ),
            Self::ProjectStructure(msg) => write!(f, "project layout: {msg}"),
            Self::IoError { path, message } => {
                write!(f, "i/o error on `{path}`: {message}")
            }
            Self::PolicyViolation { reason } => {
                write!(f, "policy violation: {reason}")
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

// ---------------------------------------------------------------------------
// Pure dry-run
// ---------------------------------------------------------------------------

/// Compute the exact set of file changes that executing `doc` would
/// produce, without touching the filesystem.
///
/// The caller supplies a [`ProjectView`] — an in-memory snapshot of
/// the project. This lets tests drive the whole pipeline without a
/// tempdir, and lets the CLI do the dry-run + preview before asking
/// the operator to confirm.
pub fn plan_execution(
    schema: &Schema,
    project: &ProjectView,
    doc: &PlanDocument,
    options: &ExecuteOptions,
    context: Option<&ContextConfig>,
) -> Result<ExecutionPreview, ExecutionError> {
    // Phase 1 — Load & Validate.
    if doc.version != PLAN_DOCUMENT_VERSION {
        return Err(ExecutionError::ValidationFailed(format!(
            "document version {} is not supported (this build reads version {})",
            doc.version, PLAN_DOCUMENT_VERSION
        )));
    }
    let review = review_plan(schema, &doc.plan, context)
        .map_err(|e| ExecutionError::ValidationFailed(e.to_string()))?;
    match &review.validation {
        ValidationOutcome::Valid => {}
        ValidationOutcome::Invalid { step, reason } => {
            return Err(ExecutionError::SchemaMismatch(format!(
                "plan invalid at step {step}: {reason}"
            )));
        }
    }

    // Phase 2 — Risk gate (context-aware via review).
    if review.risk == RiskLevel::Critical {
        return Err(ExecutionError::CriticalRiskNotAllowed);
    }

    // Phase 2b — Developer-only gate. `review_plan` would also have
    // flagged this, but we refuse independently so a future refactor
    // of the review scorer can't silently accept these.
    for step in &doc.plan.steps {
        if step.is_developer_only() {
            return Err(ExecutionError::DeveloperOnlyForbidden);
        }
    }

    // Phase 2c — Context policy gate. Refuses destructive or lossy
    // operations on fields the project context flags as personally-
    // identifying. The review layer already escalated these to
    // Critical (caught above); this gate is a dedicated refusal so
    // the error surface is explicit and named.
    if let Some(ctx) = context {
        let pii = ctx.pii_fields();
        for step in &doc.plan.steps {
            if let Some(reason) = policy_violation_for(step, &pii, ctx) {
                return Err(ExecutionError::PolicyViolation { reason });
            }
        }
    }

    // Phase 3 — Dry-run simulation. Build the complete file-change
    // set in memory, carrying a mutable shadow of the project so a
    // later step sees the in-progress edits of earlier ones (e.g. two
    // `add_field` steps on the same file).
    let mut shadow: BTreeMap<String, String> = project
        .models_files
        .iter()
        .map(|(app, file)| (app.clone(), file.source.clone()))
        .collect();
    let mut migration_counter = next_migration_number(&project.existing_migrations);
    let mut file_changes: Vec<PlannedFileChange> = Vec::new();
    let mut summary_lines: Vec<String> = Vec::new();

    // The schema shadow tracks shape mutations across steps so a later
    // `change_field_type` sees the rename an earlier step applied.
    let mut schema_shadow = schema.clone();
    for step in &doc.plan.steps {
        let (mut new_changes, one_line) = simulate_step(
            step,
            project,
            &mut shadow,
            &mut migration_counter,
            &schema_shadow,
            options,
        )?;
        file_changes.append(&mut new_changes);
        summary_lines.push(one_line);
        apply_schema_shadow(step, &mut schema_shadow);
    }

    // Deduplicate sequential updates to the same models.rs so only
    // the final version is emitted.
    file_changes = collapse_duplicate_updates(file_changes);

    Ok(ExecutionPreview {
        applied_steps: doc.plan.steps.len(),
        file_changes,
        summary: summary_lines.join("\n"),
    })
}

fn simulate_step(
    step: &Primitive,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
    schema: &Schema,
    opts: &ExecuteOptions,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    match step {
        Primitive::AddField(a) => apply_add_field(a, project, shadow, migration_counter),
        Primitive::RenameField(r) => apply_rename_field(r, project, shadow, migration_counter),
        Primitive::ChangeFieldType(c) => {
            apply_change_field_type(c, schema, project, shadow, migration_counter)
        }
        Primitive::ChangeFieldNullability(c) => {
            apply_change_field_nullability(c, schema, project, shadow, migration_counter)
        }
        Primitive::RenameModel(r) => apply_rename_model(r, project, shadow, migration_counter),
        // Everything else: refuse explicitly so the reviewer can see
        // which primitive stopped the apply.
        Primitive::AddModel(_) => Err(ExecutionError::UnsupportedPrimitive {
            op: "add_model",
            reason:
                "model scaffolding lives with `rustio new app`; use that then let the AI add fields",
        }),
        // 0.9.1 destructive gate: `remove_model` stays refused as
        // unsupported until 0.9.2; `remove_field` and `remove_relation`
        // honour `opts.allow_destructive` (the CLI's `--force` flag).
        Primitive::RemoveModel(_) => Err(ExecutionError::UnsupportedPrimitive {
            op: "remove_model",
            reason: "dropping a model + its admin registration + downstream FKs is scheduled for 0.9.2; use `rustio new app` / manual removal for now",
        }),
        Primitive::RemoveField(r) => {
            if !opts.allow_destructive {
                return Err(ExecutionError::DestructiveWithoutConfirmation {
                    op: "remove_field",
                });
            }
            apply_remove_field(r, schema, project, shadow, migration_counter)
        }
        Primitive::AddRelation(r) => apply_add_relation(r, project, shadow, migration_counter),
        Primitive::RemoveRelation(r) => {
            if !opts.allow_destructive {
                return Err(ExecutionError::DestructiveWithoutConfirmation {
                    op: "remove_relation",
                });
            }
            apply_remove_relation(r, schema, project, shadow, migration_counter)
        }
        Primitive::UpdateAdmin(_) => Err(ExecutionError::UnsupportedPrimitive {
            op: "update_admin",
            reason: "admin-attribute edits are out of scope for 0.5.2",
        }),
        Primitive::CreateMigration(_) => Err(ExecutionError::DeveloperOnlyForbidden),
    }
}

/// Collapse multiple `Update`s to the same path into a single change
/// holding the final contents — so two sequential `add_field` steps
/// on the same file emit one diff, not two.
fn collapse_duplicate_updates(changes: Vec<PlannedFileChange>) -> Vec<PlannedFileChange> {
    let mut out: Vec<PlannedFileChange> = Vec::with_capacity(changes.len());
    for c in changes {
        if let Some(existing) = out.iter_mut().rev().find(|e| {
            e.path == c.path && e.kind == FileChangeKind::Update && c.kind == FileChangeKind::Update
        }) {
            existing.new_contents = c.new_contents;
            // expected_current_contents stays pinned to the initial file
            // contents — the conflict check runs against disk at apply time.
            continue;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Per-primitive simulators
// ---------------------------------------------------------------------------

fn apply_add_field(
    a: &AddField,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    // Locate the app and initial source of the file owning this struct.
    let (app, initial_source) = locate_model_file(project, &a.model)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());

    // Idempotency: refuse if the field already exists in the struct.
    let struct_bounds = find_struct_block(&current, &a.model).ok_or_else(|| {
        ExecutionError::ProjectStructure(format!(
            "apps/{app}/models.rs does not declare `pub struct {}`",
            a.model
        ))
    })?;
    let inside_struct = &current[struct_bounds.0..=struct_bounds.1];
    if struct_declares_field(inside_struct, &a.field.name) {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: format!(
                "struct {} already declares field `{}`; the plan appears to have been applied already",
                a.model, a.field.name,
            ),
        });
    }

    // Patch the file.
    let patched = patch_models_for_add_field(&current, &a.model, &a.field).map_err(|msg| {
        ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: msg,
        }
    })?;
    shadow.insert(app.clone(), patched.clone());

    // Migration file.
    let table = find_table_for_struct(&current, &a.model)
        .or_else(|| fallback_table_name(&a.model))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                a.model
            ))
        })?;
    let sql = sql_for_add_field(&table, &a.field);
    let mig_name = format!("add_{}_to_{}", a.field.name, table);
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let file_path = project.root.join("apps").join(&app).join("models.rs");
    Ok((
        vec![
            PlannedFileChange {
                path: file_path,
                kind: FileChangeKind::Update,
                new_contents: patched,
                expected_current_contents: Some(initial_source),
            },
            PlannedFileChange {
                path: mig_path,
                kind: FileChangeKind::Create,
                new_contents: sql,
                expected_current_contents: None,
            },
        ],
        format!(
            "+ Add field \"{}\" ({}{}) to model \"{}\" (migration {})",
            a.field.name,
            a.field.ty,
            if a.field.nullable { ", nullable" } else { "" },
            a.model,
            mig_filename,
        ),
    ))
}

/// 0.9.0 — materialise an `AddRelation { kind: BelongsTo, .. }` with a
/// real SQL `FOREIGN KEY` constraint.
///
/// The generated migration emits
///
///   ALTER TABLE child ADD COLUMN via INTEGER REFERENCES parent(id)
///       ON DELETE <policy>;
///
/// SQLite's `ALTER TABLE ADD COLUMN` accepts `REFERENCES` only when the
/// column is nullable (effective default `NULL`). A `required: true`
/// relation therefore cannot be materialised as a simple ALTER — it
/// has to go through the recreate-table dance. That path is the same
/// machinery the retrofit command (M3) uses, so for now the executor
/// refuses `required: true` from `ai apply` and points callers at
/// `rustio migrate --add-fks` which implements the dance once.
///
/// Referential integrity in SQLite is only enforced per-connection
/// with `PRAGMA foreign_keys = ON`. The generated migration prepends
/// that PRAGMA so operators running `rustio migrate apply` see
/// constraints take effect immediately; application runtime should
/// set the same PRAGMA on every connection (rustio's ORM already
/// does so).
///
/// Kinds other than `BelongsTo` refuse — planning them is allowed so
/// the review layer can warn, but the executor has no story for
/// `HasMany` yet (it's a virtual accessor, not a column).
fn apply_add_relation(
    r: &AddRelation,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    use crate::ai::OnDelete;
    use crate::schema::RelationKind;
    match r.kind {
        RelationKind::BelongsTo => {}
        _ => {
            return Err(ExecutionError::UnsupportedPrimitive {
                op: "add_relation",
                reason: "only `belongs_to` is materialised in 0.9.0 — `has_many` is a virtual accessor with no column change",
            });
        }
    }

    // Reject the NOT NULL + REFERENCES combination: SQLite's ALTER TABLE
    // ADD COLUMN can carry a REFERENCES clause only on a nullable column
    // (the implicit DEFAULT NULL makes existing rows satisfy the new FK
    // without a backfill). A required FK requires recreate-table, which
    // is the retrofit path.
    if r.required {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "add_relation",
            reason: "a required (NOT NULL) foreign key cannot be added via ALTER TABLE on SQLite; use `rustio migrate --add-fks` to retrofit via recreate-table",
        });
    }
    // SET NULL is only defined when the column can actually hold NULL.
    // With `required: false` this always passes; the check guards
    // against future primitive-level misuse.
    if matches!(r.on_delete, OnDelete::SetNull) && r.required {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "add_relation",
            reason: "`on_delete: set_null` requires a nullable FK column",
        });
    }

    // Patch models.rs via the same machinery as add_field — the struct
    // change is identical (an `i64` column, plus a `#[rustio(belongs_to)]`
    // attribute). We reuse apply_add_field for the file patch, then
    // *replace* its generated SQL migration with one that includes the
    // FOREIGN KEY clause.
    let synthetic = AddField {
        model: r.from.clone(),
        field: FieldSpec {
            name: r.via.clone(),
            ty: "i64".to_string(),
            nullable: true, // see above — must be nullable for ALTER TABLE
            editable: true,
        },
    };
    let (mut changes, _) = apply_add_field(&synthetic, project, shadow, migration_counter)?;

    // Find the parent's models.rs to resolve its `const TABLE`. The
    // child's table was already resolved by `apply_add_field` above, but
    // we didn't capture it — redo the cheap lookup here rather than
    // threading the result through.
    let (child_app, _) = locate_model_file(project, &r.from)?;
    let child_src = shadow
        .get(&child_app)
        .cloned()
        .unwrap_or_else(|| project.models_files[&child_app].source.clone());
    let child_table = find_table_for_struct(&child_src, &r.from)
        .or_else(|| fallback_table_name(&r.from))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for child struct `{}`",
                r.from
            ))
        })?;

    // Parent may live in a different app, or — for plans targeting a
    // model that isn't scaffolded locally — be absent entirely. In the
    // missing-parent case, use the snake-plural fallback (matches what
    // `rustio new app` produces) rather than refusing the migration.
    let parent_table = match locate_model_file(project, &r.to) {
        Ok((parent_app, parent_source)) => {
            let parent_src = shadow.get(&parent_app).cloned().unwrap_or(parent_source);
            find_table_for_struct(&parent_src, &r.to)
                .or_else(|| fallback_table_name(&r.to))
                .ok_or_else(|| {
                    ExecutionError::ProjectStructure(format!(
                        "could not find `const TABLE` for parent struct `{}`",
                        r.to
                    ))
                })?
        }
        Err(_) => fallback_table_name(&r.to).ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not derive a table name for parent struct `{}`",
                r.to
            ))
        })?,
    };

    let fk_sql = sql_for_add_fk_column(&child_table, &r.via, &parent_table, r.on_delete);
    let mig_filename = {
        let create = changes
            .iter_mut()
            .find(|c| c.kind == FileChangeKind::Create)
            .expect("apply_add_field always plans a Create for the migration");
        create.new_contents = fk_sql;
        create
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };

    Ok((
        changes,
        format!(
            "+ Add relation `{}` from \"{}\" to \"{}\" (belongs_to → {}, {}, migration {})",
            r.via,
            r.from,
            r.to,
            parent_table,
            r.on_delete.as_str(),
            mig_filename,
        ),
    ))
}

/// 0.9.0 — emit the SQL for adding a nullable FK column to an existing
/// table. The `PRAGMA foreign_keys = ON` is repeated in the migration
/// so operators who run migrations via `sqlite3` directly (not
/// `rustio migrate apply`) still see the constraint enforced.
fn sql_for_add_fk_column(
    child_table: &str,
    via: &str,
    parent_table: &str,
    on_delete: crate::ai::OnDelete,
) -> String {
    format!(
        "-- Generated by rustio ai apply (0.9.0). DO NOT EDIT.\n\
         PRAGMA foreign_keys = ON;\n\
         ALTER TABLE {child} ADD COLUMN {via} INTEGER REFERENCES {parent}(id) {policy};\n",
        child = child_table,
        via = via,
        parent = parent_table,
        policy = on_delete.sql(),
    )
}

// ---------------------------------------------------------------------------
// 0.9.1 — remove_field / remove_relation (gated on `allow_destructive`)
// ---------------------------------------------------------------------------

/// 0.9.1 — materialise a `RemoveField` primitive.
///
/// SQLite's `ALTER TABLE` has supported `DROP COLUMN` since 3.35, but
/// the clause refuses when the column participates in a foreign key or
/// is referenced by an index — and rustio projects routinely have both.
/// Use the same recreate-table pattern as `change_field_type`: clone
/// the table with the field omitted, copy data across, drop, rename.
/// The recreate preserves every remaining field's relation metadata
/// (`column_def_with_relation_context`), so other FKs on the table
/// survive the migration unchanged.
///
/// Caller has already verified `opts.allow_destructive` — this function
/// assumes the reviewer has consented.
fn apply_remove_field(
    r: &RemoveField,
    schema: &Schema,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == r.model)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("model `{}` not in schema", r.model))
        })?;
    let field = model
        .fields
        .iter()
        .find(|f| f.name == r.field)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("field `{}.{}` not in schema", r.model, r.field))
        })?;
    if r.field == "id" {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "remove_field",
            reason: "cannot drop the `id` primary key; remove the model instead",
        });
    }

    let (app, initial_source) = locate_model_file(project, &r.model)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());
    let table = find_table_for_struct(&current, &r.model)
        .or_else(|| fallback_table_name(&r.model))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                r.model
            ))
        })?;

    // Patch the models.rs file.
    let patched =
        patch_models_for_remove_field(&current, &r.model, &r.field, &field.ty, field.nullable)
            .map_err(|msg| ExecutionError::FileConflict {
                path: format!("apps/{app}/models.rs"),
                reason: msg,
            })?;
    shadow.insert(app.clone(), patched.clone());

    // Build the table-rebuild SQL with the field omitted.
    let remaining: Vec<SchemaField> = model
        .fields
        .iter()
        .filter(|f| f.name != r.field)
        .cloned()
        .collect();
    // Resolve parent table names for any surviving FKs from the schema.
    let table_for = |target: &str| -> Option<String> {
        schema
            .models
            .iter()
            .find(|m| m.name == target)
            .and_then(|_| fallback_table_name(target))
    };
    let sql = generate_sqlite_recreate_table_migration_fk_aware(
        &table,
        &remaining,
        &BTreeMap::new(), // no casts — surviving columns copy 1:1
        &table_for,
    );

    let mig_name = format!("drop_{}_from_{}", r.field, table);
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let file_path = project.root.join("apps").join(&app).join("models.rs");
    let warn_line = format!(
        "    ⚠ This rewrites `{table}`. Data in `{}` is lost.",
        r.field
    );
    Ok((
        vec![
            PlannedFileChange {
                path: file_path,
                kind: FileChangeKind::Update,
                new_contents: patched,
                expected_current_contents: Some(initial_source),
            },
            PlannedFileChange {
                path: mig_path,
                kind: FileChangeKind::Create,
                new_contents: sql,
                expected_current_contents: None,
            },
        ],
        format!(
            "- Remove field `{}.{}` (migration {})\n{}",
            r.model, r.field, mig_filename, warn_line,
        ),
    ))
}

/// 0.9.1 — materialise a `RemoveRelation` primitive.
///
/// Relations are stored as a single FK column on the owning side, so
/// dropping a relation is exactly `apply_remove_field` on `r.via`. A
/// dedicated entry keeps the summary line honest ("Remove relation"
/// rather than "Remove field") and keeps callers from having to
/// synthesise a `RemoveField` themselves.
fn apply_remove_relation(
    r: &RemoveRelation,
    schema: &Schema,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    // Delegate the physical column drop to apply_remove_field.
    let synthetic = RemoveField {
        model: r.from.clone(),
        field: r.via.clone(),
    };
    let (changes, _) = apply_remove_field(&synthetic, schema, project, shadow, migration_counter)?;
    let mig_filename = changes
        .iter()
        .find(|c| c.kind == FileChangeKind::Create)
        .and_then(|c| c.path.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Ok((
        changes,
        format!(
            "- Remove relation `{}.{}` (migration {})",
            r.from, r.via, mig_filename,
        ),
    ))
}

/// 0.9.1 — like [`generate_sqlite_recreate_table_migration`] but every
/// remaining field with a `Relation` block re-emits its `REFERENCES`
/// clause on the new table so FKs survive the drop-field operation.
fn generate_sqlite_recreate_table_migration_fk_aware(
    table: &str,
    new_fields: &[SchemaField],
    source_exprs: &BTreeMap<String, String>,
    table_for: &impl Fn(&str) -> Option<String>,
) -> String {
    let new_table = format!("{table}__new");
    let mut out = String::new();
    out.push_str("-- Generated by rustio ai apply (0.9.1). DO NOT EDIT.\n");
    out.push_str("-- SQLite recreate-table, FK-aware: surviving foreign keys are\n");
    out.push_str("-- re-emitted on the rebuilt table.\n");
    out.push_str("PRAGMA foreign_keys = OFF;\n");
    out.push_str("BEGIN;\n\n");
    out.push_str(&format!("CREATE TABLE {new_table} (\n"));
    for (i, f) in new_fields.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&column_def_with_relation_context(f, table_for));
        if i + 1 < new_fields.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(");\n\n");
    let col_list = new_fields
        .iter()
        .map(|f| f.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let expr_list = new_fields
        .iter()
        .map(|f| {
            source_exprs
                .get(&f.name)
                .cloned()
                .unwrap_or_else(|| f.name.clone())
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "INSERT INTO {new_table} ({col_list}) SELECT {expr_list} FROM {table};\n\n"
    ));
    out.push_str(&format!("DROP TABLE {table};\n"));
    out.push_str(&format!("ALTER TABLE {new_table} RENAME TO {table};\n\n"));
    out.push_str("COMMIT;\n");
    out.push_str("PRAGMA foreign_keys = ON;\n");
    out
}

fn apply_rename_field(
    r: &RenameField,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    let (app, initial_source) = locate_model_file(project, &r.model)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());

    let struct_bounds = find_struct_block(&current, &r.model).ok_or_else(|| {
        ExecutionError::ProjectStructure(format!(
            "apps/{app}/models.rs does not declare `pub struct {}`",
            r.model
        ))
    })?;
    let inside_struct = &current[struct_bounds.0..=struct_bounds.1];
    if !struct_declares_field(inside_struct, &r.from) {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: format!(
                "struct {} does not declare `pub {}: …`; rename cannot proceed",
                r.model, r.from,
            ),
        });
    }
    if struct_declares_field(inside_struct, &r.to) {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: format!(
                "struct {} already has a field called `{}`; rename target is taken",
                r.model, r.to,
            ),
        });
    }

    let patched =
        patch_models_for_rename_field(&current, &r.model, &r.from, &r.to).map_err(|msg| {
            ExecutionError::FileConflict {
                path: format!("apps/{app}/models.rs"),
                reason: msg,
            }
        })?;
    shadow.insert(app.clone(), patched.clone());

    let table = find_table_for_struct(&current, &r.model)
        .or_else(|| fallback_table_name(&r.model))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                r.model
            ))
        })?;
    let sql = format!(
        "-- Generated by rustio ai apply (0.5.2). DO NOT EDIT.\n\
         ALTER TABLE {table} RENAME COLUMN {from} TO {to};\n",
        from = r.from,
        to = r.to,
    );
    let mig_name = format!("rename_{}_to_{}_on_{}", r.from, r.to, table);
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let file_path = project.root.join("apps").join(&app).join("models.rs");
    Ok((
        vec![
            PlannedFileChange {
                path: file_path,
                kind: FileChangeKind::Update,
                new_contents: patched,
                expected_current_contents: Some(initial_source),
            },
            PlannedFileChange {
                path: mig_path,
                kind: FileChangeKind::Create,
                new_contents: sql,
                expected_current_contents: None,
            },
        ],
        format!(
            "~ Rename field \"{}.{}\" to \"{}\" (migration {})",
            r.model, r.from, r.to, mig_filename
        ),
    ))
}

// ---------------------------------------------------------------------------
// change_field_type — SQLite recreate-table
// ---------------------------------------------------------------------------

fn apply_change_field_type(
    c: &ChangeFieldType,
    schema: &Schema,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == c.model)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("model `{}` not in schema", c.model))
        })?;
    let field = model
        .fields
        .iter()
        .find(|f| f.name == c.field)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("field `{}.{}` not in schema", c.model, c.field))
        })?;

    // Idempotency: field already has target type.
    if field.ty == c.new_type {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/?/{}.rs", c.model.to_lowercase()),
            reason: format!(
                "field `{}.{}` already has type `{}`; change appears applied",
                c.model, c.field, c.new_type,
            ),
        });
    }

    // Safe-cast gate.
    let cast_expr = cast_expression(&field.ty, &c.new_type, &c.field).ok_or(
        ExecutionError::UnsupportedPrimitive {
            op: "change_field_type",
            reason: "this type conversion is not in the 0.5.3 safe-cast set",
        },
    )?;

    let (app, initial_source) = locate_model_file(project, &c.model)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());
    let table = find_table_for_struct(&current, &c.model)
        .or_else(|| fallback_table_name(&c.model))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                c.model
            ))
        })?;

    if table_has_any_foreign_key(project, &table) {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "change_field_type",
            reason:
                "table has foreign-key constraints (incoming or outgoing); SQLite recreate-table would break them — scheduled for 0.6.0",
        });
    }

    let patched = patch_models_for_change_field_type(
        &current,
        &c.model,
        &c.field,
        &field.ty,
        &c.new_type,
        field.nullable,
    )
    .map_err(|msg| ExecutionError::FileConflict {
        path: format!("apps/{app}/models.rs"),
        reason: msg,
    })?;
    shadow.insert(app.clone(), patched.clone());

    // Build the new column list (same order as current schema).
    let new_fields: Vec<SchemaField> = model
        .fields
        .iter()
        .map(|f| {
            if f.name == c.field {
                SchemaField {
                    ty: c.new_type.clone(),
                    ..f.clone()
                }
            } else {
                f.clone()
            }
        })
        .collect();

    let mut source_exprs: BTreeMap<String, String> = BTreeMap::new();
    source_exprs.insert(c.field.clone(), cast_expr);
    let sql = generate_sqlite_recreate_table_migration(&table, &new_fields, &source_exprs);

    let mig_name = format!("change_{}_type_on_{}", c.field, table);
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let file_path = project.root.join("apps").join(&app).join("models.rs");
    let warn_line =
        format!("    ⚠ This rewrites the entire `{table}` table. Large tables may cause downtime.");
    Ok((
        vec![
            PlannedFileChange {
                path: file_path,
                kind: FileChangeKind::Update,
                new_contents: patched,
                expected_current_contents: Some(initial_source),
            },
            PlannedFileChange {
                path: mig_path,
                kind: FileChangeKind::Create,
                new_contents: sql,
                expected_current_contents: None,
            },
        ],
        format!(
            "~ Change type of {}.{} from {} to {} (migration {})\n{}",
            c.model, c.field, field.ty, c.new_type, mig_filename, warn_line,
        ),
    ))
}

/// Decide whether `old_ty` → `new_ty` is a safe SQLite cast, and return
/// the SQL expression that performs it against the source column.
/// `None` means "not in the safe-cast set" — callers refuse with
/// `UnsupportedPrimitive`.
fn cast_expression(old_ty: &str, new_ty: &str, col_name: &str) -> Option<String> {
    match (old_ty, new_ty) {
        // Same type — caller should have bailed out earlier.
        (a, b) if a == b => None,
        // SQLite stores i32 / i64 / bool as INTEGER; no cast needed.
        ("i32", "i64") | ("i64", "i32") => Some(col_name.to_string()),
        ("bool", "i32") | ("bool", "i64") | ("i32", "bool") | ("i64", "bool") => {
            Some(col_name.to_string())
        }
        // SQLite stores DateTime as TEXT; no cast needed either way.
        ("DateTime", "String") | ("String", "DateTime") => Some(col_name.to_string()),
        // Safe widening to TEXT.
        ("i32", "String") | ("i64", "String") | ("bool", "String") => {
            Some(format!("CAST({col_name} AS TEXT)"))
        }
        // Narrowing to INTEGER — explicit cast; review warns that
        // non-numeric text becomes 0.
        ("String", "i32") | ("String", "i64") | ("String", "bool") => {
            Some(format!("CAST({col_name} AS INTEGER)"))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// change_field_nullability — SQLite recreate-table
// ---------------------------------------------------------------------------

fn apply_change_field_nullability(
    c: &ChangeFieldNullability,
    schema: &Schema,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == c.model)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("model `{}` not in schema", c.model))
        })?;
    let field = model
        .fields
        .iter()
        .find(|f| f.name == c.field)
        .ok_or_else(|| {
            ExecutionError::SchemaMismatch(format!("field `{}.{}` not in schema", c.model, c.field))
        })?;

    if field.nullable == c.nullable {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/?/{}.rs", c.model.to_lowercase()),
            reason: format!(
                "field `{}.{}` is already {}; change appears applied",
                c.model,
                c.field,
                if c.nullable { "nullable" } else { "required" }
            ),
        });
    }

    let (app, initial_source) = locate_model_file(project, &c.model)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());
    let table = find_table_for_struct(&current, &c.model)
        .or_else(|| fallback_table_name(&c.model))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                c.model
            ))
        })?;
    if table_has_any_foreign_key(project, &table) {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "change_field_nullability",
            reason:
                "table has foreign-key constraints; SQLite recreate-table would break them — scheduled for 0.6.0",
        });
    }

    let patched = patch_models_for_change_nullability(
        &current,
        &c.model,
        &c.field,
        &field.ty,
        field.nullable,
        c.nullable,
    )
    .map_err(|msg| ExecutionError::FileConflict {
        path: format!("apps/{app}/models.rs"),
        reason: msg,
    })?;
    shadow.insert(app.clone(), patched.clone());

    let new_fields: Vec<SchemaField> = model
        .fields
        .iter()
        .map(|f| {
            if f.name == c.field {
                SchemaField {
                    nullable: c.nullable,
                    ..f.clone()
                }
            } else {
                f.clone()
            }
        })
        .collect();

    // When tightening (nullable → required), replace NULL source rows
    // with the type default via COALESCE. When relaxing, straight copy.
    let mut source_exprs: BTreeMap<String, String> = BTreeMap::new();
    let tightening = !c.nullable && field.nullable;
    if tightening {
        source_exprs.insert(
            c.field.clone(),
            format!(
                "COALESCE({col}, {dflt})",
                col = c.field,
                dflt = safe_default_literal(&field.ty)
            ),
        );
    }
    let sql = generate_sqlite_recreate_table_migration(&table, &new_fields, &source_exprs);

    let mig_name = format!("change_{}_nullability_on_{}", c.field, table);
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let state = if c.nullable { "nullable" } else { "required" };
    let warn_line = if tightening {
        format!(
            "    ⚠ This rewrites `{table}` and substitutes existing NULLs with the type default ({}).",
            safe_default_literal(&field.ty)
        )
    } else {
        format!("    ⚠ This rewrites the entire `{table}` table. Large tables may cause downtime.")
    };

    let file_path = project.root.join("apps").join(&app).join("models.rs");
    Ok((
        vec![
            PlannedFileChange {
                path: file_path,
                kind: FileChangeKind::Update,
                new_contents: patched,
                expected_current_contents: Some(initial_source),
            },
            PlannedFileChange {
                path: mig_path,
                kind: FileChangeKind::Create,
                new_contents: sql,
                expected_current_contents: None,
            },
        ],
        format!(
            "~ Mark {}.{} as {} (migration {})\n{}",
            c.model, c.field, state, mig_filename, warn_line
        ),
    ))
}

// ---------------------------------------------------------------------------
// rename_model — full: struct, TABLE const, admin.rs, views.rs (bounded)
// ---------------------------------------------------------------------------

fn apply_rename_model(
    r: &RenameModel,
    project: &ProjectView,
    shadow: &mut BTreeMap<String, String>,
    migration_counter: &mut u32,
) -> Result<(Vec<PlannedFileChange>, String), ExecutionError> {
    let (app, initial_source) = locate_model_file(project, &r.from)?;
    let current = shadow
        .get(&app)
        .cloned()
        .unwrap_or_else(|| initial_source.clone());

    // Idempotency.
    let struct_names = parse_struct_names(&current);
    if struct_names.iter().any(|n| n == &r.to) {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: format!(
                "struct `{}` already exists in this file; rename appears applied",
                r.to
            ),
        });
    }
    if !struct_names.iter().any(|n| n == &r.from) {
        return Err(ExecutionError::FileConflict {
            path: format!("apps/{app}/models.rs"),
            reason: format!("struct `{}` not found — nothing to rename", r.from),
        });
    }

    let old_table = find_table_for_struct(&current, &r.from)
        .or_else(|| fallback_table_name(&r.from))
        .ok_or_else(|| {
            ExecutionError::ProjectStructure(format!(
                "could not find `const TABLE` for struct `{}`",
                r.from
            ))
        })?;
    let new_table = fallback_table_name(&r.to).unwrap_or_else(|| old_table.clone());

    if table_has_any_foreign_key(project, &old_table) {
        return Err(ExecutionError::UnsupportedPrimitive {
            op: "rename_model",
            reason:
                "table has foreign-key constraints (incoming or outgoing); FK rewriting is scheduled for 0.6.0",
        });
    }

    // Patch models.rs.
    let patched_models = patch_models_for_rename_model(
        &current, &r.from, &r.to, &old_table, &new_table,
    )
    .map_err(|msg| ExecutionError::FileConflict {
        path: format!("apps/{app}/models.rs"),
        reason: msg,
    })?;
    shadow.insert(app.clone(), patched_models.clone());

    // Patch admin.rs (required — the app must re-register the model).
    let admin_path = project.root.join("apps").join(&app).join("admin.rs");
    let admin_source =
        std::fs::read_to_string(&admin_path).map_err(|e| ExecutionError::IoError {
            path: admin_path.display().to_string(),
            message: e.to_string(),
        })?;
    let admin_patched =
        patch_admin_for_rename_model(&admin_source, &r.from, &r.to).map_err(|msg| {
            ExecutionError::FileConflict {
                path: admin_path.display().to_string(),
                reason: msg,
            }
        })?;

    // Patch views.rs best-effort (identifier boundaries only). Only
    // emit a change if the file exists and actually contains the old
    // name as a standalone identifier.
    let views_path = project.root.join("apps").join(&app).join("views.rs");
    let views_change: Option<PlannedFileChange> = if views_path.is_file() {
        let views_source =
            std::fs::read_to_string(&views_path).map_err(|e| ExecutionError::IoError {
                path: views_path.display().to_string(),
                message: e.to_string(),
            })?;
        let patched_views = rename_identifier_bounded(&views_source, &r.from, &r.to);
        if patched_views != views_source {
            Some(PlannedFileChange {
                path: views_path,
                kind: FileChangeKind::Update,
                new_contents: patched_views,
                expected_current_contents: Some(views_source),
            })
        } else {
            None
        }
    } else {
        None
    };

    let sql = format!(
        "-- Generated by rustio ai apply (0.5.3). DO NOT EDIT.\n\
         ALTER TABLE {old_table} RENAME TO {new_table};\n"
    );
    let mig_name = format!("rename_{old_table}_to_{new_table}");
    let (mig_path, mig_filename) = new_migration_path(project, *migration_counter, &mig_name);
    *migration_counter += 1;

    let mut changes: Vec<PlannedFileChange> = vec![
        PlannedFileChange {
            path: project.root.join("apps").join(&app).join("models.rs"),
            kind: FileChangeKind::Update,
            new_contents: patched_models,
            expected_current_contents: Some(initial_source),
        },
        PlannedFileChange {
            path: admin_path,
            kind: FileChangeKind::Update,
            new_contents: admin_patched,
            expected_current_contents: Some(admin_source),
        },
    ];
    if let Some(vc) = views_change {
        changes.push(vc);
    }
    changes.push(PlannedFileChange {
        path: mig_path,
        kind: FileChangeKind::Create,
        new_contents: sql,
        expected_current_contents: None,
    });

    Ok((
        changes,
        format!(
            "~ Rename model \"{from}\" to \"{to}\" (migration {mig})\n\
             \x20   ⚠ Table renamed from `{old_table}` to `{new_table}`. User code using `{from}` outside apps/{app}/ must be updated manually.",
            from = r.from,
            to = r.to,
            mig = mig_filename,
        ),
    ))
}

// ---------------------------------------------------------------------------
// SQLite recreate-table helper
// ---------------------------------------------------------------------------

/// Build the standard SQLite recreate-table migration for an in-place
/// schema change: create a `<table>__new` with the target shape, copy
/// rows via `INSERT … SELECT …` (with per-column expressions for type
/// casts or nullability defaults), drop the old table, rename the new
/// one back. This is the *only* pattern SQLite supports for changes
/// that ALTER TABLE won't accept.
///
/// Caller guarantees: `new_fields` contains every column the target
/// table should have (preserving order). Columns missing from
/// `source_exprs` are copied by name from the old table.
fn generate_sqlite_recreate_table_migration(
    table: &str,
    new_fields: &[SchemaField],
    source_exprs: &BTreeMap<String, String>,
) -> String {
    let new_table = format!("{table}__new");
    let mut out = String::new();
    out.push_str("-- Generated by rustio ai apply (0.5.3). DO NOT EDIT.\n");
    out.push_str("-- SQLite recreate-table pattern: SQLite cannot ALTER COLUMN in place.\n");
    out.push_str(&format!("CREATE TABLE {new_table} (\n"));
    for (i, f) in new_fields.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&column_def(f));
        if i + 1 < new_fields.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(");\n\n");

    let col_list = new_fields
        .iter()
        .map(|f| f.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let expr_list = new_fields
        .iter()
        .map(|f| {
            source_exprs
                .get(&f.name)
                .cloned()
                .unwrap_or_else(|| f.name.clone())
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "INSERT INTO {new_table} ({col_list})\nSELECT {expr_list}\nFROM {table};\n\n"
    ));
    out.push_str(&format!("DROP TABLE {table};\n\n"));
    out.push_str(&format!("ALTER TABLE {new_table} RENAME TO {table};\n"));
    out
}

/// One column DDL for recreate-table. `id INTEGER PRIMARY KEY
/// AUTOINCREMENT` is the special case every scaffolded table uses.
///
/// 0.9.0: if the field carries a `Relation` with an `on_delete` policy,
/// a trailing `REFERENCES <parent>(id) ON DELETE <policy>` is appended.
/// The caller is responsible for passing a schema that has those
/// populated — schema files from 0.8.x have `on_delete: None` and so
/// the FK clause is skipped (backward-compat: old schemas produce the
/// same output they always have).
fn column_def(f: &SchemaField) -> String {
    column_def_with_relation_context(f, fallback_table_name)
}

fn column_def_with_relation_context(
    f: &SchemaField,
    resolve_parent_table: impl Fn(&str) -> Option<String>,
) -> String {
    let sql_ty = sql_type_for(&f.ty);
    if f.name == "id" && f.ty == "i64" && !f.nullable {
        return "id INTEGER PRIMARY KEY AUTOINCREMENT".to_string();
    }
    let suffix = if f.nullable {
        String::new()
    } else {
        format!(" NOT NULL DEFAULT {}", safe_default_literal(&f.ty))
    };
    let fk_clause = f
        .relation
        .as_ref()
        .and_then(|rel| {
            let policy = rel.on_delete.as_deref()?;
            let parent_table = resolve_parent_table(&rel.model)?;
            Some(format!(
                " REFERENCES {parent}({target_col}) ON DELETE {policy}",
                parent = parent_table,
                target_col = if rel.field.is_empty() {
                    "id"
                } else {
                    &rel.field
                },
                policy = policy.to_uppercase().replace('_', " "),
            ))
        })
        .unwrap_or_default();
    format!("{} {}{}{}", f.name, sql_ty, suffix, fk_clause)
}

/// Refuse recreate-table on any table that participates in foreign
/// keys — outgoing (declared inside the table) or incoming (referenced
/// by another table). The recreate pattern DROPs the table, which
/// would cascade-delete dependent rows under PRAGMA foreign_keys=ON.
/// 0.6.0 is scheduled to handle FK rewriting.
fn table_has_any_foreign_key(project: &ProjectView, table: &str) -> bool {
    let lt = table.to_lowercase();
    for contents in project.migration_sources.values() {
        let c = contents.to_lowercase();
        // Incoming reference from any migration.
        if c.contains(&format!("references {lt}")) || c.contains(&format!("references {lt}(")) {
            return true;
        }
        // Outgoing FK in this table's own CREATE.
        let create_needles = [
            format!("create table {lt} ("),
            format!("create table if not exists {lt} ("),
        ];
        for needle in &create_needles {
            if let Some(start) = c.find(needle) {
                let tail = &c[start..];
                if let Some(end) = tail.find(");") {
                    if tail[..end].contains("foreign key") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Replace `from` with `to` only at identifier boundaries (byte before
/// and after are not identifier chars). Used for the bounded rename
/// sweep in `views.rs` so we don't clobber substrings inside string
/// literals or comments that happen to contain the old name.
fn rename_identifier_bounded(src: &str, from: &str, to: &str) -> String {
    let bytes = src.as_bytes();
    let from_bytes = from.as_bytes();
    let n = from_bytes.len();
    if n == 0 {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut last = 0;
    while i + n <= bytes.len() {
        if &bytes[i..i + n] == from_bytes {
            let left_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let right_ok = i + n == bytes.len() || !is_ident_byte(bytes[i + n]);
            if left_ok && right_ok {
                out.push_str(&src[last..i]);
                out.push_str(to);
                i += n;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&src[last..]);
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Shadow-apply a primitive to a schema copy so later steps in the
/// same plan see the earlier step's shape change. Mirrors the review
/// layer's logic but is executor-internal so we aren't coupled to
/// review's visibility rules.
fn apply_schema_shadow(p: &Primitive, schema: &mut Schema) {
    match p {
        Primitive::AddField(a) => {
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == a.model) {
                m.fields.push(SchemaField {
                    name: a.field.name.clone(),
                    ty: a.field.ty.clone(),
                    nullable: a.field.nullable,
                    editable: a.field.editable,
                    relation: None,
                });
            }
        }
        Primitive::RenameField(r) => {
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == r.model) {
                if let Some(f) = m.fields.iter_mut().find(|f| f.name == r.from) {
                    f.name = r.to.clone();
                }
            }
        }
        Primitive::ChangeFieldType(c) => {
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == c.model) {
                if let Some(f) = m.fields.iter_mut().find(|f| f.name == c.field) {
                    f.ty = c.new_type.clone();
                }
            }
        }
        Primitive::ChangeFieldNullability(c) => {
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == c.model) {
                if let Some(f) = m.fields.iter_mut().find(|f| f.name == c.field) {
                    f.nullable = c.nullable;
                }
            }
        }
        Primitive::RenameModel(r) => {
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == r.from) {
                m.name = r.to.clone();
            }
        }
        Primitive::AddRelation(r) => {
            use crate::schema::{Relation, RelationKind};
            if !matches!(r.kind, RelationKind::BelongsTo) {
                return;
            }
            if let Some(m) = schema.models.iter_mut().find(|m| m.name == r.from) {
                if m.fields.iter().any(|f| f.name == r.via) {
                    return;
                }
                m.fields.push(SchemaField {
                    name: r.via.clone(),
                    ty: "i64".to_string(),
                    nullable: !r.required,
                    editable: true,
                    relation: Some(Relation {
                        model: r.to.clone(),
                        field: "id".to_string(),
                        kind: RelationKind::BelongsTo,
                        // AI-planner-authored relations don't declare a
                        // display field: the admin renders `#<id>` until
                        // the model author opts in via the macro.
                        display_field: None,
                        // 0.9.0 — propagate FK metadata into the schema
                        // snapshot so `rustio.schema.json` reflects what
                        // the migration just committed.
                        required: Some(r.required),
                        on_delete: Some(r.on_delete.as_str().to_string()),
                    }),
                });
            }
        }
        _ => {}
    }
}

/// Return a human-readable reason string if `step` violates a policy
/// under the given context; `None` means the step is allowed.
/// Conservative by design — the list grows only as each rule is
/// justified.
fn policy_violation_for(step: &Primitive, pii: &[&str], ctx: &ContextConfig) -> Option<String> {
    let ctx_tag = {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = &ctx.country {
            parts.push(format!("country={c}"));
        }
        if let Some(i) = &ctx.industry {
            parts.push(format!("industry={i}"));
        }
        if ctx.requires_gdpr() {
            parts.push("GDPR".to_string());
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        }
    };
    match step {
        Primitive::RemoveField(r) if pii.iter().any(|p| *p == r.field) => Some(format!(
            "refusing to remove `{}.{}` — it is personally-identifying data under the project context{}. Change the context or update the plan by hand.",
            r.model, r.field, ctx_tag,
        )),
        Primitive::ChangeFieldType(c) if pii.iter().any(|p| *p == c.field) => Some(format!(
            "refusing to change the type of `{}.{}` — it is personally-identifying data under the project context{}; retention / hashing pipelines depend on the stored shape.",
            c.model, c.field, ctx_tag,
        )),
        Primitive::RenameField(r) if pii.iter().any(|p| *p == r.from) => Some(format!(
            "refusing to rename `{}.{}` — it is personally-identifying data under the project context{}; audit trails keyed on the old name would break.",
            r.model, r.from, ctx_tag,
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// models.rs patching
// ---------------------------------------------------------------------------

/// 0.9.1 — inverse of [`patch_models_for_add_field`]. Strips a field's
/// struct declaration, its entries in `COLUMNS` / `INSERT_COLUMNS`, its
/// `from_row` accessor line, and its `insert_values` line.
///
/// Every sub-step uses exact-match substring removal and reports a
/// clean error when the expected shape isn't present. The failure mode
/// matches the add path: refuse, don't half-remove.
fn patch_models_for_remove_field(
    source: &str,
    struct_name: &str,
    field_name: &str,
    field_ty: &str,
    nullable: bool,
) -> Result<String, String> {
    let mut out = source.to_string();

    // 1. Struct field line.
    let rust_type = rust_type_for(field_ty, nullable);
    let struct_line = format!("    pub {field_name}: {rust_type},\n");
    out = replace_in_struct_literal(&out, struct_name, &struct_line, "")?;

    // 2. COLUMNS array. Scoped to the matching `impl Model for <struct>`
    // block so files with multiple models don't cross-contaminate.
    out = remove_from_str_array_scoped(&out, struct_name, "COLUMNS", field_name)?;

    // 3. INSERT_COLUMNS (not populated for auto-fields like `id`).
    // Best-effort: ignore the error if the field wasn't listed.
    if out.contains("const INSERT_COLUMNS") {
        out = remove_from_str_array_scoped(&out, struct_name, "INSERT_COLUMNS", field_name)
            .unwrap_or(out);
    }

    // 4. from_row accessor — scoped to the struct's impl block so a
    // same-named field in a sibling model isn't stripped by accident.
    let accessor = row_accessor(field_ty, nullable);
    let from_row_line = format!("            {field_name}: row.{accessor}(\"{field_name}\")?,\n",);
    out = replace_in_impl_method_literal(
        &out,
        struct_name,
        "fn from_row(",
        "Ok(Self {",
        &from_row_line,
        "",
    )?;

    // 5. insert_values line — same scoping rationale.
    let insert_line = build_insert_values_line(field_name, field_ty, nullable);
    out = replace_in_impl_method_literal(
        &out,
        struct_name,
        "fn insert_values(",
        "vec![",
        &insert_line,
        "",
    )?;

    Ok(out)
}

/// 0.9.1 — generic in-place replace inside a specific method body of a
/// specific `impl Model for <struct>` block. The method is located by
/// `fn_anchor` (e.g. `"fn from_row("`); the body delimiter is the
/// innermost brace/bracket after that, found via `body_open`
/// (`"Ok(Self {"` or `"vec!["`). Replaces the first occurrence of
/// `from` inside the body with `to`, failing cleanly if absent.
fn replace_in_impl_method_literal(
    src: &str,
    struct_name: &str,
    fn_anchor: &str,
    body_open: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let impl_anchor = format!("impl Model for {struct_name}");
    let impl_start = src
        .find(&impl_anchor)
        .ok_or_else(|| format!("could not find `{impl_anchor}`"))?;
    let impl_brace_rel = src[impl_start..]
        .find('{')
        .ok_or_else(|| format!("`{impl_anchor}` has no opening brace"))?;
    let impl_open = impl_start + impl_brace_rel;
    let impl_close = find_matching_brace(src, impl_open)
        .ok_or_else(|| format!("`{impl_anchor}` is not closed"))?;

    let block = &src[impl_open..=impl_close];
    let fn_rel = block
        .find(fn_anchor)
        .ok_or_else(|| format!("`{fn_anchor}` not found inside `{impl_anchor}`"))?;
    let body_rel_in_fn = block[fn_rel..]
        .find(body_open)
        .ok_or_else(|| format!("`{body_open}` not found after `{fn_anchor}`"))?;
    let body_open_abs = impl_open + fn_rel + body_rel_in_fn + body_open.len() - 1;
    // `body_open.len() - 1` lands on the opening bracket/brace itself.
    let body_close_abs = match src.as_bytes()[body_open_abs] {
        b'{' => find_matching_brace(src, body_open_abs),
        b'[' => find_matching_bracket(src, body_open_abs),
        _ => None,
    }
    .ok_or_else(|| format!("unterminated body after `{body_open}`"))?;

    let body = &src[body_open_abs + 1..body_close_abs];
    if !body.contains(from) {
        return Err(format!(
            "{fn_anchor} body on `{struct_name}` does not contain `{from}`"
        ));
    }
    let new_body = body.replacen(from, to, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..=body_open_abs]);
    out.push_str(&new_body);
    out.push_str(&src[body_close_abs..]);
    Ok(out)
}

/// 0.9.1 — same as [`remove_from_str_array`] but restricts the search
/// to the `impl Model for <struct>` block so files that declare
/// multiple models don't cross-contaminate.
fn remove_from_str_array_scoped(
    src: &str,
    struct_name: &str,
    const_name: &str,
    field: &str,
) -> Result<String, String> {
    let impl_anchor = format!("impl Model for {struct_name}");
    let impl_start = src
        .find(&impl_anchor)
        .ok_or_else(|| format!("could not find `{impl_anchor}`"))?;
    let brace_rel = src[impl_start..]
        .find('{')
        .ok_or_else(|| format!("`{impl_anchor}` has no opening brace"))?;
    let impl_open = impl_start + brace_rel;
    let impl_close = find_matching_brace(src, impl_open)
        .ok_or_else(|| format!("`{impl_anchor}` is not closed"))?;

    // Run the array removal only on the block's text, then splice it
    // back into the file.
    let block = &src[impl_open..=impl_close];
    let new_block = remove_from_str_array(block, const_name, field)?;
    let mut out = String::with_capacity(src.len() + new_block.len() - block.len());
    out.push_str(&src[..impl_open]);
    out.push_str(&new_block);
    out.push_str(&src[impl_close + 1..]);
    Ok(out)
}

/// 0.9.1 — drop an exact `"<field>"` entry from the named
/// `const X: &[&str]` array. Works on both single-line arrays
/// (`&["a", "b", "c"]`) and multi-line arrays (one literal per line,
/// which is what the scaffolder generates once the list grows). Handles
/// the surrounding comma so the resulting array stays valid Rust.
/// Errors cleanly if the field is not present.
fn remove_from_str_array(src: &str, const_name: &str, field: &str) -> Result<String, String> {
    let anchor = format!("const {const_name}");
    let start = src
        .find(&anchor)
        .ok_or_else(|| format!("could not find `const {const_name}`"))?;
    let rel_open = src[start..]
        .find("= &[")
        .ok_or_else(|| format!("`const {const_name}` does not use `= &[ … ]`"))?;
    let open = start + rel_open + "= &".len();
    let close = find_matching_bracket(src, open)
        .ok_or_else(|| format!("`const {const_name}` array is not closed"))?;
    let inner = &src[open + 1..close];
    let literal = format!("\"{field}\"");
    let literal_idx = inner
        .find(&literal)
        .ok_or_else(|| format!("`{const_name}` does not contain \"{field}\""))?;

    // Expand the removal slice to cover the separator surrounding the
    // literal, whether the separator is `, ` (single-line), `,\n    `
    // (multi-line interior), or a leading newline+indent followed by
    // no trailing separator (multi-line final element). The rule:
    //   - Start at the literal's first byte.
    //   - End at the next non-whitespace byte AFTER the comma that
    //     follows the literal. If there is no trailing comma (last
    //     element), rewind the start to just after the previous
    //     non-whitespace byte so we drop the leading `,` instead.
    let mut slice_start = literal_idx;
    let mut slice_end = literal_idx + literal.len();

    // Scan right looking for the next comma + any following whitespace.
    let after_literal = &inner.as_bytes()[slice_end..];
    if let Some(comma_rel) = after_literal.iter().position(|&b| !b.is_ascii_whitespace()) {
        if after_literal[comma_rel] == b',' {
            slice_end += comma_rel + 1;
            // Consume trailing whitespace after the comma too so we
            // don't leave a stranded blank line in the multi-line form.
            while slice_end < inner.len()
                && (inner.as_bytes()[slice_end] == b' ' || inner.as_bytes()[slice_end] == b'\t')
            {
                slice_end += 1;
            }
            if slice_end < inner.len() && inner.as_bytes()[slice_end] == b'\n' {
                // Multi-line case: keep exactly one newline + indent
                // before the next element by *not* consuming the \n.
                // (The leading `    ` indent of the next element was
                // already consumed by the trailing-ws loop above.)
            }
        }
    } else {
        // No trailing comma — this was the last element. Walk left
        // from the literal to the previous comma and eat it.
        let before = &inner.as_bytes()[..slice_start];
        if let Some(pos) = before.iter().rposition(|&b| !b.is_ascii_whitespace()) {
            if before[pos] == b',' {
                slice_start = pos;
            }
        }
    }

    let mut new_inner = String::with_capacity(inner.len());
    new_inner.push_str(&inner[..slice_start]);
    new_inner.push_str(&inner[slice_end..]);

    // Collapse the blank line that multi-line arrays leave behind when
    // a middle element is removed (`"x",\n        \n        "y",` →
    // `"x",\n        "y",`). The needle is `\n<horizontal-ws>\n` —
    // drop the whitespace-only line by removing the leading
    // whitespace + its trailing newline.
    let mut cursor = 0;
    while let Some(rel) = new_inner[cursor..].find('\n') {
        let pos = cursor + rel;
        let after = &new_inner[pos + 1..];
        let lead_ws = after
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        if after
            .as_bytes()
            .get(lead_ws)
            .map(|&b| b == b'\n')
            .unwrap_or(false)
        {
            let drain_end = pos + 1 + lead_ws + 1;
            new_inner.drain(pos + 1..drain_end);
            // Don't advance — the collapse might have produced a
            // second blank line.
        } else {
            cursor = pos + 1;
        }
    }

    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..=open]);
    out.push_str(&new_inner);
    out.push_str(&src[close..]);
    Ok(out)
}

fn patch_models_for_add_field(
    source: &str,
    struct_name: &str,
    field: &FieldSpec,
) -> Result<String, String> {
    let rust_type = rust_type_for(&field.ty, field.nullable);
    let mut out = source.to_string();

    // 1. Make sure chrono is imported when we're adding a DateTime.
    // Check for an actual `use chrono::` statement, not the string
    // "chrono::" anywhere — the scaffolded docstring mentions
    // `chrono::DateTime<Utc>` in comments, which would otherwise
    // fool the check and leave the file without an import.
    if field.ty == "DateTime" && !has_chrono_use(&out) {
        out = insert_chrono_import(&out);
    }

    // 2. Struct field.
    let field_line = format!("    pub {}: {},\n", field.name, rust_type);
    out = insert_before_struct_close(&out, struct_name, &field_line)?;

    // 3. COLUMNS.
    out = insert_into_str_array(&out, "COLUMNS", &field.name)?;
    // 4. INSERT_COLUMNS (best-effort; some models may skip it for
    // auto-populated fields like id).
    if out.contains("const INSERT_COLUMNS") {
        out = insert_into_str_array(&out, "INSERT_COLUMNS", &field.name)?;
    }

    // 5. from_row accessor.
    let accessor = row_accessor(&field.ty, field.nullable);
    let from_row_line = format!(
        "            {name}: row.{accessor}(\"{name}\")?,\n",
        name = field.name,
        accessor = accessor,
    );
    out = insert_before_ok_self_close(&out, &from_row_line)?;

    // 6. insert_values.
    let insert_line = build_insert_values_line(&field.name, &field.ty, field.nullable);
    out = insert_before_vec_close(&out, &insert_line)?;

    Ok(out)
}

fn patch_models_for_rename_field(
    source: &str,
    struct_name: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let mut out = source.to_string();

    // 1. Struct field name.
    out = rename_in_struct(&out, struct_name, from, to)?;

    // 2. COLUMNS + INSERT_COLUMNS — match the exact "<from>" literal.
    out = replace_in_str_array(&out, "COLUMNS", from, to)?;
    if out.contains("const INSERT_COLUMNS") {
        // INSERT_COLUMNS may not contain the field (e.g. id is excluded).
        // `replace_in_str_array` is lenient: it only rewrites on match.
        out = replace_in_str_array(&out, "INSERT_COLUMNS", from, to).unwrap_or(out);
    }

    // 3. from_row body: `<from>: row.get_X("<from>")?,` → `<to>: row.get_X("<to>")?,`
    out = rename_in_from_row(&out, from, to)?;

    // 4. insert_values body: `self.<from>` → `self.<to>`
    out = rename_in_insert_values(&out, from, to)?;

    Ok(out)
}

/// Rewrite the struct field declaration, the `from_row` accessor, and
/// (for `String`) the `.clone()` call in `insert_values` so the Rust
/// side matches the new column type.
fn patch_models_for_change_field_type(
    source: &str,
    struct_name: &str,
    field_name: &str,
    old_ty: &str,
    new_ty: &str,
    nullable: bool,
) -> Result<String, String> {
    let mut out = source.to_string();
    // Ensure chrono import when introducing DateTime.
    if (new_ty == "DateTime") && !has_chrono_use(&out) {
        out = insert_chrono_import(&out);
    }
    // 1. Struct field line.
    let old_rust = rust_type_for(old_ty, nullable);
    let new_rust = rust_type_for(new_ty, nullable);
    out = replace_in_struct_literal(
        &out,
        struct_name,
        &format!("pub {field_name}: {old_rust},"),
        &format!("pub {field_name}: {new_rust},"),
    )?;
    // 2. from_row accessor.
    let old_acc = row_accessor(old_ty, nullable);
    let new_acc = row_accessor(new_ty, nullable);
    if old_acc != new_acc {
        out = replace_in_from_row_literal(
            &out,
            &format!("{field_name}: row.{old_acc}(\"{field_name}\")?,"),
            &format!("{field_name}: row.{new_acc}(\"{field_name}\")?,"),
        )?;
    }
    // 3. insert_values line — may gain/lose `.clone()` when moving
    // between `String` and Copy-able types.
    let old_line = build_insert_values_line(field_name, old_ty, nullable);
    let new_line = build_insert_values_line(field_name, new_ty, nullable);
    if old_line != new_line {
        let old_trim = old_line.trim().to_string();
        let new_trim = new_line.trim().to_string();
        out = replace_in_insert_values_literal(&out, &old_trim, &new_trim)?;
    }
    Ok(out)
}

/// Flip the Rust-side shape for a nullability change. Struct field type
/// gains/loses `Option<…>`; the `from_row` accessor swaps between
/// `get_X` and `get_optional_X`. `insert_values` is unchanged — the
/// `From<Option<T>> for Value` blanket handles both shapes.
fn patch_models_for_change_nullability(
    source: &str,
    struct_name: &str,
    field_name: &str,
    ty: &str,
    was_nullable: bool,
    now_nullable: bool,
) -> Result<String, String> {
    let mut out = source.to_string();
    let old_rust = rust_type_for(ty, was_nullable);
    let new_rust = rust_type_for(ty, now_nullable);
    out = replace_in_struct_literal(
        &out,
        struct_name,
        &format!("pub {field_name}: {old_rust},"),
        &format!("pub {field_name}: {new_rust},"),
    )?;
    let old_acc = row_accessor(ty, was_nullable);
    let new_acc = row_accessor(ty, now_nullable);
    out = replace_in_from_row_literal(
        &out,
        &format!("{field_name}: row.{old_acc}(\"{field_name}\")?,"),
        &format!("{field_name}: row.{new_acc}(\"{field_name}\")?,"),
    )?;
    Ok(out)
}

/// Update `models.rs` for a model rename: the struct name, the
/// `impl Model for …` header, and the `TABLE` const.
fn patch_models_for_rename_model(
    source: &str,
    old_struct: &str,
    new_struct: &str,
    old_table: &str,
    new_table: &str,
) -> Result<String, String> {
    let mut out = source.to_string();

    let old_struct_decl = format!("pub struct {old_struct}");
    let new_struct_decl = format!("pub struct {new_struct}");
    if !out.contains(&old_struct_decl) {
        return Err(format!("struct `{old_struct}` not found"));
    }
    out = out.replacen(&old_struct_decl, &new_struct_decl, 1);

    let old_impl = format!("impl Model for {old_struct}");
    let new_impl = format!("impl Model for {new_struct}");
    if out.contains(&old_impl) {
        out = out.replacen(&old_impl, &new_impl, 1);
    }

    let old_tbl = format!("const TABLE: &'static str = \"{old_table}\";");
    let new_tbl = format!("const TABLE: &'static str = \"{new_table}\";");
    if out.contains(&old_tbl) {
        out = out.replacen(&old_tbl, &new_tbl, 1);
    }
    Ok(out)
}

/// Update `admin.rs` for a model rename: `use super::models::Old;`
/// and `admin.model::<Old>()`.
fn patch_admin_for_rename_model(
    source: &str,
    old_struct: &str,
    new_struct: &str,
) -> Result<String, String> {
    let mut out = source.to_string();
    let old_use = format!("use super::models::{old_struct};");
    let new_use = format!("use super::models::{new_struct};");
    if out.contains(&old_use) {
        out = out.replacen(&old_use, &new_use, 1);
    }
    let old_call = format!("admin.model::<{old_struct}>()");
    let new_call = format!("admin.model::<{new_struct}>()");
    if !out.contains(&old_call) {
        return Err(format!(
            "`admin.rs` does not call `admin.model::<{old_struct}>()`"
        ));
    }
    out = out.replacen(&old_call, &new_call, 1);
    Ok(out)
}

// --- tiny, targeted source-patching primitives ------------------------------

/// Replace a literal substring inside the named struct block only.
/// Used by change-type / nullability patchers where we need the old
/// string to be matched exactly rather than by field-name heuristics.
fn replace_in_struct_literal(
    src: &str,
    struct_name: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let (open, close) = find_struct_block(src, struct_name)
        .ok_or_else(|| format!("struct `{struct_name}` block not found"))?;
    let block = &src[open..=close];
    if !block.contains(from) {
        return Err(format!("struct `{struct_name}` does not contain `{from}`"));
    }
    let new_block = block.replacen(from, to, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..open]);
    out.push_str(&new_block);
    out.push_str(&src[close + 1..]);
    Ok(out)
}

fn replace_in_from_row_literal(src: &str, from: &str, to: &str) -> Result<String, String> {
    let fn_start = src
        .find("fn from_row(")
        .ok_or_else(|| "`fn from_row(` not found".to_string())?;
    let ok_self_rel = src[fn_start..]
        .find("Ok(Self {")
        .ok_or_else(|| "`Ok(Self {` not found".to_string())?;
    let ok_self_open = fn_start + ok_self_rel + "Ok(Self ".len();
    let ok_self_close = find_matching_brace(src, ok_self_open)
        .ok_or_else(|| "`Ok(Self { … }` is not closed".to_string())?;
    let block = &src[ok_self_open..=ok_self_close];
    if !block.contains(from) {
        return Err(format!("from_row does not contain `{from}`"));
    }
    let replaced = block.replacen(from, to, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..ok_self_open]);
    out.push_str(&replaced);
    out.push_str(&src[ok_self_close + 1..]);
    Ok(out)
}

fn replace_in_insert_values_literal(src: &str, from: &str, to: &str) -> Result<String, String> {
    let fn_start = src
        .find("fn insert_values(")
        .ok_or_else(|| "`fn insert_values(` not found".to_string())?;
    let vec_rel = src[fn_start..]
        .find("vec![")
        .ok_or_else(|| "no `vec![` inside insert_values".to_string())?;
    let vec_open = fn_start + vec_rel + 4;
    let vec_close = find_matching_bracket(src, vec_open)
        .ok_or_else(|| "`vec![ … ]` is not closed".to_string())?;
    let block = &src[vec_open..=vec_close];
    if !block.contains(from) {
        return Err(format!("insert_values does not contain `{from}`"));
    }
    let replaced = block.replacen(from, to, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..vec_open]);
    out.push_str(&replaced);
    out.push_str(&src[vec_close + 1..]);
    Ok(out)
}

fn find_struct_block(src: &str, name: &str) -> Option<(usize, usize)> {
    let anchor = format!("pub struct {name}");
    let start = src.find(&anchor)?;
    // Guard against substring matches (`TaskExtra` when looking for `Task`):
    // the next char after the name must be whitespace or `{` or `<`.
    let after_name = start + anchor.len();
    match src.as_bytes().get(after_name)? {
        b' ' | b'{' | b'\t' | b'\n' | b'<' => {}
        _ => return None,
    }
    let open = start + src[start..].find('{')?;
    let close = find_matching_brace(src, open)?;
    Some((open, close))
}

fn find_matching_brace(src: &str, open_idx: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if *bytes.get(open_idx)? != b'{' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn struct_declares_field(inside_struct: &str, field_name: &str) -> bool {
    // Match `pub <field>:` or `pub <field> :`. Line-scoped.
    for line in inside_struct.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("pub ") {
            let rest = rest.trim_start();
            // Identifier then optional whitespace then ":"
            let mut chars = rest.chars();
            let mut ident = String::new();
            for ch in chars.by_ref() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ident.push(ch);
                } else {
                    break;
                }
            }
            if ident == field_name {
                let rest = rest.trim_start_matches(&ident[..]).trim_start();
                if rest.starts_with(':') {
                    return true;
                }
            }
        }
    }
    false
}

fn insert_before_struct_close(
    src: &str,
    struct_name: &str,
    new_line: &str,
) -> Result<String, String> {
    let (_open, close) = find_struct_block(src, struct_name)
        .ok_or_else(|| format!("could not locate `pub struct {struct_name}` block"))?;
    insert_before_brace(src, close, new_line)
}

fn insert_before_ok_self_close(src: &str, new_line: &str) -> Result<String, String> {
    // Find "Ok(Self {" inside a `fn from_row` body — simple string
    // search is good enough because the token is distinctive in the
    // scaffold template. Refuse if we see more than one occurrence.
    let needle = "Ok(Self {";
    let first = src
        .find(needle)
        .ok_or_else(|| "could not locate `Ok(Self {` in from_row".to_string())?;
    if src[first + needle.len()..].contains(needle) {
        return Err("multiple `Ok(Self {` in file; refusing to choose".into());
    }
    let open = first + needle.len() - 1; // index of `{`
    let close = find_matching_brace(src, open)
        .ok_or_else(|| "`Ok(Self { … }` is not closed".to_string())?;
    insert_before_brace(src, close, new_line)
}

fn insert_before_vec_close(src: &str, new_line: &str) -> Result<String, String> {
    // Find `fn insert_values(` then `vec![` then the matching `]`.
    let fn_idx = src
        .find("fn insert_values(")
        .ok_or_else(|| "could not locate `fn insert_values(`".to_string())?;
    let vec_rel = src[fn_idx..]
        .find("vec![")
        .ok_or_else(|| "no `vec![` inside `insert_values`".to_string())?;
    let vec_open = fn_idx + vec_rel + 4; // index of `[`
    let close = find_matching_bracket(src, vec_open)
        .ok_or_else(|| "`vec![ … ]` is not closed".to_string())?;
    insert_before_bracket(src, close, new_line)
}

fn find_matching_bracket(src: &str, open_idx: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if *bytes.get(open_idx)? != b'[' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut i = open_idx;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn insert_before_brace(src: &str, close: usize, new_line: &str) -> Result<String, String> {
    let before = &src[..close];
    let last_nl = before.rfind('\n').ok_or_else(|| {
        "refusing to patch single-line `{ … }`: file layout is outside the 0.5.2 safe subset"
            .to_string()
    })?;
    let mut out = String::with_capacity(src.len() + new_line.len());
    out.push_str(&src[..=last_nl]);
    out.push_str(new_line);
    if !new_line.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&src[last_nl + 1..]);
    Ok(out)
}

fn insert_before_bracket(src: &str, close: usize, new_line: &str) -> Result<String, String> {
    let before = &src[..close];
    let last_nl = before.rfind('\n').ok_or_else(|| {
        "refusing to patch single-line `vec![ … ]`: outside the safe subset".to_string()
    })?;
    let mut out = String::with_capacity(src.len() + new_line.len());
    out.push_str(&src[..=last_nl]);
    out.push_str(new_line);
    if !new_line.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&src[last_nl + 1..]);
    Ok(out)
}

fn insert_into_str_array(src: &str, const_name: &str, column: &str) -> Result<String, String> {
    let anchor = format!("const {const_name}");
    let start = src
        .find(&anchor)
        .ok_or_else(|| format!("could not find `const {const_name}`"))?;
    // Skip past the type annotation (e.g. `&'static [&'static str]`) to
    // the literal `= &[ … ]`. Looking for `= &[` is precise enough for
    // the scaffold's layout and refuses exotic formats loudly.
    let rel_open = src[start..]
        .find("= &[")
        .ok_or_else(|| format!("`const {const_name}` does not use the expected `= &[ … ]` form"))?;
    let open = start + rel_open + "= &".len();
    let close = find_matching_bracket(src, open)
        .ok_or_else(|| format!("`const {const_name}` array is not closed"))?;
    let inner = &src[open + 1..close];
    if inner.contains(&format!("\"{column}\"")) {
        return Err(format!(
            "`{const_name}` already contains \"{column}\"; refusing to duplicate"
        ));
    }
    // Build the new inner content.
    let trimmed = inner.trim_end_matches(|c: char| c.is_whitespace() || c == ',');
    let addition = if trimmed.trim().is_empty() {
        format!("\"{column}\"")
    } else {
        format!("{trimmed}, \"{column}\"")
    };
    // Preserve any trailing whitespace (newline indent) between the
    // last element and the closing bracket for multi-line arrays.
    let tail_ws_start = inner
        .rfind(|c: char| !c.is_whitespace() && c != ',')
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail_ws = &inner[tail_ws_start..];
    let mut out = String::with_capacity(src.len() + column.len() + 4);
    out.push_str(&src[..=open]);
    out.push_str(&addition);
    out.push_str(tail_ws);
    out.push_str(&src[close..]);
    Ok(out)
}

fn replace_in_str_array(
    src: &str,
    const_name: &str,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let anchor = format!("const {const_name}");
    let start = src
        .find(&anchor)
        .ok_or_else(|| format!("could not find `const {const_name}`"))?;
    let rel_open = src[start..]
        .find("= &[")
        .ok_or_else(|| format!("`const {const_name}` does not use the expected `= &[ … ]` form"))?;
    let open = start + rel_open + "= &".len();
    let close = find_matching_bracket(src, open)
        .ok_or_else(|| format!("`const {const_name}` array is not closed"))?;
    let inner = &src[open + 1..close];
    let from_literal = format!("\"{from}\"");
    let to_literal = format!("\"{to}\"");
    if !inner.contains(&from_literal) {
        return Err(format!(
            "`{const_name}` does not contain \"{from}\"; rename cannot proceed"
        ));
    }
    if inner.contains(&to_literal) {
        return Err(format!(
            "`{const_name}` already contains \"{to}\"; rename target is taken"
        ));
    }
    // Replace only inside the bracketed range so we don't clobber other
    // occurrences of the same string elsewhere in the file.
    let new_inner = inner.replacen(&from_literal, &to_literal, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..=open]);
    out.push_str(&new_inner);
    out.push_str(&src[close..]);
    Ok(out)
}

fn rename_in_struct(src: &str, struct_name: &str, from: &str, to: &str) -> Result<String, String> {
    let (open, close) =
        find_struct_block(src, struct_name).ok_or_else(|| "struct block not found".to_string())?;
    let block = &src[open..=close];
    let from_pattern = format!("pub {from}:");
    let to_pattern = format!("pub {to}:");
    if !block.contains(&from_pattern) {
        return Err(format!(
            "struct {struct_name} does not declare `pub {from}:`"
        ));
    }
    let new_block = block.replacen(&from_pattern, &to_pattern, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..open]);
    out.push_str(&new_block);
    out.push_str(&src[close + 1..]);
    Ok(out)
}

fn rename_in_from_row(src: &str, from: &str, to: &str) -> Result<String, String> {
    let fn_start = src
        .find("fn from_row(")
        .ok_or_else(|| "from_row not found".to_string())?;
    let ok_self_rel = src[fn_start..]
        .find("Ok(Self {")
        .ok_or_else(|| "Ok(Self not found".to_string())?;
    let ok_self_open = fn_start + ok_self_rel + "Ok(Self ".len();
    let ok_self_close = find_matching_brace(src, ok_self_open)
        .ok_or_else(|| "Ok(Self block is not closed".to_string())?;
    let block = &src[ok_self_open..=ok_self_close];
    // Match the full accessor line so `priority` doesn't collide with `priority_2`.
    // Pattern: `\n<ws><from>: row.get_*("<from>")?,`
    let from_lhs = format!("{from}:");
    let from_arg = format!("\"{from}\"");
    let to_lhs = format!("{to}:");
    let to_arg = format!("\"{to}\"");
    if !block.contains(&from_lhs) {
        return Err(format!(
            "from_row does not reference `{from}:`; rename cannot proceed"
        ));
    }
    let replaced = block
        .replacen(&from_lhs, &to_lhs, 1)
        .replacen(&from_arg, &to_arg, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..ok_self_open]);
    out.push_str(&replaced);
    out.push_str(&src[ok_self_close + 1..]);
    Ok(out)
}

fn rename_in_insert_values(src: &str, from: &str, to: &str) -> Result<String, String> {
    let fn_start = src
        .find("fn insert_values(")
        .ok_or_else(|| "insert_values not found".to_string())?;
    let vec_rel = src[fn_start..]
        .find("vec![")
        .ok_or_else(|| "no `vec![` inside insert_values".to_string())?;
    let vec_open = fn_start + vec_rel + 4;
    let vec_close = find_matching_bracket(src, vec_open)
        .ok_or_else(|| "vec![ … ] is not closed".to_string())?;
    let block = &src[vec_open..=vec_close];
    let from_pattern = format!("self.{from}");
    let to_pattern = format!("self.{to}");
    if !block.contains(&from_pattern) {
        return Err(format!(
            "insert_values does not reference `self.{from}`; rename cannot proceed"
        ));
    }
    let replaced = block.replacen(&from_pattern, &to_pattern, 1);
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..vec_open]);
    out.push_str(&replaced);
    out.push_str(&src[vec_close + 1..]);
    Ok(out)
}

/// True when `src` contains an actual `use chrono::…;` statement at the
/// start of a line. A docstring mentioning `chrono::DateTime` does not
/// count — only a real import satisfies the check.
fn has_chrono_use(src: &str) -> bool {
    src.lines()
        .any(|l| l.trim_start().starts_with("use chrono::"))
}

fn insert_chrono_import(src: &str) -> String {
    // Put the use statement right after the last top-level `use` line.
    let mut last_use_end: Option<usize> = None;
    for (idx, line) in src.match_indices('\n') {
        // Re-use match_indices to walk line boundaries.
        let before_nl = &src[..idx];
        let line_start = before_nl.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_txt = &src[line_start..idx];
        if line_txt.trim_start().starts_with("use ") {
            last_use_end = Some(idx);
        }
        let _ = line; // unused
    }
    match last_use_end {
        Some(end) => {
            let mut out = String::with_capacity(src.len() + 40);
            out.push_str(&src[..=end]);
            out.push_str("use chrono::{DateTime, Utc};\n");
            out.push_str(&src[end + 1..]);
            out
        }
        None => format!("use chrono::{{DateTime, Utc}};\n{src}"),
    }
}

// --- per-type helpers -------------------------------------------------------

fn rust_type_for(ty: &str, nullable: bool) -> String {
    let base = match ty {
        "i32" => "i32",
        "i64" => "i64",
        "String" => "String",
        "bool" => "bool",
        "DateTime" => "DateTime<Utc>",
        other => other,
    };
    if nullable {
        format!("Option<{base}>")
    } else {
        base.to_string()
    }
}

fn row_accessor(ty: &str, nullable: bool) -> String {
    let suffix = match ty {
        "i32" => "i32",
        "i64" => "i64",
        "String" => "string",
        "bool" => "bool",
        "DateTime" => "datetime",
        _ => "string",
    };
    if nullable {
        format!("get_optional_{suffix}")
    } else {
        format!("get_{suffix}")
    }
}

fn build_insert_values_line(field: &str, ty: &str, _nullable: bool) -> String {
    // `.clone()` is needed for non-`Copy` types so `insert_values(&self)`
    // doesn't move out of `self`. `String` (and `Option<String>`) are
    // the ones that matter today; every other supported primitive is
    // `Copy` (or converts from `Copy`). If more non-Copy types land,
    // extend this list explicitly rather than guessing.
    let call = if ty == "String" {
        format!("self.{field}.clone().into()")
    } else {
        format!("self.{field}.into()")
    };
    format!("            {call},\n")
}

// --- project introspection --------------------------------------------------

fn locate_model_file(
    project: &ProjectView,
    struct_name: &str,
) -> Result<(String, String), ExecutionError> {
    let mut matches: Vec<&str> = project
        .models_files
        .iter()
        .filter(|(_, f)| f.struct_names.iter().any(|s| s == struct_name))
        .map(|(app, _)| app.as_str())
        .collect();
    match matches.len() {
        0 => Err(ExecutionError::ProjectStructure(format!(
            "no apps/<app>/models.rs declares `pub struct {struct_name}`"
        ))),
        1 => {
            let app = matches.remove(0).to_string();
            let source = project.models_files[&app].source.clone();
            Ok((app, source))
        }
        _ => Err(ExecutionError::ProjectStructure(format!(
            "multiple apps declare `pub struct {struct_name}`: {}",
            matches.join(", ")
        ))),
    }
}

fn find_table_for_struct(src: &str, struct_name: &str) -> Option<String> {
    // Extract the string value of `const TABLE: &'static str = "<name>";`
    // from the matching `impl Model for <struct>` block.
    //
    // 0.9.1: scoped by struct name. Older callers that want the
    // whole-file first match should pass an empty string — but no
    // in-tree site does. The previous "one Model impl per file"
    // assumption breaks on real projects like medflow that declare
    // several models per app.
    let impl_anchor = format!("impl Model for {struct_name}");
    let slice = if let Some(impl_start) = src.find(&impl_anchor) {
        // Restrict to the matching impl block so sibling models can't
        // shadow this one's `const TABLE`.
        let brace_rel = src[impl_start..].find('{')?;
        let open = impl_start + brace_rel;
        let close = find_matching_brace(src, open)?;
        &src[open..=close]
    } else {
        // Fallback (rare): no matching impl found — scan the whole file.
        // Preserves the 0.8.x behaviour for callers that rely on the
        // fallback path before the struct exists.
        src
    };
    let anchor = "const TABLE: &'static str = \"";
    let start = slice.find(anchor)? + anchor.len();
    let end = slice[start..].find('"')?;
    Some(slice[start..start + end].to_string())
}

/// Snake-case derivation used when `const TABLE` can't be read
/// (shouldn't happen with scaffold output — defensive fallback).
fn fallback_table_name(struct_name: &str) -> Option<String> {
    let mut out = String::with_capacity(struct_name.len() + 4);
    for (i, ch) in struct_name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    // Pluralise naively — matches what `rustio new app` does.
    if !out.ends_with('s') {
        out.push('s');
    }
    Some(out)
}

fn next_migration_number(existing: &[String]) -> u32 {
    let mut max: u32 = 0;
    for name in existing {
        let Some(prefix) = name.split('_').next() else {
            continue;
        };
        if let Ok(n) = prefix.parse::<u32>() {
            if n > max {
                max = n;
            }
        }
    }
    max + 1
}

fn new_migration_path(project: &ProjectView, number: u32, slug: &str) -> (PathBuf, String) {
    let filename = format!("{number:04}_{slug}.sql");
    (project.root.join("migrations").join(&filename), filename)
}

pub(super) fn sql_for_add_field(table: &str, field: &FieldSpec) -> String {
    let sql_type = sql_type_for(&field.ty);
    if field.nullable {
        format!(
            "-- Generated by rustio ai apply. DO NOT EDIT.\n\
             ALTER TABLE {table} ADD COLUMN {name} {sql_type};\n",
            name = field.name,
        )
    } else {
        let default = safe_default_literal(&field.ty);
        format!(
            "-- Generated by rustio ai apply. DO NOT EDIT.\n\
             ALTER TABLE {table} ADD COLUMN {name} {sql_type} NOT NULL DEFAULT {default};\n",
            name = field.name,
        )
    }
}

/// Phase 2: PostgreSQL native types. `i32` → `INTEGER`, `i64` → `BIGINT`
/// (separate from `INTEGER` unlike SQLite which collapses both). `bool`
/// is a real `BOOLEAN`, not 0/1. `DateTime` is `TIMESTAMPTZ` so the
/// stored value is timezone-aware.
fn sql_type_for(ty: &str) -> &'static str {
    match ty {
        "i32" => "INTEGER",
        "i64" => "BIGINT",
        "bool" => "BOOLEAN",
        "String" => "TEXT",
        "DateTime" => "TIMESTAMPTZ",
        _ => "TEXT",
    }
}

/// Phase 2: PostgreSQL `NOT NULL DEFAULT` literals. `bool` defaults to
/// `FALSE` (not `0`); `DateTime` uses the same epoch literal SQLite did,
/// rendered with `+00` so PG parses it as `timestamptz` without a
/// "missing time zone" warning.
fn safe_default_literal(ty: &str) -> &'static str {
    match ty {
        "i32" | "i64" => "0",
        "bool" => "FALSE",
        "String" => "''",
        "DateTime" => "'1970-01-01 00:00:00+00'",
        _ => "''",
    }
}

// ---------------------------------------------------------------------------
// Impure entry — reads project from disk, applies atomically
// ---------------------------------------------------------------------------

impl ProjectView {
    /// Build a [`ProjectView`] by reading the project at `root`. Reads
    /// every `apps/*/models.rs` and lists `migrations/*`. Returns a
    /// [`ExecutionError::ProjectStructure`] if the scaffold isn't
    /// recognisable — the executor will not apply to a non-rustio
    /// directory.
    pub fn from_dir(root: &Path) -> Result<Self, ExecutionError> {
        let apps_dir = root.join("apps");
        let migrations_dir = root.join("migrations");
        if !apps_dir.is_dir() {
            return Err(ExecutionError::ProjectStructure(format!(
                "expected directory `apps/` at {}",
                root.display()
            )));
        }
        if !migrations_dir.is_dir() {
            return Err(ExecutionError::ProjectStructure(format!(
                "expected directory `migrations/` at {}",
                root.display()
            )));
        }

        let mut models_files = BTreeMap::new();
        let entries = std::fs::read_dir(&apps_dir).map_err(|e| ExecutionError::IoError {
            path: apps_dir.display().to_string(),
            message: e.to_string(),
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| ExecutionError::IoError {
                path: apps_dir.display().to_string(),
                message: e.to_string(),
            })?;
            let ty = entry.file_type().map_err(|e| ExecutionError::IoError {
                path: entry.path().display().to_string(),
                message: e.to_string(),
            })?;
            if !ty.is_dir() {
                continue;
            }
            let app_dir = entry.path();
            let app_name = app_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
                .unwrap_or_default();
            if app_name.is_empty() {
                continue;
            }
            let models_path = app_dir.join("models.rs");
            if !models_path.is_file() {
                continue;
            }
            let source =
                std::fs::read_to_string(&models_path).map_err(|e| ExecutionError::IoError {
                    path: models_path.display().to_string(),
                    message: e.to_string(),
                })?;
            let struct_names = parse_struct_names(&source);
            models_files.insert(
                app_name,
                ParsedModelsFile {
                    path: models_path,
                    source,
                    struct_names,
                },
            );
        }

        let mut existing_migrations = Vec::new();
        let mut migration_sources: BTreeMap<String, String> = BTreeMap::new();
        let entries = std::fs::read_dir(&migrations_dir).map_err(|e| ExecutionError::IoError {
            path: migrations_dir.display().to_string(),
            message: e.to_string(),
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| ExecutionError::IoError {
                path: migrations_dir.display().to_string(),
                message: e.to_string(),
            })?;
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".sql") {
                    let path = entry.path();
                    let contents =
                        std::fs::read_to_string(&path).map_err(|e| ExecutionError::IoError {
                            path: path.display().to_string(),
                            message: e.to_string(),
                        })?;
                    migration_sources.insert(name.to_string(), contents);
                    existing_migrations.push(name.to_string());
                }
            }
        }
        existing_migrations.sort();

        Ok(ProjectView {
            root: root.to_path_buf(),
            models_files,
            existing_migrations,
            migration_sources,
        })
    }
}

fn parse_struct_names(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in source.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("pub struct ") {
            // Name runs until whitespace, `{`, or `<`.
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

/// Run a plan against the project on disk.
///
/// Reads the schema at `<root>/rustio.schema.json`, builds a
/// [`ProjectView`], calls [`plan_execution`], verifies preconditions
/// against the live filesystem, and applies the change set atomically.
/// No migrations are executed — the user runs `rustio migrate apply`
/// afterwards.
pub fn execute_plan_document(
    project_root: &Path,
    doc: &PlanDocument,
    options: &ExecuteOptions,
    context: Option<&ContextConfig>,
) -> Result<ExecutionResult, ExecutionError> {
    let schema_path = project_root.join("rustio.schema.json");
    let schema_json =
        std::fs::read_to_string(&schema_path).map_err(|e| ExecutionError::IoError {
            path: schema_path.display().to_string(),
            message: e.to_string(),
        })?;
    let schema =
        Schema::parse(&schema_json).map_err(|e| ExecutionError::ValidationFailed(e.to_string()))?;
    let project = ProjectView::from_dir(project_root)?;
    let preview = plan_execution(&schema, &project, doc, options, context)?;
    commit_changes(&preview)?;
    let generated: Vec<String> = preview
        .file_changes
        .iter()
        .map(|c| display_path(project_root, &c.path))
        .collect();
    Ok(ExecutionResult {
        applied_steps: preview.applied_steps,
        generated_files: generated,
        summary: preview.summary,
    })
}

/// Commit the preview to disk. Each target is written to a sibling
/// `.rustio_tmp` file first; only after every target has a tempfile
/// does the executor rename them into place. If any rename fails, the
/// already-renamed files are restored from their pre-apply content.
fn commit_changes(preview: &ExecutionPreview) -> Result<(), ExecutionError> {
    // 1. Conflict + precondition pass against the live filesystem.
    for change in &preview.file_changes {
        match change.kind {
            FileChangeKind::Create => {
                if change.path.exists() {
                    return Err(ExecutionError::FileConflict {
                        path: change.path.display().to_string(),
                        reason: "file already exists — refusing to overwrite".to_string(),
                    });
                }
                if let Some(parent) = change.path.parent() {
                    if !parent.is_dir() {
                        return Err(ExecutionError::ProjectStructure(format!(
                            "parent directory `{}` does not exist",
                            parent.display()
                        )));
                    }
                }
            }
            FileChangeKind::Update => {
                let actual =
                    std::fs::read_to_string(&change.path).map_err(|e| ExecutionError::IoError {
                        path: change.path.display().to_string(),
                        message: e.to_string(),
                    })?;
                if let Some(expected) = &change.expected_current_contents {
                    if &actual != expected {
                        return Err(ExecutionError::FileConflict {
                            path: change.path.display().to_string(),
                            reason: "file changed on disk after the plan was generated".to_string(),
                        });
                    }
                }
            }
        }
    }

    // 2. Write each change to a .rustio_tmp sibling file.
    let mut tmp_paths: Vec<PathBuf> = Vec::with_capacity(preview.file_changes.len());
    for change in &preview.file_changes {
        let tmp = change.path.with_extension(match change.path.extension() {
            Some(e) => format!("{}.rustio_tmp", e.to_string_lossy()),
            None => "rustio_tmp".to_string(),
        });
        if let Err(e) = std::fs::write(&tmp, &change.new_contents) {
            cleanup_tmps(&tmp_paths);
            return Err(ExecutionError::IoError {
                path: tmp.display().to_string(),
                message: e.to_string(),
            });
        }
        tmp_paths.push(tmp);
    }

    // 3. Rename .rustio_tmp → final path. Track (target, original) so
    // we can roll back if a later rename fails.
    let mut renamed: Vec<(PathBuf, Option<String>)> =
        Vec::with_capacity(preview.file_changes.len());
    for (i, change) in preview.file_changes.iter().enumerate() {
        let tmp = &tmp_paths[i];
        let original = match change.kind {
            FileChangeKind::Update => change.expected_current_contents.clone(),
            FileChangeKind::Create => None,
        };
        if let Err(e) = std::fs::rename(tmp, &change.path) {
            // Roll back: restore already-renamed targets, clean up
            // remaining tmps.
            rollback_renames(&renamed);
            cleanup_tmps(&tmp_paths[i..]);
            return Err(ExecutionError::IoError {
                path: change.path.display().to_string(),
                message: e.to_string(),
            });
        }
        renamed.push((change.path.clone(), original));
    }
    Ok(())
}

fn cleanup_tmps(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

fn rollback_renames(renamed: &[(PathBuf, Option<String>)]) {
    for (path, original) in renamed.iter().rev() {
        match original {
            Some(contents) => {
                let _ = std::fs::write(path, contents);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn display_path(root: &Path, absolute: &Path) -> String {
    absolute
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .map(String::from)
        .unwrap_or_else(|| absolute.display().to_string())
}

// ---------------------------------------------------------------------------
// 0.9.0 — FK retrofit
// ---------------------------------------------------------------------------

/// Result of scanning a schema for belongs_to relations that were
/// materialised before 0.9.0 (no `on_delete` / `required` metadata).
#[derive(Debug, Clone)]
pub struct RetrofitReport {
    /// `(model_name, field_name)` pairs the retrofit would upgrade.
    pub upgraded: Vec<(String, String)>,
    /// `(filename, sql)` pairs to write into the `migrations/` dir.
    /// Empty when nothing needs retrofitting.
    pub migrations: Vec<(String, String)>,
}

/// 0.9.0 — generate a retrofit plan for every belongs_to relation that
/// lacks `on_delete` metadata. Returns a report the caller can either
/// print (dry-run) or materialise (write migrations + update schema).
///
/// Safety: the caller must pass a `Schema` that faithfully reflects the
/// live SQL tables. If the user has hand-written migrations that added
/// columns, indexes, or constraints outside the schema.json shape, the
/// recreate-table SQL will drop them. The CLI surfaces a warning to
/// this effect and requires a `--write` flag to actually emit files.
pub fn plan_retrofit_foreign_keys(schema: &crate::schema::Schema) -> RetrofitReport {
    use crate::schema::{RelationKind, SchemaField};

    let mut upgraded = Vec::new();
    let mut migrations = Vec::new();

    // Build a model name → table name lookup. Retrofit targets only
    // schema entries, so fallback_table_name fills the gap for
    // projects whose `const TABLE` we can't read offline.
    let table_for = |model_name: &str| -> Option<String> {
        schema
            .models
            .iter()
            .find(|m| m.name == model_name)
            .and_then(|m| {
                // `SchemaModel` carries `table` implicitly through the
                // exporter — fall through to the naming convention when
                // the field isn't present.
                fallback_table_name(&m.name)
            })
    };

    for model in &schema.models {
        let table = match fallback_table_name(&model.name) {
            Some(t) => t,
            None => continue,
        };
        // Build the upgraded field list (populate on_delete+required
        // defaults on any relation field that lacks them).
        let mut touched_this_model = false;
        let upgraded_fields: Vec<SchemaField> = model
            .fields
            .iter()
            .map(|f| {
                let mut f = f.clone();
                if let Some(rel) = f.relation.as_mut() {
                    if matches!(rel.kind, RelationKind::BelongsTo) && rel.on_delete.is_none() {
                        rel.on_delete = Some("restrict".to_string());
                        rel.required = Some(!f.nullable);
                        upgraded.push((model.name.clone(), f.name.clone()));
                        touched_this_model = true;
                    }
                }
                f
            })
            .collect();

        if !touched_this_model {
            continue;
        }

        // Build SQL that recreates the table with explicit FOREIGN KEYs.
        let mut sql = String::new();
        sql.push_str("-- Generated by `rustio migrate add-fks` (0.9.0).\n");
        sql.push_str("-- Review before running: the recreate-table pattern drops and\n");
        sql.push_str("-- rebuilds the table. Columns or indexes added outside of\n");
        sql.push_str("-- rustio's schema.json will be lost if not reflected here.\n");
        sql.push_str("PRAGMA foreign_keys = OFF;\n");
        sql.push_str("BEGIN;\n\n");
        let new_table = format!("{table}__new");
        sql.push_str(&format!("CREATE TABLE {new_table} (\n"));
        for (i, f) in upgraded_fields.iter().enumerate() {
            sql.push_str("    ");
            sql.push_str(&column_def_with_relation_context(f, |target| {
                table_for(target)
            }));
            if i + 1 < upgraded_fields.len() {
                sql.push(',');
            }
            sql.push('\n');
        }
        sql.push_str(");\n\n");
        let col_list = upgraded_fields
            .iter()
            .map(|f| f.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(&format!(
            "INSERT INTO {new_table} ({col_list}) SELECT {col_list} FROM {table};\n\n"
        ));
        sql.push_str(&format!("DROP TABLE {table};\n"));
        sql.push_str(&format!("ALTER TABLE {new_table} RENAME TO {table};\n\n"));
        sql.push_str("COMMIT;\n");
        sql.push_str("PRAGMA foreign_keys = ON;\n");

        migrations.push((format!("retrofit_fks_{table}"), sql));
    }

    RetrofitReport {
        upgraded,
        migrations,
    }
}

// ---------------------------------------------------------------------------
// Human-readable preview
// ---------------------------------------------------------------------------

/// Render an [`ExecutionPreview`] as an operator-friendly block. The
/// CLI prints this before asking for confirmation.
pub fn render_preview_human(preview: &ExecutionPreview, risk: RiskLevel) -> String {
    let mut out = String::from("Plan to apply\n\n");
    out.push_str("Applying:\n");
    // Each summary line already carries its own glyph (`+` for add,
    // `~` for mutate, `-` for destructive; warning lines are indented
    // with four leading spaces). The renderer just reserves a two-
    // space indent for every line so the block is visually uniform.
    for line in preview.summary.lines() {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("\nFiles to be written:\n");
    for change in &preview.file_changes {
        let kind = match change.kind {
            FileChangeKind::Create => "create",
            FileChangeKind::Update => "update",
        };
        out.push_str(&format!("  - {kind} {}\n", change.path.display()));
    }
    out.push_str(&format!("\nRisk:\n  {}\n", risk.as_str()));
    out
}
