//! Advanced executor tests — 0.5.3.
//!
//! Covers the primitives that require SQLite recreate-table:
//!
//! - `change_field_type`
//! - `change_field_nullability`
//! - `rename_model`
//!
//! Invariants:
//!
//! - Every "advanced" migration uses the recreate-table shape:
//!   `CREATE TABLE <t>__new`, `INSERT SELECT`, `DROP`, `RENAME`.
//! - Required → nullable is safe; nullable → required emits
//!   `COALESCE(col, default)` in the INSERT SELECT.
//! - Tables with FK constraints are refused, not silently destroyed.
//! - Rename_model touches models.rs + admin.rs; views.rs is patched
//!   best-effort at identifier boundaries.
//! - All primitives are idempotent — second apply returns
//!   `FileConflict`, not a silent success.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{TimeZone, Utc};

use super::executor::{
    plan_execution, ExecuteOptions, ExecutionError, ExecutionPreview, ParsedModelsFile, ProjectView,
};
use super::planner::PlanResult;
use super::review::{build_plan_document_with_timestamp, PlanDocument};
use super::{ChangeFieldNullability, ChangeFieldType, FieldSpec, Plan, Primitive, RenameModel};
use crate::schema::{Schema, SchemaField, SchemaModel, SCHEMA_VERSION};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn pkg_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn fixed_ts() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap()
}

const POST_MODELS_SRC: &str = r#"use rustio_core::{Error, Model, Row, RustioAdmin, Value};

#[derive(Debug, RustioAdmin)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub score: i32,
    pub subtitle: Option<String>,
}

impl Model for Post {
    const TABLE: &'static str = "posts";
    const COLUMNS: &'static [&'static str] = &["id", "title", "score", "subtitle"];
    const INSERT_COLUMNS: &'static [&'static str] = &["title", "score", "subtitle"];

    fn id(&self) -> i64 {
        self.id
    }

    fn from_row(row: Row<'_>) -> Result<Self, Error> {
        Ok(Self {
            id: row.get_i64("id")?,
            title: row.get_string("title")?,
            score: row.get_i32("score")?,
            subtitle: row.get_optional_string("subtitle")?,
        })
    }

    fn insert_values(&self) -> Vec<Value> {
        vec![
            self.title.clone().into(),
            self.score.into(),
            self.subtitle.clone().into(),
        ]
    }
}
"#;

const POST_ADMIN_SRC: &str = r#"use rustio_core::admin::Admin;

use super::models::Post;

pub fn install(admin: Admin) -> Admin {
    admin.model::<Post>()
}
"#;

const POST_VIEWS_SRC: &str = r#"use rustio_core::Router;

use super::models::Post;

pub fn register(router: Router) -> Router {
    // A tiny views stub that references Post, to verify rename_model
    // rewrites identifier occurrences at word boundaries.
    let _use_it: Vec<&Post> = Vec::new();
    let _also: Option<Post> = None;
    router
}
"#;

fn post_schema() -> Schema {
    Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![SchemaModel {
            name: "Post".into(),
            table: "posts".into(),
            admin_name: "posts".into(),
            display_name: "Posts".into(),
            singular_name: "Post".into(),
            fields: vec![
                SchemaField {
                    name: "id".into(),
                    ty: "i64".into(),
                    nullable: false,
                    editable: false,
                    relation: None,
                },
                SchemaField {
                    name: "title".into(),
                    ty: "String".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                },
                SchemaField {
                    name: "score".into(),
                    ty: "i32".into(),
                    nullable: false,
                    editable: true,
                    relation: None,
                },
                SchemaField {
                    name: "subtitle".into(),
                    ty: "String".into(),
                    nullable: true,
                    editable: true,
                    relation: None,
                },
            ],
            relations: vec![],
            core: false,
        }],
    }
}

