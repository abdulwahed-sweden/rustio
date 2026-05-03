//! Background update check for `rustio-cli`.
//!
//! Synchronous read of a small JSON cache at `~/.rustio/version-check.json`,
//! detached background thread for the network refresh. Adds zero
//! perceptible latency to startup. Every error path is silent — failure
//! to detect an update is a non-event.
//!
//! Hard rules:
//! * Never panic. Never `eprintln!` from an error path.
//! * Never block the caller (the network call happens on a detached thread).
//! * The "update available" banner prints at most once every 24h
//!   (`last_notified_unix` in the cache enforces this).
//! * Disabled by `RUSTIO_NO_UPDATE_CHECK=1`, by `CI=1`/`CI=true`, and
//!   when the user runs `rustio doctor` (doctor reports its own
//!   version status as part of its diagnostics output).
//!
//! Cache shape:
//! ```json
//! {
//!   "checked_at_unix":         1714771200,
//!   "last_notified_unix":      1714771200,
//!   "latest_version":          "1.7.1",
//!   "current_version_at_check": "1.7.1"
//! }
//! ```
//!
//! `current_version_at_check` lets a fresh-install or post-upgrade run
//! invalidate stale cache contents — when the live `CARGO_PKG_VERSION`
//! differs from what was on disk, the refresh thread also resets
//! `last_notified_unix` to 0 so the user can be notified immediately
//! the next time a newer release ships.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CRATE_NAME: &str = "rustio-cli";
const CRATES_IO_URL: &str = "https://crates.io/api/v1/crates/rustio-cli";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const NOTIFY_TTL_SECS: u64 = 24 * 60 * 60;
const HTTP_TIMEOUT_SECS: u64 = 5;

#[derive(Clone, Debug)]
struct Cache {
    checked_at_unix: u64,
    last_notified_unix: u64,
    latest_version: String,
    current_version_at_check: String,
}

/// Entry point. Cheap on the hot path: one cache read, possibly one
/// cache write, possibly a thread spawn. Returns immediately.
pub fn run() {
    if std::env::var_os("RUSTIO_NO_UPDATE_CHECK").is_some() {
        return;
    }
    if is_ci() {
        return;
    }
    if first_subcommand().as_deref() == Some("doctor") {
        return;
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    let now = now_unix();
    let cache = read_cache();

    // Synchronous: print the banner if applicable, persist
    // last_notified so we don't re-spam within NOTIFY_TTL_SECS.
    if let Some(c) = cache.as_ref() {
        if c.current_version_at_check == current
            && should_notify(&current, &c.latest_version).is_some()
            && now.saturating_sub(c.last_notified_unix) >= NOTIFY_TTL_SECS
        {
            print_notice(&current, &c.latest_version);
            let updated = Cache {
                last_notified_unix: now,
                ..c.clone()
            };
            let _ = write_cache(&updated);
        }
    }

    // Background refresh if cache is missing, stale, or recorded
    // against a different CLI version than the one running.
    let needs_refresh = match cache {
        None => true,
        Some(ref c) => {
            now.saturating_sub(c.checked_at_unix) >= CACHE_TTL_SECS
                || c.current_version_at_check != current
        }
    };
    if needs_refresh {
        std::thread::spawn(move || {
            // Re-read so we preserve `last_notified` across the
            // fetch — except when the live version doesn't match
            // the cached one (post-upgrade, the slate is clean).
            let preserved_last_notified = match read_cache() {
                Some(c) if c.current_version_at_check == current => c.last_notified_unix,
                _ => 0,
            };
            if let Some(latest) = fetch_latest() {
                let new = Cache {
                    checked_at_unix: now_unix(),
                    last_notified_unix: preserved_last_notified,
                    latest_version: latest,
                    current_version_at_check: current,
                };
                let _ = write_cache(&new);
            }
        });
    }
}

fn should_notify(current: &str, latest: &str) -> Option<(String, String)> {
    let c = semver::Version::parse(current).ok()?;
    let l = semver::Version::parse(latest).ok()?;
    if l > c {
        Some((current.to_string(), latest.to_string()))
    } else {
        None
    }
}

fn print_notice(current: &str, latest: &str) {
    eprintln!("⚡ Update available: {CRATE_NAME} {current} → {latest}");
    eprintln!("Run: cargo install {CRATE_NAME} --force");
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".rustio").join("version-check.json"))
}

