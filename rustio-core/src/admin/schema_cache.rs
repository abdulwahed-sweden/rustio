//! Process-local schema cache — 0.7.2.
//!
//! The planner / review / executor chain already reads
//! `rustio.schema.json` on every invocation. What was missing was a
//! *runtime-visible* reload: the dashboard and suggestion engine
//! baked their decisions against the compiled `AdminEntry` list, so
//! after `rustio schema` regenerated the file or after an
//! `ai apply`, the admin kept showing stale suggestions until the
//! operator restarted the server.
//!
//! This module gives the admin a single refreshable source of truth:
//! a `RwLock<Option<Schema>>` behind a `OnceLock`. The suggestion
//! engine reads from here; the `/admin/schema/reload` handler writes
//! to here; the apply handler writes to here automatically on
//! success. No restart required.
//!
//! ## Safety
//!
//! - The cache stores only a parsed [`Schema`] — no handlers, no
//!   mutable project state. Reloading a bad file returns `Err`; the
//!   previous good value stays in the cache.
//! - Reads take a `RwLock` read guard. Writes take a write guard
//!   and swap the inner `Option`. A poisoned lock falls back to
//!   "cache empty" rather than panic.
//! - The cache is pure data — it cannot cause file writes, DB
//!   access, or any side effect.

use std::path::Path;
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

use crate::schema::Schema;

/// Lazily-initialised global. `None` inside the RwLock means the
/// schema file wasn't present or didn't parse — still a valid
/// state; the admin degrades to compile-time fallbacks.
fn cell() -> &'static RwLock<Option<CachedSchema>> {
    static INSTANCE: OnceLock<RwLock<Option<CachedSchema>>> = OnceLock::new();
    INSTANCE.get_or_init(|| RwLock::new(initial_load()))
}

#[derive(Debug, Clone)]
pub struct CachedSchema {
    pub schema: Schema,
    /// Wall-clock timestamp of the load that produced this cached
    /// value, for the dashboard's "schema loaded at …" line.
    pub loaded_at: SystemTime,
}

fn initial_load() -> Option<CachedSchema> {
    read_current_schema_file().ok().map(|schema| CachedSchema {
        schema,
        loaded_at: SystemTime::now(),
    })
}

/// Read `rustio.schema.json` from disk and return the parsed value.
/// Caller decides whether to update the cache.
fn read_current_schema_file() -> Result<Schema, String> {
    let path = Path::new("rustio.schema.json");
    if !path.exists() {
        return Err("rustio.schema.json not found".into());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read error: {e}"))?;
    Schema::parse(&raw).map_err(|e| format!("parse error: {e}"))
}

/// Snapshot of the currently-cached schema, or `None` if the file
/// is missing / unreadable / unparsable.
pub fn snapshot() -> Option<CachedSchema> {
    cell().read().ok().and_then(|g| g.clone())
}

/// Re-read `rustio.schema.json` and atomically replace the cached
/// value. Returns the freshly-loaded schema on success; leaves the
/// previous value intact on any error.
pub fn refresh() -> Result<CachedSchema, String> {
    let fresh = read_current_schema_file()?;
    let mut guard = cell()
        .write()
        .map_err(|_| "schema cache lock is poisoned — restart the server to recover".to_string())?;
    let cached = CachedSchema {
        schema: fresh,
        loaded_at: SystemTime::now(),
    };
    *guard = Some(cached.clone());
    Ok(cached)
}

/// Like [`refresh`] but does not surface errors. Used by the apply
/// handler to auto-reload after a successful write; the user-visible
/// [`refresh`] endpoint returns a clear error on failure.
pub fn refresh_best_effort() {
    let _ = refresh();
}

/// Formats `loaded_at` as `YYYY-MM-DD HH:MM:SS UTC` for the
/// dashboard footer.
pub fn format_loaded_at(t: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Utc> = t.into();
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_loaded_at_produces_stable_shape() {
        // Pin a known epoch so the assertion doesn't depend on wall
        // clock; verifies the format string doesn't regress.
        let t = SystemTime::UNIX_EPOCH;
        let s = format_loaded_at(t);
        assert_eq!(s, "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn snapshot_returns_same_value_when_not_refreshed() {
        // The cache value is immutable between refreshes, so two
        // consecutive snapshots see byte-identical data.
        let a = snapshot();
        let b = snapshot();
        // Compare the schema only — loaded_at is identical too
        // because refresh wasn't called between the two reads.
        assert_eq!(a.map(|c| c.schema), b.map(|c| c.schema),);
    }
}