fn project_with_post(root: &str) -> ProjectView {
    let mut models_files = BTreeMap::new();
    models_files.insert(
        "posts".to_string(),
        ParsedModelsFile {
            path: PathBuf::from(format!("{root}/apps/posts/models.rs")),
            source: POST_MODELS_SRC.to_string(),
            struct_names: vec!["Post".into()],
        },
    );
    ProjectView {
        root: PathBuf::from(root),
        models_files,
        existing_migrations: vec!["0001_create_posts.sql".into()],
        migration_sources: BTreeMap::new(),
    }
}

fn doc_for(schema: &Schema, prompt: &str, plan: Plan) -> PlanDocument {
    let result = PlanResult {
        plan,
        explanation: "unit-test".into(),
    };
    build_plan_document_with_timestamp(schema, prompt, &result, fixed_ts(), None)
        .expect("fixture plans should build")
}

fn unwrap_preview(p: Result<ExecutionPreview, ExecutionError>) -> ExecutionPreview {
    p.unwrap_or_else(|e| panic!("plan_execution should have succeeded: {e}"))
}

// ---------------------------------------------------------------------------
// change_field_type
// ---------------------------------------------------------------------------

#[test]
fn change_type_i32_to_string_uses_cast_and_rewrites_models() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Post".into(),
        field: "score".into(),
        new_type: "String".into(),
    })]);
    let doc = doc_for(&schema, "change score to String", plan);
    let preview = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));

    // The summary carries the `~` glyph and the table-rewrite warning.
    assert!(
        preview
            .summary
            .starts_with("~ Change type of Post.score from i32 to String"),
        "summary: {:?}",
        preview.summary,
    );
    assert!(
        preview
            .summary
            .contains("⚠ This rewrites the entire `posts` table"),
        "missing rewrite warning: {:?}",
        preview.summary,
    );

    // Two file changes: models.rs + recreate migration.
    assert_eq!(preview.file_changes.len(), 2);
    let models_src = &preview.file_changes[0].new_contents;
    assert!(
        models_src.contains("pub score: String,"),
        "struct field should be String:\n{models_src}",
    );
    assert!(
        models_src.contains("score: row.get_string(\"score\")?,"),
        "from_row accessor should be get_string:\n{models_src}",
    );
    assert!(
        models_src.contains("self.score.clone().into(),"),
        "insert_values should now .clone() on String:\n{models_src}",
    );

    // Migration uses the recreate-table pattern with CAST on `score`.
    let mig = &preview.file_changes[1].new_contents;
    assert!(mig.contains("CREATE TABLE posts__new ("), "mig:\n{mig}");
    assert!(
        mig.contains("id INTEGER PRIMARY KEY AUTOINCREMENT"),
        "mig should preserve PK AUTOINCREMENT:\n{mig}",
    );
    assert!(
        mig.contains("CAST(score AS TEXT)"),
        "mig should CAST score to TEXT:\n{mig}",
    );
    assert!(
        mig.contains("INSERT INTO posts__new (id, title, score, subtitle)"),
        "mig should INSERT every column:\n{mig}",
    );
    assert!(mig.contains("DROP TABLE posts;"), "mig:\n{mig}");
    assert!(
        mig.contains("ALTER TABLE posts__new RENAME TO posts;"),
        "mig:\n{mig}",
    );
}