fn read_cache() -> Option<Cache> {
    let path = cache_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    parse_cache(&raw)
}

fn parse_cache(raw: &str) -> Option<Cache> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    Some(Cache {
        checked_at_unix: v.get("checked_at_unix")?.as_u64()?,
        last_notified_unix: v.get("last_notified_unix")?.as_u64()?,
        latest_version: v.get("latest_version")?.as_str()?.to_string(),
        current_version_at_check: v
            .get("current_version_at_check")?
            .as_str()?
            .to_string(),
    })
}

fn write_cache(c: &Cache) -> std::io::Result<()> {
    let Some(path) = cache_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let body = serde_json::json!({
        "checked_at_unix": c.checked_at_unix,
        "last_notified_unix": c.last_notified_unix,
        "latest_version": c.latest_version,
        "current_version_at_check": c.current_version_at_check,
    });
    let serialized = serde_json::to_string(&body).unwrap_or_default();
    std::fs::write(&path, serialized)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn fetch_latest() -> Option<String> {
    let user_agent = format!(
        "{CRATE_NAME}/{} (https://github.com/abdulwahed-sweden/rustio)",
        env!("CARGO_PKG_VERSION")
    );
    let resp = ureq::get(CRATES_IO_URL)
        .set("User-Agent", &user_agent)
        .set("Accept", "application/json")
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .call()
        .ok()?;
    let body = resp.into_string().ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    Some(
        v.get("crate")?
            .get("max_stable_version")?
            .as_str()?
            .to_string(),
    )
}

fn ci_truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true" | "TRUE" | "True"))
}

fn is_ci() -> bool {
    ci_truthy(std::env::var("CI").ok().as_deref())
}

fn first_subcommand() -> Option<String> {
    std::env::args().skip(1).find(|a| !a.starts_with('-'))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_notify_returns_none_when_equal() {
        assert!(should_notify("1.7.1", "1.7.1").is_none());
    }

    #[test]
    fn should_notify_returns_some_when_older() {
        let n = should_notify("1.7.0", "1.7.1");
        assert!(n.is_some());
        let (c, l) = n.unwrap();
        assert_eq!(c, "1.7.0");
        assert_eq!(l, "1.7.1");
    }

    #[test]
    fn should_notify_returns_none_when_current_is_newer() {
        assert!(should_notify("2.0.0", "1.7.1").is_none());
    }

    #[test]
    fn should_notify_handles_minor_to_major_bump() {
        assert!(should_notify("1.99.99", "2.0.0").is_some());
    }

    #[test]
    fn should_notify_silent_on_garbage_input() {
        assert!(should_notify("not-a-version", "1.7.1").is_none());
        assert!(should_notify("1.7.0", "garbage").is_none());
    }

    #[test]
    fn ci_truthy_recognises_canonical_values() {
        assert!(ci_truthy(Some("1")));
        assert!(ci_truthy(Some("true")));
        assert!(ci_truthy(Some("TRUE")));
        assert!(ci_truthy(Some("True")));
        assert!(!ci_truthy(Some("yes")));
        assert!(!ci_truthy(Some("0")));
        assert!(!ci_truthy(Some("")));
        assert!(!ci_truthy(None));
    }

    #[test]
    fn parse_cache_round_trips_via_json_value() {
        let raw = r#"{
            "checked_at_unix": 1714771200,
            "last_notified_unix": 1714771199,
            "latest_version": "1.7.1",
            "current_version_at_check": "1.7.0"
        }"#;
        let c = parse_cache(raw).expect("valid cache parses");
        assert_eq!(c.checked_at_unix, 1714771200);
        assert_eq!(c.last_notified_unix, 1714771199);
        assert_eq!(c.latest_version, "1.7.1");
        assert_eq!(c.current_version_at_check, "1.7.0");
    }

    #[test]
    fn parse_cache_returns_none_on_missing_field() {
        let raw = r#"{"checked_at_unix": 1, "latest_version": "1.0.0"}"#;
        assert!(parse_cache(raw).is_none());
    }

    #[test]
    fn parse_cache_returns_none_on_garbage() {
        assert!(parse_cache("not-json").is_none());
        assert!(parse_cache("").is_none());
    }
}
