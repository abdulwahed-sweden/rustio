use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use sqlx::Row as _;

use crate::error::Error;
use crate::orm::Db;

const TRACKING_TABLE: &str = "rustio_migrations";

pub fn list(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir).map_err(|e| Error::Internal(e.to_string()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && e.path().extension().and_then(|s| s.to_str()) == Some("sql")
        })
        .map(|e| e.path())
        .collect();
    files.sort();
    Ok(files)
}

pub fn generate(dir: &Path, name: &str, content: &str) -> Result<PathBuf, Error> {
    let sanitized = sanitize_name(name);
    if sanitized.is_empty() {
        return Err(Error::BadRequest(
            "migration name cannot be empty".to_string(),
        ));
    }
    fs::create_dir_all(dir).map_err(|e| Error::Internal(e.to_string()))?;
    let existing = list(dir)?;
    let next = next_number(&existing);
    let filename = format!("{:04}_{}.sql", next, sanitized);
    let path = dir.join(filename);
    fs::write(&path, content).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(path)
}

#[derive(Debug, Clone)]
pub struct MigrationRecord {
    pub filename: String,
    pub applied_at: String,
}

#[derive(Debug)]
pub struct Status {
    pub applied: Vec<MigrationRecord>,
    pub pending: Vec<String>,
}

pub async fn applied(db: &Db) -> Result<Vec<MigrationRecord>, Error> {
    ensure_tracking_table(db).await?;
    let rows = sqlx::query(&format!(
        "SELECT filename, applied_at FROM {TRACKING_TABLE} ORDER BY filename"
    ))
    .fetch_all(db.pool())
    .await?;
    Ok(rows
        .iter()
        .map(|r| MigrationRecord {
            filename: r.get(0),
            applied_at: r.get(1),
        })
        .collect())
}

pub async fn status(db: &Db, dir: &Path) -> Result<Status, Error> {
    let applied_records = applied(db).await?;
    let applied_names: HashSet<String> =
        applied_records.iter().map(|r| r.filename.clone()).collect();
    let files = list(dir)?;
    let pending: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
        .filter(|n| !applied_names.contains(n))
        .collect();
    Ok(Status {
        applied: applied_records,
        pending,
    })
}

pub async fn apply(db: &Db, dir: &Path) -> Result<Vec<String>, Error> {
    ensure_tracking_table(db).await?;

    let rows = sqlx::query(&format!("SELECT filename FROM {TRACKING_TABLE}"))
        .fetch_all(db.pool())
        .await?;
    let already_applied: HashSet<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();

    let files = list(dir)?;
    let mut newly_applied = Vec::new();

    for path in files {
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if already_applied.contains(&filename) {
            continue;
        }

        let sql = fs::read_to_string(&path)
            .map_err(|e| Error::Internal(format!("reading {filename}: {e}")))?;

        let mut tx = db.pool().begin().await?;
        for stmt in split_sql(&sql) {
            sqlx::query(stmt)
                .execute(&mut *tx)
                .await
                .map_err(|e| Error::Internal(format!("migration {filename} failed: {e}")))?;
        }
        sqlx::query(&format!(
            "INSERT INTO {TRACKING_TABLE} (filename) VALUES (?)"
        ))
        .bind(&filename)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        newly_applied.push(filename);
    }

    Ok(newly_applied)
}

async fn ensure_tracking_table(db: &Db) -> Result<(), Error> {
    db.execute(&format!(
        "CREATE TABLE IF NOT EXISTS {TRACKING_TABLE} (
            filename TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"
    ))
    .await
}

fn next_number(files: &[PathBuf]) -> u32 {
    files
        .iter()
        .filter_map(|p| p.file_name()?.to_str())
        .filter_map(|name| {
            let (prefix, _) = name.split_once('_')?;
            prefix.parse::<u32>().ok()
        })
        .max()
        .map(|n| n + 1)
        .unwrap_or(1)
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    let mut last_sep = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_sep = false;
        } else if !last_sep {
            out.push('_');
            last_sep = true;
        }
    }
    out.trim_matches('_').to_string()
}