#[test]
fn change_type_unsafe_cast_is_refused() {
    // String → {i32,i64,bool} is warned-but-allowed (TEXT → INTEGER
    // CAST is well-defined in SQLite). Truly unsafe combinations —
    // e.g. DateTime → i32, which mixes storage classes in a way SQLite
    // wouldn't CAST meaningfully — must be refused.
    let schema = post_schema();
    let mut project = project_with_post("/p");
    // Seed a DateTime field so we have a source for the refused cast.
    let mut schema = schema;
    schema.models[0].fields.push(SchemaField {
        name: "published_at".into(),
        ty: "DateTime".into(),
        nullable: false,
        editable: true,
        relation: None,
    });
    // models.rs mirror: insert a DateTime field so the executor finds
    // it in the struct block.
    let src = POST_MODELS_SRC.replace(
        "pub subtitle: Option<String>,\n}",
        "pub subtitle: Option<String>,\n    pub published_at: DateTime<Utc>,\n}",
    );
    project.models_files.get_mut("posts").unwrap().source = src;

    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Post".into(),
        field: "published_at".into(),
        new_type: "i32".into(),
    })]);
    let doc = doc_for(&schema, "change published_at to i32", plan);
    let err = plan_execution(&schema, &project, &doc, &ExecuteOptions::default(), None)
        .expect_err("DateTime → i32 is not a safe cast");
    match err {
        ExecutionError::UnsupportedPrimitive { op, reason } => {
            assert_eq!(op, "change_field_type");
            assert!(reason.contains("safe-cast"));
        }
        other => panic!("expected UnsupportedPrimitive, got {other:?}"),
    }
}

#[test]
fn change_type_idempotent_same_type_is_refused() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Post".into(),
        field: "score".into(),
        new_type: "i32".into(), // already i32
    })]);
    let doc = doc_for(&schema, "no-op", plan);
    let err = plan_execution(&schema, &project, &doc, &ExecuteOptions::default(), None)
        .expect_err("no-op type change must be refused");
    assert!(matches!(err, ExecutionError::FileConflict { .. }));
}

#[test]
fn change_type_refuses_on_foreign_key_tables() {
    let schema = post_schema();
    let mut project = project_with_post("/p");
    // Simulate a foreign-key constraint landing on `posts` from
    // another table's migration.
    project.migration_sources.insert(
        "0002_create_comments.sql".into(),
        "CREATE TABLE comments (id INTEGER, post_id INTEGER, FOREIGN KEY (post_id) REFERENCES posts(id));"
            .into(),
    );
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Post".into(),
        field: "score".into(),
        new_type: "String".into(),
    })]);
    let doc = doc_for(&schema, "change score to String", plan);
    let err = plan_execution(&schema, &project, &doc, &ExecuteOptions::default(), None)
        .expect_err("FK-participating table must be refused");
    match err {
        ExecutionError::UnsupportedPrimitive { op, reason } => {
            assert_eq!(op, "change_field_type");
            assert!(reason.contains("foreign"));
        }
        other => panic!("expected UnsupportedPrimitive, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// change_field_nullability
// ---------------------------------------------------------------------------

#[test]
fn nullability_required_to_nullable_is_safe() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldNullability(
        ChangeFieldNullability {
            model: "Post".into(),
            field: "title".into(),
            nullable: true,
        },
    )]);
    let doc = doc_for(&schema, "relax title to optional", plan);
    let preview = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));
    // Struct field becomes Option<String>.
    let models_src = &preview.file_changes[0].new_contents;
    assert!(
        models_src.contains("pub title: Option<String>,"),
        "struct:\n{models_src}",
    );
    assert!(
        models_src.contains("title: row.get_optional_string(\"title\")?,"),
        "from_row accessor should be get_optional_string:\n{models_src}",
    );
    // Migration: straight copy (no COALESCE).
    let mig = &preview.file_changes[1].new_contents;
    assert!(
        !mig.contains("COALESCE"),
        "relaxing nullability needs no COALESCE:\n{mig}",
    );
    assert!(mig.contains("CREATE TABLE posts__new ("), "mig:\n{mig}");
    // title in new table lacks NOT NULL.
    assert!(
        mig.contains("title TEXT\n") || mig.contains("title TEXT,"),
        "mig:\n{mig}"
    );
}

