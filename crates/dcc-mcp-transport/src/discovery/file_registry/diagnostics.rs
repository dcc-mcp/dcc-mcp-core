//! Durable, process-scoped diagnostics for registry write transactions.
//!
//! Each writer process owns one versioned JSON record under the registry
//! directory. Updates are published with an atomic replace, so a doctor
//! process can read a consistent snapshot without taking the registry lock.

use std::{
    collections::HashMap,
    fs,
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Arc,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};

pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;
const DIAGNOSTICS_DIR: &str = "registry-write-diagnostics";
#[cfg(test)]
pub(crate) const MAX_STALE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
#[cfg(not(test))]
const MAX_STALE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const MAX_STALE_REMOVALS_PER_SNAPSHOT: usize = 16;
pub(crate) const SLOW_WAIT_MS: u64 = 250;
static WRITER_UPDATE_LOCK: Mutex<()> = Mutex::new(());
static PUBLISH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteDiagnosticsSnapshot {
    pub heartbeat_lock_timeouts: u64,
    pub last_heartbeat_write_ms: Option<u64>,
    pub registry_lock_wait_ms: Option<u64>,
    #[serde(default)]
    pub other_write_lock_timeouts: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriterRecord {
    schema_version: u32,
    pid: u32,
    #[serde(default)]
    pub process_start_ms: Option<u64>,
    started_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    #[serde(default)]
    pub warning_episode_open: bool,
    counters: WriteDiagnosticsSnapshot,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct WriterUpdate {
    pub is_heartbeat: bool,
    pub is_registry_lock: bool,
    pub lock_timeout: bool,
    pub lock_wait_ms: Option<u64>,
    pub heartbeat_write_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WarningDecision {
    #[default]
    Suppress,
    Emit,
}

pub struct WriterDiagnostics {
    path: PathBuf,
    session: Arc<Mutex<WriterRecord>>,
}

/// One mutable record is shared by every handle for the same writer path.
static WRITER_SESSIONS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<WriterRecord>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
pub fn process_start_ms(registry_dir: &Path) -> Option<u64> {
    let path = registry_dir.join(DIAGNOSTICS_DIR).join(format!(
        "v{DIAGNOSTICS_SCHEMA_VERSION}-writer-{}.json",
        std::process::id()
    ));
    read_record(&path).and_then(|record| record.process_start_ms)
}

#[cfg(test)]
pub fn process_identity_for_current_process() -> Option<u64> {
    process_identity(std::process::id())
}

#[cfg(test)]
pub fn write_test_record(
    registry_dir: &Path,
    started_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    process_start_ms: Option<u64>,
) {
    let path = registry_dir.join(DIAGNOSTICS_DIR).join(format!(
        "v{DIAGNOSTICS_SCHEMA_VERSION}-writer-{}.json",
        std::process::id()
    ));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let record = WriterRecord {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        pid: std::process::id(),
        process_start_ms,
        started_at_unix_ms,
        updated_at_unix_ms,
        warning_episode_open: false,
        counters: WriteDiagnosticsSnapshot {
            heartbeat_lock_timeouts: 7,
            ..WriteDiagnosticsSnapshot::default()
        },
    };
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
}

impl WriterDiagnostics {
    pub fn new(registry_dir: &Path) -> Self {
        let path = registry_dir.join(DIAGNOSTICS_DIR).join(format!(
            "v{DIAGNOSTICS_SCHEMA_VERSION}-writer-{}.json",
            std::process::id()
        ));
        let session = WRITER_SESSIONS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(load_or_create_record(&path))))
            .clone();
        Self { path, session }
    }

    pub fn record(&mut self, update: WriterUpdate) -> WarningDecision {
        let _guard = WRITER_UPDATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut record = self
            .session
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(current) = read_record(&self.path)
            && current.pid == record.pid
            && current.process_start_ms == record.process_start_ms
            && current.updated_at_unix_ms > record.updated_at_unix_ms
        {
            *record = current;
        }

        let now = now_ms();
        let counters = &mut record.counters;
        if update.lock_timeout && update.is_registry_lock {
            if update.is_heartbeat {
                counters.heartbeat_lock_timeouts += 1;
            } else {
                counters.other_write_lock_timeouts += 1;
            }
        }
        if let Some(ms) = update.lock_wait_ms {
            counters.registry_lock_wait_ms = max_value(counters.registry_lock_wait_ms, Some(ms));
        }
        if let Some(ms) = update.heartbeat_write_ms {
            counters.last_heartbeat_write_ms = Some(ms);
        }

        let noisy = update.lock_timeout || update.lock_wait_ms.is_some_and(|ms| ms >= SLOW_WAIT_MS);
        let should_emit = noisy && !record.warning_episode_open;
        if noisy {
            record.warning_episode_open = true;
        } else if update.lock_wait_ms.is_some() {
            record.warning_episode_open = false;
        }
        record.updated_at_unix_ms = now;
        self.publish(&record).ok();

        if should_emit {
            WarningDecision::Emit
        } else {
            WarningDecision::Suppress
        }
    }

    fn publish(&self, record: &WriterRecord) -> io::Result<()> {
        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "diagnostics path has no parent")
        })?;
        fs::create_dir_all(parent)?;
        let sequence = PUBLISH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .path
            .with_extension(format!("tmp-{}-{sequence}", std::process::id()));
        let content = serde_json::to_vec(record)
            .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&content)?;
        file.sync_all()?;
        drop(file);
        if let Err(error) = atomic_replace(&temp, &self.path) {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }
}

