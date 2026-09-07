use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    CLI_BINARY_NAME, CLI_VERSION, FAILURE_RETRY_INTERVAL_SECS, SUCCESS_CHECK_INTERVAL_SECS,
    mark_cached,
};

const CACHE_SCHEMA_VERSION: u8 = 1;
const BACKGROUND_ENV: &str = "DCC_MCP_UPDATE_BACKGROUND";
const DISABLE_ENV: &str = "DCC_MCP_DISABLE_UPDATE_CHECK";
const FORCE_ENV: &str = "DCC_MCP_FORCE_UPDATE_CHECK";
const CACHE_DIR_ENV: &str = "DCC_MCP_UPDATE_CACHE_DIR";
const JUST_APPLIED_ENV: &str = "DCC_MCP_UPDATE_JUST_APPLIED";

#[derive(Debug, Default)]
pub struct CachedCliUpdate {
    pub notification: Option<Value>,
    pub refresh_due: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheRecord {
    schema_version: u8,
    binary_name: String,
    current_version: String,
    checked_at_unix_secs: u64,
    successful: bool,
    payload: Value,
}

pub fn cached_cli_update() -> CachedCliUpdate {
    if !automatic_checks_enabled() || is_background_refresh() {
        return CachedCliUpdate::default();
    }
    inspect_cache(&cache_path(), unix_timestamp(), CLI_VERSION)
}

pub fn record_cli_update(payload: &Value) {
    let path = cache_path();
    let _ = write_cache(&path, payload, unix_timestamp(), CLI_VERSION);
    let _ = std::fs::remove_file(refresh_lock_path(&path));
}

pub fn is_background_refresh() -> bool {
    env_truthy(BACKGROUND_ENV)
}

pub fn just_applied_status() -> Option<Value> {
    env_truthy(JUST_APPLIED_ENV).then(|| {
        serde_json::json!({
            "version_status": "applied",
            "binary_name": CLI_BINARY_NAME,
            "current_version": CLI_VERSION,
            "latest_version": CLI_VERSION,
            "update_available": false,
            "applied_on_this_launch": true,
            "update_policy": {
                "requires_user_confirmation": true,
                "apply_supported_by_this_command": true,
                "apply_command": "dcc-mcp-cli update apply --yes",
                "staged_for_next_launch": true,
                "running_server_restarted": false,
            }
        })
    })
}

pub fn spawn_cli_update_refresh(base_url: &str) -> bool {
    if !automatic_checks_enabled() || is_background_refresh() {
        return false;
    }
    let cache = cache_path();
    let lock = refresh_lock_path(&cache);
    if !reserve_refresh(&lock, unix_timestamp()) {
        return false;
    }
    let Ok(executable) = std::env::current_exe() else {
        let _ = std::fs::remove_file(lock);
        return false;
    };
    let mut command = Command::new(executable);
    command
        .args([
            "--base-url",
            base_url,
            "--output",
            "json",
            "update",
            "check",
        ])
        .env(BACKGROUND_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    if command.spawn().is_ok() {
        true
    } else {
        let _ = std::fs::remove_file(lock);
        false
    }
}

fn reserve_refresh(path: &Path, now: u64) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    if let Ok(metadata) = std::fs::metadata(path)
        && let Ok(modified) = metadata.modified()
    {
        let age = std::time::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(now))
            .and_then(|current| current.duration_since(modified).ok())
            .map_or(0, |duration| duration.as_secs());
        if age < FAILURE_RETRY_INTERVAL_SECS {
            return false;
        }
        let _ = std::fs::remove_file(path);
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .is_ok()
}

fn inspect_cache(path: &Path, now: u64, current_version: &str) -> CachedCliUpdate {
    let Ok(bytes) = std::fs::read(path) else {
        return CachedCliUpdate {
            refresh_due: true,
            ..CachedCliUpdate::default()
        };
    };
    let Ok(record) = serde_json::from_slice::<CacheRecord>(&bytes) else {
        return CachedCliUpdate {
            refresh_due: true,
            ..CachedCliUpdate::default()
        };
    };
    if record.schema_version != CACHE_SCHEMA_VERSION
        || record.binary_name != CLI_BINARY_NAME
        || record.current_version != current_version
    {
        return CachedCliUpdate {
            refresh_due: true,
            ..CachedCliUpdate::default()
        };
    }
    let age = now.saturating_sub(record.checked_at_unix_secs);
    let ttl = if record.successful {
        SUCCESS_CHECK_INTERVAL_SECS
    } else {
        FAILURE_RETRY_INTERVAL_SECS
    };
    if age >= ttl {
        return CachedCliUpdate {
            refresh_due: true,
            ..CachedCliUpdate::default()
        };
    }
    let notification = (record.successful
        && record.payload["update_available"].as_bool() == Some(true))
    .then(|| mark_cached(record.payload, age));
    CachedCliUpdate {
        notification,
        refresh_due: false,
    }
}

fn write_cache(
    path: &Path,
    payload: &Value,
    checked_at_unix_secs: u64,
    current_version: &str,
) -> anyhow::Result<()> {
    let record = CacheRecord {
        schema_version: CACHE_SCHEMA_VERSION,
        binary_name: CLI_BINARY_NAME.into(),
        current_version: current_version.into(),
        checked_at_unix_secs,
        successful: payload["version_status"] != "check_failed" && payload.get("error").is_none(),
        payload: payload.clone(),
    };
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("update cache path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut temporary, &record)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| anyhow::Error::from(error.error))?;
    Ok(())
}

fn cache_path() -> PathBuf {
    let root = std::env::var_os(CACHE_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| std::env::temp_dir().join("dcc-mcp-cache"));
    root.join("dcc-mcp").join("update").join("cli-check.json")
}

fn refresh_lock_path(cache: &Path) -> PathBuf {
    cache.with_extension("lock")
}

fn automatic_checks_enabled() -> bool {
    if env_truthy(DISABLE_ENV) {
        return false;
    }
    !cfg!(debug_assertions) || env_truthy(FORCE_ENV)
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available_payload() -> Value {
        serde_json::json!({
            "version_status": "update_available",
            "update_available": true,
            "current_version": "0.20.22",
            "latest_version": "0.20.23",
            "check": {"source": "live", "cache_age_secs": 0},
        })
    }

    #[test]
    fn fresh_available_cache_becomes_agent_notification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check.json");
        write_cache(&path, &available_payload(), 100, CLI_VERSION).unwrap();
        write_cache(&path, &available_payload(), 100, CLI_VERSION).unwrap();

        let cached = inspect_cache(&path, 130, CLI_VERSION);

        assert!(!cached.refresh_due);
        let notice = cached.notification.unwrap();
        assert_eq!(notice["check"]["source"], "cache");
        assert_eq!(notice["check"]["cache_age_secs"], 30);
    }

    #[test]
    fn stale_or_other_version_cache_refreshes_without_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check.json");
        write_cache(&path, &available_payload(), 100, CLI_VERSION).unwrap();

        assert!(inspect_cache(&path, 100 + SUCCESS_CHECK_INTERVAL_SECS, CLI_VERSION).refresh_due);
        assert!(inspect_cache(&path, 101, "99.0.0").refresh_due);
    }

    #[test]
    fn failures_are_quiet_and_use_shorter_retry_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check.json");
        let failure = serde_json::json!({
            "version_status": "check_failed",
            "error": "update_check_failed",
            "update_available": false,
        });
        write_cache(&path, &failure, 100, CLI_VERSION).unwrap();

        let fresh = inspect_cache(&path, 100 + FAILURE_RETRY_INTERVAL_SECS - 1, CLI_VERSION);
        assert!(!fresh.refresh_due);
        assert!(fresh.notification.is_none());
        assert!(inspect_cache(&path, 100 + FAILURE_RETRY_INTERVAL_SECS, CLI_VERSION).refresh_due);
    }

    #[test]
    fn refresh_reservation_is_single_flight() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check.lock");

        assert!(reserve_refresh(&path, unix_timestamp()));
        assert!(!reserve_refresh(&path, unix_timestamp()));
    }
}