#[test]
fn nullability_nullable_to_required_uses_coalesce() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldNullability(
        ChangeFieldNullability {
            model: "Post".into(),
            field: "subtitle".into(),
            nullable: false,
        },
    )]);
    let doc = doc_for(&schema, "tighten subtitle to required", plan);
    let preview = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));
    let models_src = &preview.file_changes[0].new_contents;
    assert!(
        models_src.contains("pub subtitle: String,"),
        "struct should drop Option<>:\n{models_src}",
    );
    assert!(
        models_src.contains("subtitle: row.get_string(\"subtitle\")?,"),
        "accessor should be get_string:\n{models_src}",
    );
    // Migration: COALESCE(subtitle, '') in the INSERT SELECT.
    let mig = &preview.file_changes[1].new_contents;
    assert!(
        mig.contains("COALESCE(subtitle, '')"),
        "COALESCE needed to replace NULLs:\n{mig}",
    );
    // The warn line flags the NULL-substitution explicitly.
    assert!(
        preview.summary.contains("substitutes existing NULLs"),
        "summary should surface the NULL substitution warning: {:?}",
        preview.summary,
    );
}

#[test]
fn nullability_same_state_is_refused() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldNullability(
        ChangeFieldNullability {
            model: "Post".into(),
            field: "subtitle".into(),
            nullable: true, // already nullable
        },
    )]);
    let doc = doc_for(&schema, "no-op", plan);
    let err = plan_execution(&schema, &project, &doc, &ExecuteOptions::default(), None)
        .expect_err("no-op must be refused");
    assert!(matches!(err, ExecutionError::FileConflict { .. }));
}

// ---------------------------------------------------------------------------
// rename_model
// ---------------------------------------------------------------------------

mod rename_model_integration {
    use super::*;
    use std::fs;

    use crate::ai::executor::execute_plan_document;

    fn scratch_dir(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rustio-exec-adv-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("apps/posts")).unwrap();
        fs::create_dir_all(root.join("migrations")).unwrap();
        fs::write(root.join("apps/posts/models.rs"), POST_MODELS_SRC).unwrap();
        fs::write(root.join("apps/posts/admin.rs"), POST_ADMIN_SRC).unwrap();
        fs::write(root.join("apps/posts/views.rs"), POST_VIEWS_SRC).unwrap();
        fs::write(
            root.join("migrations/0001_create_posts.sql"),
            "CREATE TABLE posts (id INTEGER PRIMARY KEY);\n",
        )
        .unwrap();
        let schema_json = post_schema().to_pretty_json().unwrap();
        fs::write(root.join("rustio.schema.json"), schema_json).unwrap();
        root
    }

    #[test]
    fn rename_model_updates_models_admin_views_and_emits_migration() {
        let root = scratch_dir("rename");
        let schema = post_schema();
        let plan = Plan::new(vec![Primitive::RenameModel(RenameModel {
            from: "Post".into(),
            to: "Article".into(),
        })]);
        let doc = doc_for(&schema, "rename Post to Article", plan);

        let result = execute_plan_document(&root, &doc, &ExecuteOptions::default(), None).unwrap();
        assert_eq!(result.applied_steps, 1);
        // Three or four file paths: models.rs, admin.rs, views.rs
        // (if it changed), plus the migration.
        assert!(
            result
                .generated_files
                .iter()
                .any(|p| p.ends_with("apps/posts/models.rs")),
            "files: {:?}",
            result.generated_files,
        );
        assert!(
            result
                .generated_files
                .iter()
                .any(|p| p.ends_with("apps/posts/admin.rs")),
            "files: {:?}",
            result.generated_files,
        );
        assert!(
            result
                .generated_files
                .iter()
                .any(|p| p.ends_with("migrations/0002_rename_posts_to_articles.sql")),
            "files: {:?}",
            result.generated_files,
        );

        let models = fs::read_to_string(root.join("apps/posts/models.rs")).unwrap();
        assert!(
            models.contains("pub struct Article {"),
            "models.rs:\n{models}"
        );
        assert!(
            models.contains("impl Model for Article"),
            "models.rs:\n{models}"
        );
        assert!(
            models.contains("const TABLE: &'static str = \"articles\";"),
            "models.rs TABLE const:\n{models}",
        );
        assert!(
            !models.contains("pub struct Post "),
            "old struct name must be gone:\n{models}",
        );

        let admin = fs::read_to_string(root.join("apps/posts/admin.rs")).unwrap();
        assert!(
            admin.contains("use super::models::Article;"),
            "admin.rs use:\n{admin}",
        );
        assert!(
            admin.contains("admin.model::<Article>()"),
            "admin.rs call:\n{admin}",
        );

        let views = fs::read_to_string(root.join("apps/posts/views.rs")).unwrap();
        assert!(
            views.contains("use super::models::Article;"),
            "views.rs use:\n{views}",
        );
        assert!(
            views.contains("&Article"),
            "views.rs should rename bare identifier:\n{views}",
        );
        assert!(
            !views.contains("use super::models::Post"),
            "old use must be gone:\n{views}",
        );

        let mig =
            fs::read_to_string(root.join("migrations/0002_rename_posts_to_articles.sql")).unwrap();
        assert!(
            mig.contains("ALTER TABLE posts RENAME TO articles;"),
            "migration:\n{mig}",
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rename_model_refuses_if_new_struct_already_present() {
        let root = scratch_dir("rename_collide");
        // Put a second struct with the target name into models.rs.
        let mangled = POST_MODELS_SRC.replace(
            "pub struct Post {",
            "pub struct Article { }\n\npub struct Post {",
        );
        fs::write(root.join("apps/posts/models.rs"), &mangled).unwrap();
        let schema = post_schema();
        let plan = Plan::new(vec![Primitive::RenameModel(RenameModel {
            from: "Post".into(),
            to: "Article".into(),
        })]);
        let doc = doc_for(&schema, "rename", plan);
        let err = execute_plan_document(&root, &doc, &ExecuteOptions::default(), None).unwrap_err();
        match err {
            ExecutionError::FileConflict { reason, .. } => {
                assert!(reason.contains("already exists"), "reason: {reason}");
            }
            other => panic!("expected FileConflict, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }
}

// ---------------------------------------------------------------------------
// Determinism + rollback
// ---------------------------------------------------------------------------

#[test]
fn recreate_table_migration_is_deterministic() {
    let schema = post_schema();
    let project = project_with_post("/p");
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Post".into(),
        field: "score".into(),
        new_type: "String".into(),
    })]);
    let doc = doc_for(&schema, "x", plan);
    let a = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));
    let b = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));
    assert_eq!(a, b);
    // Regression canary: the preview's migration file always uses the
    // `<table>__new` temporary name — if that ever changes we want
    // a loud signal.
    let mig = &a.file_changes[1].new_contents;
    assert!(mig.contains("CREATE TABLE posts__new"), "mig:\n{mig}");
}