fn load_or_create_record(path: &Path) -> WriterRecord {
    let _guard = WRITER_UPDATE_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let pid = std::process::id();
    let process_start_ms = process_identity(pid);
    let now = now_ms();
    let existing = read_record(path);
    let reusable = existing.as_ref().is_some_and(|record| {
        record.schema_version == DIAGNOSTICS_SCHEMA_VERSION
            && record.pid == pid
            && record.process_start_ms == process_start_ms
    });
    if reusable {
        existing.unwrap_or_default()
    } else {
        WriterRecord {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            pid,
            process_start_ms,
            started_at_unix_ms: now,
            updated_at_unix_ms: now,
            warning_episode_open: false,
            counters: WriteDiagnosticsSnapshot::default(),
        }
    }
}
pub fn snapshot(registry_dir: &Path) -> WriteDiagnosticsSnapshot {
    let dir = registry_dir.join(DIAGNOSTICS_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return WriteDiagnosticsSnapshot::default();
    };
    let now = now_ms();
    let prefix = format!("v{DIAGNOSTICS_SCHEMA_VERSION}-writer-");
    let mut aggregate = WriteDiagnosticsSnapshot::default();
    let mut stale_removals = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.contains(".tmp-") {
            if stale_removals < MAX_STALE_REMOVALS_PER_SNAPSHOT && is_stale_temp(&path, now) {
                let _ = fs::remove_file(&path);
                stale_removals += 1;
            }
            continue;
        }
        if !name.starts_with(&prefix) || !name.ends_with(".json") {
            continue;
        }
        let Some(record) = read_record(&path) else {
            continue;
        };
        if is_stale(&record, now) {
            if stale_removals < MAX_STALE_REMOVALS_PER_SNAPSHOT {
                let _ = fs::remove_file(&path);
                stale_removals += 1;
            }
            continue;
        }
        aggregate.heartbeat_lock_timeouts = aggregate
            .heartbeat_lock_timeouts
            .saturating_add(record.counters.heartbeat_lock_timeouts);
        aggregate.other_write_lock_timeouts = aggregate
            .other_write_lock_timeouts
            .saturating_add(record.counters.other_write_lock_timeouts);
        aggregate.last_heartbeat_write_ms = max_value(
            aggregate.last_heartbeat_write_ms,
            record.counters.last_heartbeat_write_ms,
        );
        aggregate.registry_lock_wait_ms = max_value(
            aggregate.registry_lock_wait_ms,
            record.counters.registry_lock_wait_ms,
        );
    }
    aggregate
}

fn read_record(path: &Path) -> Option<WriterRecord> {
    let content = fs::read_to_string(path).ok()?;
    let record: WriterRecord = serde_json::from_str(&content).ok()?;
    (record.schema_version == DIAGNOSTICS_SCHEMA_VERSION && record.pid > 0).then_some(record)
}

fn is_stale(record: &WriterRecord, now: u64) -> bool {
    let Some(current_start) = process_identity(record.pid) else {
        return true;
    };
    if record
        .process_start_ms
        .is_some_and(|start| start != current_start)
    {
        return true;
    }
    record.updated_at_unix_ms > 0
        && now.saturating_sub(record.updated_at_unix_ms) > MAX_STALE_AGE.as_millis() as u64
}

fn is_stale_temp(path: &Path, now: u64) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    let Ok(modified_ms) = modified.duration_since(UNIX_EPOCH) else {
        return false;
    };
    now.saturating_sub(modified_ms.as_millis().min(u64::MAX as u128) as u64)
        > MAX_STALE_AGE.as_millis() as u64
}

/// Return the OS process start token, which also proves the PID is alive.
fn process_identity(pid: u32) -> Option<u64> {
    let process_id = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    system
        .process(process_id)
        .map(|process| process.start_time())
}

fn atomic_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from = from
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let to = to
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(from, to)
    }
}

fn max_value(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
pub fn reset_counts(registry_dir: &Path) {
    let path = registry_dir.join(DIAGNOSTICS_DIR).join(format!(
        "v{DIAGNOSTICS_SCHEMA_VERSION}-writer-{}.json",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    WRITER_SESSIONS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&path);
}