fn split_sql(sql: &str) -> Vec<&str> {
    sql.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rustio-mig-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn sanitize_lowercases_and_underscores() {
        assert_eq!(sanitize_name("Add Blog Table"), "add_blog_table");
        assert_eq!(sanitize_name("create-users-table"), "create_users_table");
        assert_eq!(sanitize_name("add  spaces"), "add_spaces");
        assert_eq!(sanitize_name("CamelCase"), "camelcase");
    }

    #[test]
    fn sanitize_trims_outer_separators() {
        assert_eq!(sanitize_name("_add_"), "add");
        assert_eq!(sanitize_name("--blog--"), "blog");
    }

    #[test]
    fn sanitize_empty_returns_empty() {
        assert_eq!(sanitize_name(""), "");
        assert_eq!(sanitize_name("   "), "");
        assert_eq!(sanitize_name("!!!"), "");
    }

    #[test]
    fn next_number_starts_at_one() {
        assert_eq!(next_number(&[]), 1);
    }

    #[test]
    fn next_number_follows_highest() {
        let files = vec![
            PathBuf::from("migrations/0001_first.sql"),
            PathBuf::from("migrations/0003_third.sql"),
            PathBuf::from("migrations/0002_second.sql"),
        ];
        assert_eq!(next_number(&files), 4);
    }

    #[test]
    fn next_number_ignores_non_numeric_prefixes() {
        let files = vec![
            PathBuf::from("migrations/readme.sql"),
            PathBuf::from("migrations/0005_real.sql"),
        ];
        assert_eq!(next_number(&files), 6);
    }

    #[test]
    fn split_sql_handles_multiple_statements() {
        let sql = "CREATE TABLE a (id INT); CREATE TABLE b (id INT);";
        let stmts = split_sql(sql);
        assert_eq!(
            stmts,
            vec!["CREATE TABLE a (id INT)", "CREATE TABLE b (id INT)"]
        );
    }

    #[test]
    fn split_sql_ignores_empty_trailing() {
        assert!(split_sql(";;  ;").is_empty());
    }

    #[test]
    fn generate_creates_files_with_numbered_prefixes() {
        let dir = tmp("gen");
        let p1 = generate(&dir, "create users", "-- one").unwrap();
        let p2 = generate(&dir, "add index", "-- two").unwrap();
        assert!(p1
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("0001_create_users"));
        assert!(p2
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("0002_add_index"));
        assert_eq!(fs::read_to_string(&p1).unwrap(), "-- one");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn generate_rejects_empty_name_after_sanitization() {
        let dir = tmp("gen-empty");
        assert!(matches!(
            generate(&dir, "!!!", ""),
            Err(Error::BadRequest(_))
        ));
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn apply_creates_tracking_table_even_with_no_migrations() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("apply-empty");
        fs::create_dir_all(&dir).unwrap();
        let applied = apply(&db, &dir).await.unwrap();
        assert!(applied.is_empty());
        let row = sqlx::query("SELECT COUNT(*) FROM rustio_migrations")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let count: i64 = row.get(0);
        assert_eq!(count, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn apply_runs_pending_and_is_idempotent() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("apply-idem");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("0001_create.sql"), "CREATE TABLE t (id INTEGER);").unwrap();
        fs::write(
            dir.join("0002_insert.sql"),
            "INSERT INTO t (id) VALUES (42);",
        )
        .unwrap();

        let first = apply(&db, &dir).await.unwrap();
        assert_eq!(first, vec!["0001_create.sql", "0002_insert.sql"]);

        let second = apply(&db, &dir).await.unwrap();
        assert!(second.is_empty());

        let row = sqlx::query("SELECT id FROM t")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let id: i64 = row.get(0);
        assert_eq!(id, 42);

        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn apply_picks_up_new_migration_added_later() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("apply-followup");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("0001_first.sql"),
            "CREATE TABLE first (id INTEGER);",
        )
        .unwrap();
        apply(&db, &dir).await.unwrap();

        fs::write(
            dir.join("0002_second.sql"),
            "CREATE TABLE second (id INTEGER);",
        )
        .unwrap();
        let applied = apply(&db, &dir).await.unwrap();
        assert_eq!(applied, vec!["0002_second.sql"]);

        sqlx::query("INSERT INTO first (id) VALUES (1)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO second (id) VALUES (2)")
            .execute(db.pool())
            .await
            .unwrap();

        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn status_reports_applied_and_pending_separately() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("status");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("0001_a.sql"), "CREATE TABLE a (id INTEGER);").unwrap();
        fs::write(dir.join("0002_b.sql"), "CREATE TABLE b (id INTEGER);").unwrap();
        fs::write(dir.join("0003_c.sql"), "CREATE TABLE c (id INTEGER);").unwrap();

        // Apply only 0001 and 0002 by isolating them
        fs::write(dir.join("0001_a.sql"), "CREATE TABLE a (id INTEGER);").unwrap();
        let applied_now = apply(&db, &dir).await.unwrap();
        assert_eq!(applied_now.len(), 3);

        // Add a fourth, not yet applied
        fs::write(dir.join("0004_d.sql"), "CREATE TABLE d (id INTEGER);").unwrap();

        let s = status(&db, &dir).await.unwrap();
        assert_eq!(s.applied.len(), 3);
        assert_eq!(
            s.applied
                .iter()
                .map(|r| r.filename.as_str())
                .collect::<Vec<_>>(),
            vec!["0001_a.sql", "0002_b.sql", "0003_c.sql"]
        );
        assert_eq!(s.pending, vec!["0004_d.sql"]);

        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn status_on_empty_project_returns_empty_both() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("status-empty");
        fs::create_dir_all(&dir).unwrap();
        let s = status(&db, &dir).await.unwrap();
        assert!(s.applied.is_empty());
        assert!(s.pending.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn failed_migration_rolls_back_and_is_not_marked_applied() {
        let db = Db::memory().await.unwrap();
        let dir = tmp("apply-failure");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("0001_ok.sql"), "CREATE TABLE ok (id INTEGER);").unwrap();
        fs::write(dir.join("0002_bad.sql"), "CREATE TABLE ok (id INTEGER);").unwrap(); // duplicate name → fails

        let result = apply(&db, &dir).await;
        assert!(result.is_err());

        let rows = sqlx::query("SELECT filename FROM rustio_migrations")
            .fetch_all(db.pool())
            .await
            .unwrap();
        let applied: Vec<String> = rows.iter().map(|r| r.get::<String, _>(0)).collect();
        assert_eq!(applied, vec!["0001_ok.sql"]);

        fs::remove_dir_all(&dir).ok();
    }
}