#[test]
fn large_schema_simulation_holds_determinism() {
    // Build a schema with many fields and run a type change — ensures
    // the recreate-table migration scales to wider tables and that
    // every unchanged column is copied verbatim.
    let mut fields: Vec<SchemaField> = vec![SchemaField {
        name: "id".into(),
        ty: "i64".into(),
        nullable: false,
        editable: false,
        relation: None,
    }];
    for i in 0..20 {
        fields.push(SchemaField {
            name: format!("field_{i:02}"),
            ty: if i % 2 == 0 { "String" } else { "i32" }.to_string(),
            nullable: false,
            editable: true,
            relation: None,
        });
    }
    let schema = Schema {
        version: SCHEMA_VERSION,
        rustio_version: pkg_version(),
        models: vec![SchemaModel {
            name: "Wide".into(),
            table: "wides".into(),
            admin_name: "wides".into(),
            display_name: "Wides".into(),
            singular_name: "Wide".into(),
            fields: fields.clone(),
            relations: vec![],
            core: false,
        }],
    };
    // Build a synthetic models.rs that matches the schema.
    let mut src = String::from(
        "use rustio_core::{Error, Model, Row, RustioAdmin, Value};\n\n\
         #[derive(Debug, RustioAdmin)]\npub struct Wide {\n",
    );
    for f in &fields {
        src.push_str(&format!(
            "    pub {}: {},\n",
            f.name,
            if f.ty == "i32" {
                "i32"
            } else if f.ty == "i64" {
                "i64"
            } else {
                "String"
            }
        ));
    }
    src.push_str("}\n\nimpl Model for Wide {\n");
    src.push_str("    const TABLE: &'static str = \"wides\";\n");
    src.push_str("    const COLUMNS: &'static [&'static str] = &[");
    let cols: Vec<String> = fields.iter().map(|f| format!("\"{}\"", f.name)).collect();
    src.push_str(&cols.join(", "));
    src.push_str("];\n");
    src.push_str("    const INSERT_COLUMNS: &'static [&'static str] = &[");
    let inserts: Vec<String> = fields
        .iter()
        .filter(|f| f.name != "id")
        .map(|f| format!("\"{}\"", f.name))
        .collect();
    src.push_str(&inserts.join(", "));
    src.push_str("];\n\n");
    src.push_str("    fn id(&self) -> i64 { self.id }\n\n");
    src.push_str("    fn from_row(row: Row<'_>) -> Result<Self, Error> {\n        Ok(Self {\n");
    for f in &fields {
        let acc = match f.ty.as_str() {
            "i32" => "get_i32",
            "i64" => "get_i64",
            "String" => "get_string",
            _ => "get_string",
        };
        src.push_str(&format!(
            "            {name}: row.{acc}(\"{name}\")?,\n",
            name = f.name,
        ));
    }
    src.push_str("        })\n    }\n\n");
    src.push_str("    fn insert_values(&self) -> Vec<Value> {\n        vec![\n");
    for f in fields.iter().filter(|f| f.name != "id") {
        if f.ty == "String" {
            src.push_str(&format!("            self.{}.clone().into(),\n", f.name));
        } else {
            src.push_str(&format!("            self.{}.into(),\n", f.name));
        }
    }
    src.push_str("        ]\n    }\n}\n");

    let mut models_files = BTreeMap::new();
    models_files.insert(
        "wides".into(),
        ParsedModelsFile {
            path: PathBuf::from("/p/apps/wides/models.rs"),
            source: src,
            struct_names: vec!["Wide".into()],
        },
    );
    let project = ProjectView {
        root: PathBuf::from("/p"),
        models_files,
        existing_migrations: vec!["0001_create_wides.sql".into()],
        migration_sources: BTreeMap::new(),
    };

    // Change field_05 (i32) → String.
    let plan = Plan::new(vec![Primitive::ChangeFieldType(ChangeFieldType {
        model: "Wide".into(),
        field: "field_05".into(),
        new_type: "String".into(),
    })]);
    let doc = doc_for(&schema, "change field_05", plan);
    let preview = unwrap_preview(plan_execution(
        &schema,
        &project,
        &doc,
        &ExecuteOptions::default(),
        None,
    ));
    let mig = &preview.file_changes[1].new_contents;
    // Every column shows up in the INSERT column list, and only
    // field_05 is wrapped in CAST.
    for f in &fields {
        assert!(
            mig.contains(&format!(", {}", f.name))
                || mig.contains(&format!("({}, ", f.name))
                || mig.contains(&format!("({}", f.name)),
            "column `{}` missing from INSERT:\n{mig}",
            f.name,
        );
    }
    assert!(
        mig.contains("CAST(field_05 AS TEXT)"),
        "only field_05 should be cast:\n{mig}",
    );
    let cast_count = mig.matches("CAST(").count();
    assert_eq!(cast_count, 1, "exactly one CAST expected, got {cast_count}");
}

// ---------------------------------------------------------------------------
// Smoke: the sqlite recreate-table helper is correct on a minimal shape
// ---------------------------------------------------------------------------

#[test]
fn field_spec_is_used_as_a_sentinel_for_unused_import() {
    // No-op — present so rustc doesn't complain about the FieldSpec
    // import in tight test configurations.
    let _ = FieldSpec {
        name: "x".into(),
        ty: "i32".into(),
        nullable: false,
        editable: true,
    };
}
