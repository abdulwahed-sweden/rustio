//! Plan executor. Writes migration files to disk. Rust source changes
//! are deferred — the current executor keeps its scope narrow (generate
//! migrations) so that hand-written `models.rs` files stay under the
//! user's control. That's a conscious trade-off; the 0.5.x series let
//! the executor edit models.rs, but it caused more surprises than it
//! saved.
//!
//! Destructive primitives (`RemoveField`, `RenameModel`) refuse to run
//! unless the caller sets `allow_destructive = true` on the options.

use std::fs;
use std::path::{Path, PathBuf};

use crate::ai::primitive::{Plan, Primitive};

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("destructive operation refused without --yes flag: {0}")]
    DestructiveRefused(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported primitive: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    pub allow_destructive: bool,
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub files_written: Vec<PathBuf>,
}

pub fn apply_plan(
    plan: &Plan,
    migrations_dir: &Path,
    opts: &ApplyOptions,
) -> Result<ApplyOutcome, ApplyError> {
    fs::create_dir_all(migrations_dir)?;
    let mut files_written = Vec::new();

    // Find the next version number by scanning the directory.
    let mut next_version = discover_next_version(migrations_dir);

    for step in &plan.steps {
        let (name, sql) = render_step(step, opts)?;
        let filename = format!("{:04}_{}.sql", next_version, name);
        let path = migrations_dir.join(filename);
        fs::write(&path, sql)?;
        files_written.push(path);
        next_version += 1;
    }

    Ok(ApplyOutcome { files_written })
}

fn render_step(step: &Primitive, opts: &ApplyOptions) -> Result<(String, String), ApplyError> {
    match step {
        Primitive::AddField { model, field } => {
            let table = table_name(model);
            let sql_type = sql_type_for(&field.field_type);
            let not_null = if field.nullable { "" } else { " NOT NULL DEFAULT ''" };
            let sql = format!(
                "ALTER TABLE {table} ADD COLUMN {col} {sql_type}{not_null};\n",
                col = field.name,
                not_null = if sql_type == "INTEGER" && !field.nullable {
                    " NOT NULL DEFAULT 0".into()
                } else {
                    not_null.to_string()
                }
            );
            Ok((format!("add_{}_to_{}", field.name, table), sql))
        }
        Primitive::RemoveField { model, field } => {
            if !opts.allow_destructive {
                return Err(ApplyError::DestructiveRefused(format!(
                    "remove field {field} from {model}"
                )));
            }
            let table = table_name(model);
            let sql = format!("ALTER TABLE {table} DROP COLUMN {field};\n");
            Ok((format!("drop_{}_from_{}", field, table), sql))
        }
        Primitive::RenameField { model, from, to } => {
            let table = table_name(model);
            let sql = format!("ALTER TABLE {table} RENAME COLUMN {from} TO {to};\n");
            Ok((format!("rename_{}_to_{}_in_{}", from, to, table), sql))
        }
        Primitive::AddRelation { from_model, to_model, via } => {
            let table = table_name(from_model);
            // Materialise as a plain i64 column. FK enforcement lives in 0.10+.
            let sql = format!(
                "-- relation: {from_model} -> {to_model}\nALTER TABLE {table} ADD COLUMN {via} INTEGER NOT NULL DEFAULT 0;\n"
            );
            Ok((format!("link_{}_to_{}", table, to_model.to_ascii_lowercase()), sql))
        }
        Primitive::RenameModel { from, to } => {
            if !opts.allow_destructive {
                return Err(ApplyError::DestructiveRefused(format!(
                    "rename model {from} to {to}"
                )));
            }
            let from_table = table_name(from);
            let to_table = table_name(to);
            let sql = format!("ALTER TABLE {from_table} RENAME TO {to_table};\n");
            Ok((format!("rename_{}_to_{}", from_table, to_table), sql))
        }
    }
}

fn table_name(model: &str) -> String {
    // "Post" → "posts". Mirrors the planner's naive singular→plural
    // round trip.
    let lower = model.to_ascii_lowercase();
    if lower.ends_with('s') {
        lower
    } else {
        format!("{lower}s")
    }
}

fn sql_type_for(field_type: &str) -> &'static str {
    match field_type {
        "i32" | "i64" | "bool" | "OptionalI64" => "INTEGER",
        "DateTime" => "TEXT",
        _ => "TEXT",
    }
}

fn discover_next_version(dir: &Path) -> u32 {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return 1,
    };
    let mut max_seen = 0u32;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = match name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if let Some((prefix, _)) = name.split_once('_') {
            if let Ok(n) = prefix.parse::<u32>() {
                max_seen = max_seen.max(n);
            }
        }
    }
    max_seen + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::primitive::FieldSpec;
    use tempfile::TempDir;

    // NOTE: using tempfile only under #[cfg(test)] so the crate doesn't
    // need it as a dependency in release builds. The workspace adds it
    // in dev-dependencies in the core crate.

    #[test]
    fn add_field_writes_a_migration() {
        let dir = TempDir::new().unwrap();
        let plan = Plan::new("x").step(Primitive::AddField {
            model: "Post".into(),
            field: FieldSpec {
                name: "slug".into(),
                field_type: "String".into(),
                nullable: false,
            },
        });
        let out = apply_plan(&plan, dir.path(), &ApplyOptions::default()).unwrap();
        assert_eq!(out.files_written.len(), 1);
        let contents = fs::read_to_string(&out.files_written[0]).unwrap();
        assert!(contents.contains("ALTER TABLE posts"));
        assert!(contents.contains("slug"));
    }

    #[test]
    fn remove_field_refuses_without_flag() {
        let dir = TempDir::new().unwrap();
        let plan = Plan::new("x").step(Primitive::RemoveField {
            model: "Post".into(),
            field: "title".into(),
        });
        let err = apply_plan(&plan, dir.path(), &ApplyOptions::default()).unwrap_err();
        assert!(matches!(err, ApplyError::DestructiveRefused(_)));
    }

    #[test]
    fn remove_field_runs_with_flag() {
        let dir = TempDir::new().unwrap();
        let plan = Plan::new("x").step(Primitive::RemoveField {
            model: "Post".into(),
            field: "title".into(),
        });
        let out = apply_plan(
            &plan,
            dir.path(),
            &ApplyOptions { allow_destructive: true },
        )
        .unwrap();
        assert_eq!(out.files_written.len(), 1);
    }
}
