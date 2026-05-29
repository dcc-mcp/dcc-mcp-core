//! File-based service registry.
//!
//! Stores service entries as JSON in a registry directory.
//! Uses `(dcc_type, instance_id)` as key to support multiple instances per DCC type.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use fs4::{FileExt, TryLockError};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tracing;

use super::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceKey, ServiceStatus};
use crate::error::{TransportError, TransportResult};

/// Return `true` when `pid` refers to a currently running OS process.
///
/// Used by [`FileRegistry::prune_dead_pids`] to detect ghost entries left behind
/// when a DCC plugin crashes after registering but before the heartbeat loop
/// starts. See issue #227.
fn is_pid_alive(pid: u32) -> bool {
    let sp = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[sp]), true);
    sys.process(sp).is_some()
}

/// File name for the registry JSON.
const REGISTRY_FILE: &str = "services.json";
const REGISTRY_LOCK_FILE: &str = "services.lock";
const LOCKS_DIR: &str = "locks";
const REGISTRY_LOCK_TIMEOUT_ENV: &str = "DCC_MCP_REGISTRY_LOCK_TIMEOUT_MS";
const REGISTRY_LOCK_BACKOFF_ENV: &str = "DCC_MCP_REGISTRY_LOCK_BACKOFF_MS";
const DEFAULT_REGISTRY_LOCK_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_REGISTRY_LOCK_BACKOFF_MS: u64 = 10;
const REGISTRY_LOCK_SLOW_WARN_MS: u64 = 250;

type EntryMap = HashMap<ServiceKey, ServiceEntry>;

fn env_duration_ms(name: &str, default_ms: u64) -> Duration {
    let ms = std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_ms);
    Duration::from_millis(ms)
}

/// Severity of a registry-lock acquisition wait.
///
/// Derived purely from the elapsed wait time so the decision can be
/// unit-tested without standing up a tracing subscriber. A wait that needed
/// at most a single backoff tick is `Quiet` (no log emitted): brief
/// contention between concurrent heartbeat writers is expected and not
/// actionable, so logging it — even at debug — only adds noise to the admin
/// log feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockWaitLevel {
    /// Acquired quickly enough that no log line is warranted.
    Quiet,
    /// Acquired after non-trivial retrying — useful at debug for diagnosis.
    Retry,
    /// Acquired only after a slow wait — surfaced at warn.
    Slow,
}

/// Classify how noteworthy a lock-acquisition wait was.
///
/// * `elapsed >= slow_warn` → [`LockWaitLevel::Slow`]
/// * `elapsed >= 2 * backoff` (more than one backoff tick) → [`LockWaitLevel::Retry`]
/// * otherwise → [`LockWaitLevel::Quiet`]
fn classify_lock_wait(elapsed: Duration, backoff: Duration, slow_warn: Duration) -> LockWaitLevel {
    if elapsed >= slow_warn {
        LockWaitLevel::Slow
    } else if elapsed >= backoff.saturating_mul(2) {
        LockWaitLevel::Retry
    } else {
        LockWaitLevel::Quiet
    }
}

fn entries_to_map(entries: impl IntoIterator<Item = ServiceEntry>) -> EntryMap {
    entries
        .into_iter()
        .map(|entry| (entry.key(), entry))
        .collect()
}

fn maps_equal(left: &EntryMap, right: &EntryMap) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(key, entry)| right.get(key) == Some(entry))
}

fn ensure_registry_dir(path: &Path) -> TransportResult<()> {
    fs::create_dir_all(path).map_err(|e| {
        TransportError::RegistryFile(format!(
            "failed to create registry dir {}: {}",
            path.display(),
            e
        ))
    })
}

#[cfg(test)]
type BeforeFlushHook = Box<dyn FnOnce() + Send + 'static>;

#[cfg(test)]
static BEFORE_TRANSACTION_FLUSH_HOOK: Mutex<Option<(PathBuf, BeforeFlushHook)>> = Mutex::new(None);

#[cfg(test)]
type BeforeTempSyncHook = Box<dyn FnOnce(&Path) -> std::io::Result<()> + Send + 'static>;

#[cfg(test)]
static BEFORE_TEMP_SYNC_HOOK: Mutex<Option<(PathBuf, BeforeTempSyncHook)>> = Mutex::new(None);

#[cfg(test)]
fn set_before_transaction_flush_hook(registry_dir: &Path, hook: impl FnOnce() + Send + 'static) {
    let mut slot = BEFORE_TRANSACTION_FLUSH_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *slot = Some((registry_dir.to_path_buf(), Box::new(hook)));
}

#[cfg(test)]
fn run_before_transaction_flush_hook(registry_dir: &Path) {
    let hook = {
        let mut slot = BEFORE_TRANSACTION_FLUSH_HOOK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let should_run = slot
            .as_ref()
            .is_some_and(|(dir, _)| dir.as_path() == registry_dir);
        if should_run { slot.take() } else { None }
    };
    if let Some((_, hook)) = hook {
        hook();
    }
}

#[cfg(test)]
fn set_before_temp_sync_hook(
    registry_dir: &Path,
    hook: impl FnOnce(&Path) -> std::io::Result<()> + Send + 'static,
) {
    let mut slot = BEFORE_TEMP_SYNC_HOOK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *slot = Some((registry_dir.to_path_buf(), Box::new(hook)));
}

#[cfg(test)]
fn run_before_temp_sync_hook(registry_dir: &Path, temp_path: &Path) -> std::io::Result<()> {
    let hook = {
        let mut slot = BEFORE_TEMP_SYNC_HOOK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let should_run = slot
            .as_ref()
            .is_some_and(|(dir, _)| dir.as_path() == registry_dir);
        if should_run { slot.take() } else { None }
    };
    if let Some((_, hook)) = hook {
        hook(temp_path)?;
    }
    Ok(())
}

/// File-based service registry with instance-level keying.
///
/// Key improvement over the Python implementation: uses `(dcc_type, instance_id)` as key
/// instead of `dcc_type` alone, enabling multiple instances of the same DCC type.
///
/// **Hot-reload feature**: Detects external writes to services.json via mtime tracking.
/// When another process writes to the registry file, this process automatically reloads
/// the new entries without requiring a restart. This enables the gateway to discover
/// instances registered by other processes (e.g., Maya plugin using McpHttpConfig.gateway_port).
pub struct FileRegistry {
    /// In-memory cache of services.
    services: DashMap<ServiceKey, ServiceEntry>,
    /// Directory where registry file is stored.
    registry_dir: PathBuf,
    /// Sentinel lock files owned by entries registered in this process.
    sentinel_handles: DashMap<ServiceKey, File>,
    /// Last-seen modification time of services.json.
    /// Used to detect external writes (hot-reload).
    last_mtime: Mutex<Option<SystemTime>>,
    /// Serialises concurrent in-process write transactions so they share a single
    /// stable temp filename (`.tmp.<pid>.services.json`) instead of
    /// generating a fresh `(pid, tid, seq)` path per write.
    ///
    /// A per-write unique filename is AV/EDR-pathological on Windows:
    /// every new filename triggers a full minifilter altitude walk
    /// (content scan + cloud reputation lookup on enterprise hosts),
    /// causing multi-second stalls on the calling thread (issue #853).
    /// uniqueness in the temp path. Cross-process writers are serialized by
    /// `services.lock`.
    write_lock: Mutex<()>,
    /// Maximum time a write transaction may wait for the in-process mutex and
    /// cross-process `services.lock` before returning an observable error.
    write_lock_timeout: Duration,
    /// Sleep interval between bounded try-lock attempts.
    write_lock_backoff: Duration,
}

impl FileRegistry {
    /// Create a new file registry at the given directory.
    pub fn new(registry_dir: impl Into<PathBuf>) -> TransportResult<Self> {
        Self::new_with_lock_policy(
            registry_dir,
            env_duration_ms(REGISTRY_LOCK_TIMEOUT_ENV, DEFAULT_REGISTRY_LOCK_TIMEOUT_MS),
            env_duration_ms(REGISTRY_LOCK_BACKOFF_ENV, DEFAULT_REGISTRY_LOCK_BACKOFF_MS),
        )
    }

    fn new_with_lock_policy(
        registry_dir: impl Into<PathBuf>,
        write_lock_timeout: Duration,
        write_lock_backoff: Duration,
    ) -> TransportResult<Self> {
        let registry_dir = registry_dir.into();
        ensure_registry_dir(&registry_dir)?;

        let registry = Self {
            services: DashMap::new(),
            registry_dir,
            sentinel_handles: DashMap::new(),
            last_mtime: Mutex::new(None),
            write_lock: Mutex::new(()),
            write_lock_timeout,
            write_lock_backoff: write_lock_backoff.max(Duration::from_millis(1)),
        };

        // Load existing entries
        registry.load_from_file()?;
        registry.update_mtime()?;

        Ok(registry)
    }

    /// Reload from disk when `services.json` changed in another process (e.g. a
    /// DCC heartbeat flush). Call this before mutating pool metadata so the
    /// in-memory view matches the file gateway and adapters share.
    pub fn refresh_from_disk(&self) -> TransportResult<()> {
        self.reload_if_stale()
    }

    /// Reload from file if another process has written to it since our last read (hot-reload).
    ///
    /// This is O(1) on the happy path: single `stat` syscall + mutex check.
    /// Only does actual file I/O when another process has modified services.json.
    fn reload_if_stale(&self) -> TransportResult<()> {
        let path = self.registry_file_path();

        // Quick stat to get current mtime
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let had_cached_file = {
                    let mut cached = self.last_mtime.lock().unwrap_or_else(|e| e.into_inner());
                    let had_cached_file = cached.is_some();
                    *cached = None;
                    had_cached_file
                };
                if had_cached_file || !self.services.is_empty() {
                    tracing::warn!(
                        path = %path.display(),
                        "FileRegistry registry file missing during hot-reload; clearing in-memory snapshot"
                    );
                    self.services.clear();
                }
                return Ok(());
            }
            Err(_) => return Ok(()),
        };
        let Ok(current_mtime) = meta.modified() else {
            // Can't get mtime, skip reload
            return Ok(());
        };

        // Compare with cached mtime. Recover from a poisoned lock: the
        // protected value is just an Option<SystemTime>, so a previous
        // panicking holder cannot leave it in an inconsistent state.
        let mut cached = self.last_mtime.lock().unwrap_or_else(|e| e.into_inner());
        if *cached == Some(current_mtime) {
            // File hasn't changed — fast path, no I/O
            return Ok(());
        }

        // File was modified by another process — drop lock and reload
        *cached = Some(current_mtime);
        drop(cached);

        // Load the new entries
        if let Err(e) = self.load_from_file() {
            tracing::warn!("FileRegistry hot-reload failed: {}", e);
        } else {
            // Downgraded to TRACE: hot-reload fires every heartbeat_secs (default 5 s)
            // because each DCC instance updates services.json on every heartbeat.
            // DEBUG level produces excessive log noise in production.
            tracing::trace!("FileRegistry hot-reloaded from disk");
        }
        Ok(())
    }

    /// Update the cached mtime to the current file modification time.
    fn update_mtime(&self) -> TransportResult<()> {
        let path = self.registry_file_path();
        if !path.exists() {
            let mut cached = self.last_mtime.lock().unwrap_or_else(|e| e.into_inner());
            *cached = None;
            return Ok(());
        }

        let Ok(meta) = fs::metadata(&path) else {
            return Ok(());
        };
        let Ok(mtime) = meta.modified() else {
            return Ok(());
        };

        let mut cached = self.last_mtime.lock().unwrap_or_else(|e| e.into_inner());
        *cached = Some(mtime);
        Ok(())
    }

    fn force_reload_from_file(&self) -> TransportResult<Vec<ServiceEntry>> {
        self.load_from_file()?;
        self.update_mtime()?;
        Ok(self.list_all_in_memory())
    }

    fn locks_dir(&self) -> PathBuf {
        self.registry_dir.join(LOCKS_DIR)
    }

    fn registry_lock_path(&self) -> PathBuf {
        self.registry_dir.join(REGISTRY_LOCK_FILE)
    }

    fn with_write_transaction<T>(
        &self,
        op: impl FnOnce() -> TransportResult<(T, bool)>,
    ) -> TransportResult<T> {
        let started = Instant::now();
        let _guard = self.acquire_process_write_lock(started)?;
        ensure_registry_dir(&self.registry_dir)?;
        let path = self.registry_lock_path();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                TransportError::RegistryFile(format!(
                    "failed to open registry lock {}: {}",
                    path.display(),
                    e
                ))
            })?;
        self.acquire_registry_file_lock(&file, &path, started)?;
        self.log_lock_wait(started.elapsed(), &path);

        let result = (|| {
            let baseline = self.force_reload_from_file()?;
            let baseline_owned_sentinels = self.owned_sentinel_keys();
            let (value, changed) = match op() {
                Ok(result) => result,
                Err(err) => {
                    self.rollback_failed_transaction(&baseline, &baseline_owned_sentinels);
                    return Err(err);
                }
            };
            if changed {
                #[cfg(test)]
                run_before_transaction_flush_hook(&self.registry_dir);
                if let Err(err) = self.flush_to_file_after_transaction(&baseline) {
                    self.rollback_failed_transaction(&baseline, &baseline_owned_sentinels);
                    return Err(err);
                }
            }
            Ok(value)
        })();
        let unlock_result = file.unlock().map_err(|e| {
            TransportError::RegistryFile(format!(
                "failed to unlock registry {}: {}",
                path.display(),
                e
            ))
        });

        match (result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    }

    fn acquire_process_write_lock(&self, started: Instant) -> TransportResult<MutexGuard<'_, ()>> {
        loop {
            match self.write_lock.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::Poisoned(err)) => return Ok(err.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    self.sleep_or_timeout(started, "in-process registry mutex")?;
                }
            }
        }
    }

    fn acquire_registry_file_lock(
        &self,
        file: &File,
        path: &Path,
        started: Instant,
    ) -> TransportResult<()> {
        loop {
            match FileExt::try_lock(file) {
                Ok(()) => return Ok(()),
                Err(TryLockError::WouldBlock) => {
                    self.sleep_or_timeout(started, "registry lock file")?;
                }
                Err(TryLockError::Error(err)) => {
                    return Err(TransportError::RegistryFile(format!(
                        "failed to lock registry {}: {}",
                        path.display(),
                        err
                    )));
                }
            }
        }
    }

    fn sleep_or_timeout(&self, started: Instant, target: &str) -> TransportResult<()> {
        let elapsed = started.elapsed();
        if elapsed >= self.write_lock_timeout {
            tracing::warn!(
                waited_ms = elapsed.as_millis() as u64,
                timeout_ms = self.write_lock_timeout.as_millis() as u64,
                target,
                "FileRegistry write transaction timed out waiting for lock"
            );
            return Err(TransportError::RegistryFile(format!(
                "timed out waiting for {target} after {}ms",
                self.write_lock_timeout.as_millis()
            )));
        }

        let remaining = self.write_lock_timeout.saturating_sub(elapsed);
        std::thread::sleep(self.write_lock_backoff.min(remaining));
        Ok(())
    }

    fn log_lock_wait(&self, elapsed: Duration, path: &Path) {
        match classify_lock_wait(
            elapsed,
            self.write_lock_backoff,
            Duration::from_millis(REGISTRY_LOCK_SLOW_WARN_MS),
        ) {
            LockWaitLevel::Slow => tracing::warn!(
                waited_ms = elapsed.as_millis() as u64,
                timeout_ms = self.write_lock_timeout.as_millis() as u64,
                lock = %path.display(),
                "FileRegistry write transaction acquired registry lock after slow wait"
            ),
            LockWaitLevel::Retry => tracing::debug!(
                waited_ms = elapsed.as_millis() as u64,
                timeout_ms = self.write_lock_timeout.as_millis() as u64,
                lock = %path.display(),
                "FileRegistry write transaction acquired registry lock after retry"
            ),
            LockWaitLevel::Quiet => {}
        }
    }

    fn sentinel_path_for(&self, key: &ServiceKey) -> PathBuf {
        self.locks_dir()
            .join(format!("{}-{}.lock", key.dcc_type, key.instance_id))
    }

    fn owned_sentinel_keys(&self) -> HashSet<ServiceKey> {
        self.sentinel_handles
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn create_sentinel(&self, key: &ServiceKey) -> TransportResult<(PathBuf, File)> {
        let locks_dir = self.locks_dir();
        fs::create_dir_all(&locks_dir).map_err(|e| {
            TransportError::RegistryFile(format!(
                "failed to create sentinel dir {}: {}",
                locks_dir.display(),
                e
            ))
        })?;
        let path = self.sentinel_path_for(key);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                TransportError::RegistryFile(format!(
                    "failed to open sentinel {}: {}",
                    path.display(),
                    e
                ))
            })?;
        FileExt::lock(&file).map_err(|e| {
            TransportError::RegistryFile(format!(
                "failed to lock sentinel {}: {}",
                path.display(),
                e
            ))
        })?;
        Ok((path, file))
    }

    fn sentinel_is_dead(&self, key: &ServiceKey, path: &Path) -> bool {
        if self.sentinel_handles.contains_key(key) {
            return false;
        }
        let file = match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return true,
            Err(err) => {
                tracing::warn!(sentinel = %path.display(), error = %err, "failed to open sentinel; keeping entry");
                return false;
            }
        };
        match FileExt::try_lock(&file) {
            Ok(()) => {
                let _ = file.unlock();
                true
            }
            Err(TryLockError::WouldBlock) => false,
            Err(TryLockError::Error(err)) => {
                tracing::warn!(sentinel = %path.display(), error = %err, "failed to probe sentinel; keeping entry");
                false
            }
        }
    }

    /// Register a service.
    pub fn register(&self, mut entry: ServiceEntry) -> TransportResult<()> {
        self.with_write_transaction(|| {
            let key = entry.key();
            tracing::info!(
                dcc_type = %entry.dcc_type,
                instance_id = %entry.instance_id,
                host = %entry.host,
                port = entry.port,
                "registering service"
            );
            let (sentinel_path, sentinel_file) = self.create_sentinel(&key)?;
            entry.sentinel_path = Some(sentinel_path);
            self.sentinel_handles.insert(key.clone(), sentinel_file);
            self.services.insert(key, entry);
            Ok(((), true))
        })
    }

    /// Deregister a service by key.
    pub fn deregister(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>> {
        self.with_write_transaction(|| {
            let removed = self.services.remove(key).map(|(_, entry)| entry);
            if let Some((_, file)) = self.sentinel_handles.remove(key) {
                let _ = file.unlock();
            }
            if let Some(entry) = &removed
                && let Some(path) = &entry.sentinel_path
            {
                let _ = fs::remove_file(path);
            }
            if removed.is_some() {
                tracing::info!(
                    dcc_type = %key.dcc_type,
                    instance_id = %key.instance_id,
                    "deregistered service"
                );
            }
            let changed = removed.is_some();
            Ok((removed, changed))
        })
    }

    /// Get a service entry by key.
    pub fn get(&self, key: &ServiceKey) -> Option<ServiceEntry> {
        self.services.get(key).map(|r| r.value().clone())
    }

    /// List all instances for a given DCC type.
    pub fn list_instances(&self, dcc_type: &str) -> Vec<ServiceEntry> {
        let _ = self.reload_if_stale();
        self.services
            .iter()
            .filter(|r| r.value().dcc_type == dcc_type)
            .map(|r| r.value().clone())
            .collect()
    }

    /// List all registered services.
    pub fn list_all(&self) -> Vec<ServiceEntry> {
        let _ = self.reload_if_stale();
        self.list_all_in_memory()
    }

    fn list_all_in_memory(&self) -> Vec<ServiceEntry> {
        self.services.iter().map(|r| r.value().clone()).collect()
    }

    /// Update heartbeat for a service.
    pub fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool> {
        self.with_write_transaction(|| {
            let found = if let Some(mut entry) = self.services.get_mut(key) {
                entry.value_mut().touch();
                true
            } else {
                false
            };
            Ok((found, found))
        })
    }

    /// Update status for a service.
    pub fn update_status(&self, key: &ServiceKey, status: ServiceStatus) -> TransportResult<bool> {
        self.with_write_transaction(|| {
            let found = if let Some(mut entry) = self.services.get_mut(key) {
                entry.value_mut().status = status;
                true
            } else {
                false
            };
            Ok((found, found))
        })
    }

    /// Acquire an optional pool lease for an idle instance.
    ///
    /// When `instance_id` is supplied it may be the full UUID or a unique prefix.
    /// Expired leases are cleared opportunistically before selection.
    pub fn acquire_lease(
        &self,
        dcc_type: &str,
        instance_id: Option<&str>,
        owner: impl Into<String>,
        current_job_id: Option<String>,
        ttl: Option<Duration>,
    ) -> TransportResult<Option<ServiceEntry>> {
        let owner = owner.into();
        self.with_write_transaction(|| {
            let now = SystemTime::now();
            let expires_at = ttl.map(|duration| now + duration);
            let mut selected: Option<ServiceEntry> = None;
            let mut changed = false;

            for mut item in self.services.iter_mut() {
                let entry = item.value_mut();
                if entry.lease_expired(now) {
                    entry.clear_lease();
                    changed = true;
                }
                if selected.is_some() || !entry.dcc_type.eq_ignore_ascii_case(dcc_type) {
                    continue;
                }
                if let Some(id) = instance_id {
                    let full = entry.instance_id.to_string();
                    if full != id && !full.starts_with(id) {
                        continue;
                    }
                }
                if entry.status == ServiceStatus::Available && entry.lease_owner.is_none() {
                    entry.acquire_lease(owner.clone(), current_job_id.clone(), expires_at);
                    selected = Some(entry.clone());
                    changed = true;
                }
            }

            Ok((selected, changed))
        })
    }

    /// Release a pool lease. When `owner` is supplied it must match the holder.
    pub fn release_lease(
        &self,
        key: &ServiceKey,
        owner: Option<&str>,
    ) -> TransportResult<Option<ServiceEntry>> {
        self.with_write_transaction(|| {
            let released = if let Some(mut entry) = self.services.get_mut(key) {
                let owner_matches = owner
                    .is_none_or(|expected| entry.value().lease_owner.as_deref() == Some(expected));
                if owner_matches && entry.value().lease_owner.is_some() {
                    entry.value_mut().clear_lease();
                    Some(entry.value().clone())
                } else {
                    None
                }
            } else {
                None
            };
            let changed = released.is_some();
            Ok((released, changed))
        })
    }

    /// Update scene and/or version metadata for a service, and refresh heartbeat.
    ///
    /// This is the primary way for a running instance to report that the user
    /// has opened a different scene (e.g. switched documents in Photoshop) or
    /// that the DCC version has changed.
    pub fn update_metadata(
        &self,
        key: &ServiceKey,
        scene: Option<&str>,
        version: Option<&str>,
    ) -> TransportResult<bool> {
        self.with_write_transaction(|| {
            let found = if let Some(mut entry) = self.services.get_mut(key) {
                let e = entry.value_mut();
                if let Some(s) = scene {
                    e.scene = if s.is_empty() {
                        None
                    } else {
                        Some(s.to_string())
                    };
                }
                if let Some(v) = version {
                    e.version = if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    };
                }
                e.touch(); // also refresh heartbeat
                true
            } else {
                false
            };
            Ok((found, found))
        })
    }

    /// Update the active document, full document list, and optional display name.
    ///
    /// Designed for multi-document DCC applications (e.g. Photoshop, After Effects)
    /// that can have several files open simultaneously. For single-document DCCs
    /// (Maya, Blender, Houdini) it is equivalent to [`update_metadata`] with the
    /// `scene` field, but also stores `pid` and `display_name` when provided.
    ///
    /// # Parameters
    /// - `active_document` — the currently focused file; stored in `scene`.
    ///   Pass `Some("")` to clear.
    /// - `documents` — full list of open documents; replaces the previous list.
    ///   Pass `&[]` to clear.
    /// - `display_name` — human-readable instance label (e.g. `"PS-Marketing"`).
    ///   Pass `Some("")` to clear.  `None` leaves the existing value unchanged.
    ///
    /// Always refreshes the heartbeat so the gateway does not mark the instance stale.
    pub fn update_documents(
        &self,
        key: &ServiceKey,
        active_document: Option<&str>,
        documents: &[String],
        display_name: Option<&str>,
    ) -> TransportResult<bool> {
        self.with_write_transaction(|| {
            let found = if let Some(mut entry) = self.services.get_mut(key) {
                let e = entry.value_mut();

                if let Some(doc) = active_document {
                    e.scene = if doc.is_empty() {
                        None
                    } else {
                        Some(doc.to_string())
                    };
                }

                // Always replace the documents list (caller owns the authoritative set).
                e.documents = documents
                    .iter()
                    .filter(|d| !d.is_empty())
                    .cloned()
                    .collect();

                if let Some(name) = display_name {
                    e.display_name = if name.is_empty() {
                        None
                    } else {
                        Some(name.to_string())
                    };
                }

                e.touch();
                true
            } else {
                false
            };

            Ok((found, found))
        })
    }

    /// Set the OS process ID for a registered service.
    ///
    /// Normally called once at registration time; exposed separately so that
    /// bridge plugins can set it after the initial [`register`] call if the PID
    /// was not known at startup.
    pub fn set_pid(&self, key: &ServiceKey, pid: u32) -> TransportResult<bool> {
        self.with_write_transaction(|| {
            let found = if let Some(mut entry) = self.services.get_mut(key) {
                entry.value_mut().pid = Some(pid);
                entry.value_mut().touch();
                true
            } else {
                false
            };
            Ok((found, found))
        })
    }

    /// Remove stale services (no heartbeat within timeout).
    ///
    /// The gateway sentinel entry ([`GATEWAY_SENTINEL_DCC_TYPE`]) is
    /// **never** evicted here — its staleness is meaningful only if the
    /// gateway process itself is dead, which [`Self::prune_dead_pids`] handles
    /// via PID liveness probe. See issue #230.
    pub fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize> {
        self.with_write_transaction(|| {
            let stale_keys: Vec<ServiceKey> = self
                .services
                .iter()
                .filter(|r| {
                    let e = r.value();
                    e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE && e.is_stale(timeout)
                })
                .map(|r| r.key().clone())
                .collect();

            let count = stale_keys.len();
            for key in &stale_keys {
                self.services.remove(key);
                tracing::info!(
                    dcc_type = %key.dcc_type,
                    instance_id = %key.instance_id,
                    "removed stale service"
                );
            }

            Ok((count, count > 0))
        })
    }

    /// Remove entries whose owning OS process is no longer running.
    ///
    /// Sentinel locks are checked before PID liveness: if another process can
    /// acquire the sentinel lock, the owner is gone even if the PID has already
    /// been reused. Rows from older versions without a sentinel fall back to the
    /// PID probe. Includes the gateway sentinel.
    ///
    /// Issue #719 follow-up: reload the on-disk registry before pruning
    /// so rows written by a separate (now-crashed) process become
    /// visible to this handle's in-memory cache. Without the reload a
    /// gateway reader would never see ghost rows left behind by a DCC
    /// that crashed after registering itself in a different
    /// `FileRegistry` instance.
    pub fn prune_dead_entries(&self) -> TransportResult<usize> {
        self.with_write_transaction(|| {
            let dead_keys: Vec<ServiceKey> = self
                .services
                .iter()
                .filter(|r| {
                    let entry = r.value();
                    if let Some(path) = entry.sentinel_path.as_deref() {
                        self.sentinel_is_dead(r.key(), path)
                    } else {
                        entry.pid.is_some_and(|p| !is_pid_alive(p))
                    }
                })
                .map(|r| r.key().clone())
                .collect();

            let count = dead_keys.len();
            for key in &dead_keys {
                let removed = self.services.remove(key).map(|(_, entry)| entry);
                self.sentinel_handles.remove(key);
                if let Some(entry) = removed
                    && let Some(path) = entry.sentinel_path
                {
                    let _ = fs::remove_file(path);
                }
                tracing::info!(
                    dcc_type = %key.dcc_type,
                    instance_id = %key.instance_id,
                    "removed ghost entry (owner sentinel/PID is dead)"
                );
            }

            Ok((count, count > 0))
        })
    }

    /// Backward-compatible name for [`Self::prune_dead_entries`].
    pub fn prune_dead_pids(&self) -> TransportResult<usize> {
        self.prune_dead_entries()
    }

    /// Read all live entries, evicting any whose owning OS process is dead.
    ///
    /// Combines the reload-if-stale + [`Self::prune_dead_pids`] + [`Self::list_all`]
    /// dance into a single call so external readers (gateway aggregator,
    /// gateway election (#523), `dcc-mcp-cli`, third-party tools) get
    /// auto-eviction at read time without re-implementing the pattern.
    ///
    /// Returns `(live_entries, evicted_count)`. `evicted_count` is `0` on the
    /// happy path, so callers can fall back to a simple `read_alive()?.0` if
    /// they only care about the rows. The atomic temp+rename rewrite of
    /// `services.json` happens automatically inside `prune_dead_pids` whenever
    /// at least one entry is evicted.
    ///
    /// Closes loonghao/dcc-mcp-maya#126.
    pub fn read_alive(&self) -> TransportResult<(Vec<ServiceEntry>, usize)> {
        let evicted = self.prune_dead_entries()?;
        Ok((self.list_all(), evicted))
    }

    /// [`Self::read_alive`] + a `tracing::warn!` whenever the eviction count
    /// crosses `warn_threshold`.
    ///
    /// Intended for the gateway's startup audit and any background reaper
    /// task that wants visibility on chronic ghost-row accumulation. The
    /// default threshold for callers that don't care about precision is
    /// `10` — small enough to surface real problems, large enough to absorb
    /// a single noisy crash on startup without spamming the log.
    pub fn read_alive_with_log(
        &self,
        warn_threshold: usize,
    ) -> TransportResult<(Vec<ServiceEntry>, usize)> {
        let (entries, evicted) = self.read_alive()?;
        if evicted >= warn_threshold {
            tracing::warn!(
                evicted,
                warn_threshold,
                kept = entries.len(),
                "FileRegistry::read_alive evicted ghost entries above warn threshold"
            );
        }
        Ok((entries, evicted))
    }

    /// Get the number of registered services.
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    // ── File I/O ──

    fn registry_file_path(&self) -> PathBuf {
        self.registry_dir.join(REGISTRY_FILE)
    }

    /// Flush the in-memory services to the JSON file atomically.
    ///
    /// Uses a temp-file + rename pattern so readers never see a partially-
    /// written file. Callers must already be inside [`Self::with_write_transaction`],
    /// which serializes cross-process read-modify-write cycles.
    fn flush_to_file_after_transaction(&self, baseline: &[ServiceEntry]) -> TransportResult<()> {
        // Mutation paths reload before applying their in-memory change. Do
        // not hot-reload again here: doing so can overwrite the just-mutated
        // row with the stale on-disk snapshot (#1088).
        let local_entries: Vec<ServiceEntry> = self.list_all_in_memory();
        let entries = self.reconcile_unlocked_writer_changes(baseline, local_entries)?;
        let content = serde_json::to_string_pretty(&entries).map_err(|e| {
            TransportError::Serialization(format!("failed to serialize registry: {}", e))
        })?;

        let path = self.registry_file_path();
        self.write_atomic(&path, content)?;
        self.replace_services(&entries);

        // Update cached mtime after write
        let _ = self.update_mtime();
        self.warn_if_post_write_clobbered(&entries);

        Ok(())
    }

    fn rollback_failed_transaction(
        &self,
        baseline_entries: &[ServiceEntry],
        baseline_owned_sentinels: &HashSet<ServiceKey>,
    ) {
        let rollback_entries = match self.read_entries_from_file() {
            Ok(entries) => entries,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    path = %self.registry_file_path().display(),
                    "FileRegistry could not reload durable snapshot after failed write; restoring pre-transaction snapshot"
                );
                baseline_entries.to_vec()
            }
        };

        self.replace_services(&rollback_entries);
        self.reconcile_sentinels_after_rollback(&rollback_entries, baseline_owned_sentinels);
        let _ = self.update_mtime();
    }

    fn reconcile_sentinels_after_rollback(
        &self,
        rollback_entries: &[ServiceEntry],
        baseline_owned_sentinels: &HashSet<ServiceKey>,
    ) {
        let rollback_keys: HashSet<ServiceKey> =
            rollback_entries.iter().map(ServiceEntry::key).collect();
        let desired_owned: HashSet<ServiceKey> = baseline_owned_sentinels
            .intersection(&rollback_keys)
            .cloned()
            .collect();

        let current_owned = self.owned_sentinel_keys();
        for key in current_owned.difference(&desired_owned) {
            if let Some((_, file)) = self.sentinel_handles.remove(key) {
                let _ = file.unlock();
            }
            let _ = fs::remove_file(self.sentinel_path_for(key));
        }

        for key in desired_owned {
            if self.sentinel_handles.contains_key(&key) {
                continue;
            }
            match self.create_sentinel(&key) {
                Ok((path, file)) => {
                    if let Some(mut entry) = self.services.get_mut(&key) {
                        entry.value_mut().sentinel_path = Some(path);
                    }
                    self.sentinel_handles.insert(key, file);
                }
                Err(err) => tracing::warn!(
                    error = %err,
                    dcc_type = %key.dcc_type,
                    instance_id = %key.instance_id,
                    "FileRegistry could not restore owned sentinel after failed write"
                ),
            }
        }
    }

    fn reconcile_unlocked_writer_changes(
        &self,
        baseline: &[ServiceEntry],
        local_entries: Vec<ServiceEntry>,
    ) -> TransportResult<Vec<ServiceEntry>> {
        let disk_entries = self.read_entries_from_file()?;
        let baseline_map = entries_to_map(baseline.iter().cloned());
        let disk_map = entries_to_map(disk_entries);
        if maps_equal(&baseline_map, &disk_map) {
            return Ok(local_entries);
        }

        tracing::warn!(
            path = %self.registry_file_path().display(),
            baseline_count = baseline_map.len(),
            disk_count = disk_map.len(),
            local_count = local_entries.len(),
            "legacy unlocked writer detected while FileRegistry held services.lock; merging registry snapshots"
        );

        let local_map = entries_to_map(local_entries);
        let mut merged = baseline_map.clone();
        for (key, disk_entry) in disk_map {
            merged.insert(key, disk_entry);
        }

        for (key, local_entry) in &local_map {
            if baseline_map.get(key) != Some(local_entry) {
                merged.insert(key.clone(), local_entry.clone());
            }
        }

        for key in baseline_map.keys() {
            if !local_map.contains_key(key) {
                merged.remove(key);
            }
        }

        Ok(merged.into_values().collect())
    }

    fn warn_if_post_write_clobbered(&self, intended_entries: &[ServiceEntry]) {
        match self.read_entries_from_file() {
            Ok(entries) => {
                if maps_equal(
                    &entries_to_map(entries.iter().cloned()),
                    &entries_to_map(intended_entries.iter().cloned()),
                ) {
                    return;
                }
                tracing::warn!(
                    path = %self.registry_file_path().display(),
                    intended_count = intended_entries.len(),
                    disk_count = entries.len(),
                    "legacy unlocked writer detected after FileRegistry write; services.json no longer matches committed snapshot"
                );
            }
            Err(err) => tracing::warn!(
                error = %err,
                path = %self.registry_file_path().display(),
                "FileRegistry could not verify services.json after write"
            ),
        }
    }

    fn replace_services(&self, entries: &[ServiceEntry]) {
        self.services.clear();
        for entry in entries {
            self.services.insert(entry.key(), entry.clone());
        }
    }

    /// Atomically write `content` to `path` using a temp file + rename.
    ///
    /// ## Design (issue #853)
    ///
    /// The original implementation (issue #560) used a per-write unique temp
    /// filename keyed on `(pid, tid, seq)` to allow multiple in-process
    /// writers to proceed concurrently without sharing the temp path.
    ///
    /// On Windows that pattern is AV/EDR-pathological: every new filename
    /// triggers the full minifilter altitude walk (content scan + cloud
    /// reputation lookup on enterprise workstations), causing multi-second
    /// stalls on the calling thread — observable as a frozen host UI during
    /// plugin load.
    ///
    /// The write transaction serialises in-process writers via `write_lock`
    /// so only one writer touches the temp file at a time, allowing the temp
    /// filename to be stable (`.tmp.<pid>.services.json`) across writes. AV/EDR
    /// minifilters fast-path the `CreateFile` on the second and subsequent
    /// writes because they see the same file being modified, not a new one.
    ///
    /// Cross-process read-modify-write cycles are serialized by
    /// `services.lock`; the bounded `fs::rename` retry loop still handles
    /// transient reader races on the target path.
    ///
    /// The temp file is explicitly `sync_data`'d before the rename so Windows
    /// cannot persist the directory swap while losing dirty file data during a
    /// power loss or hard kill (#1104).
    fn write_atomic(&self, path: &Path, content: String) -> TransportResult<()> {
        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        ensure_registry_dir(dir)?;
        let pid = std::process::id();
        // Stable per-process temp name — AV/EDR minifilters see the same
        // file being rewritten rather than a brand-new path each time.
        let temp_path = dir.join(format!(".tmp.{pid}.services.json"));

        let mut file = match OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                return Err(TransportError::RegistryFile(format!(
                    "failed to open temp file {}: {}",
                    temp_path.display(),
                    e
                )));
            }
        };

        if let Err(e) = file.write_all(content.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(TransportError::RegistryFile(format!(
                "failed to write temp file {}: {}",
                temp_path.display(),
                e
            )));
        }

        #[cfg(test)]
        if let Err(e) = run_before_temp_sync_hook(dir, &temp_path) {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(TransportError::RegistryFile(format!(
                "failed to sync temp file {}: {}",
                temp_path.display(),
                e
            )));
        }

        if let Err(e) = file.sync_data() {
            drop(file);
            let _ = fs::remove_file(&temp_path);
            return Err(TransportError::RegistryFile(format!(
                "failed to sync temp file {}: {}",
                temp_path.display(),
                e
            )));
        }
        drop(file);

        // Bounded retry around `fs::rename` — Windows can briefly return
        // `PermissionDenied` if another process has the target file open for
        // reading at the exact instant we try to swap it in.
        const MAX_ATTEMPTS: u32 = 8;
        const BACKOFF_MS: u64 = 10;
        let mut last_err: Option<std::io::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match fs::rename(&temp_path, path) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(BACKOFF_MS * (attempt as u64 + 1)));
                }
            }
        }
        // Best-effort temp cleanup so we don't leak `.tmp.*` files on
        // persistent failure.
        let _ = fs::remove_file(&temp_path);
        Err(TransportError::RegistryFile(format!(
            "failed to rename {} -> {} after {} attempts: {}",
            temp_path.display(),
            path.display(),
            MAX_ATTEMPTS,
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    /// Load services from the JSON file into memory.
    ///
    /// Readers do not take a lock — `write_atomic` swaps the target file in
    /// via `rename`, so the worst case for a racing reader is briefly seeing
    /// the previous snapshot. With a small bounded retry on
    /// `read_to_string` we also tolerate the narrow Windows window where a
    /// concurrent `rename` returns `PermissionDenied` to the reader.
    fn load_from_file(&self) -> TransportResult<()> {
        let entries = self.read_entries_from_file()?;
        if entries.is_empty() && !self.services.is_empty() {
            tracing::warn!(
                path = %self.registry_file_path().display(),
                "FileRegistry registry file missing or empty; clearing in-memory snapshot"
            );
        }
        self.replace_services(&entries);

        tracing::debug!(count = self.services.len(), "loaded services from file");
        Ok(())
    }

    fn read_entries_from_file(&self) -> TransportResult<Vec<ServiceEntry>> {
        let path = self.registry_file_path();
        let Some(content) = Self::read_with_retry(&path)? else {
            return Ok(Vec::new());
        };

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        if Self::looks_like_zero_padded_empty_registry(&content) {
            self.quarantine_zero_padded_registry_file(&path, content.len());
            return Ok(Vec::new());
        }

        serde_json::from_str(&content).map_err(|e| {
            TransportError::RegistryFile(format!("failed to parse {}: {}", path.display(), e))
        })
    }

    fn looks_like_zero_padded_empty_registry(content: &str) -> bool {
        content.as_bytes().contains(&0)
            && content
                .bytes()
                .all(|byte| byte == 0 || byte.is_ascii_whitespace())
    }

    fn quarantine_zero_padded_registry_file(&self, path: &Path, bytes: usize) {
        let ts = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let quarantined = path.with_extension(format!("json.corrupted-{ts}"));
        match fs::rename(path, &quarantined) {
            Ok(()) => tracing::warn!(
                path = %path.display(),
                quarantined = %quarantined.display(),
                bytes,
                "FileRegistry: registry file looked like a zero-padded NTFS artefact; quarantined and starting empty"
            ),
            Err(error) => tracing::warn!(
                path = %path.display(),
                quarantined = %quarantined.display(),
                bytes,
                error = %error,
                "FileRegistry: registry file looked like a zero-padded NTFS artefact; failed to quarantine but starting empty"
            ),
        }
    }

    /// Read the registry file with a short bounded retry to tolerate the
    /// Windows "file briefly held by another process" `PermissionDenied`
    /// race that can happen during a concurrent `rename`.
    fn read_with_retry(path: &PathBuf) -> TransportResult<Option<String>> {
        const MAX_ATTEMPTS: u32 = 5;
        const BACKOFF_MS: u64 = 5;
        let mut last_err: Option<std::io::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match fs::read_to_string(path) {
                Ok(s) => return Ok(Some(s)),
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(BACKOFF_MS * (attempt as u64 + 1)));
                }
            }
        }
        Err(TransportError::RegistryFile(format!(
            "failed to read {} after {} attempts: {}",
            path.display(),
            MAX_ATTEMPTS,
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        )))
    }
}

#[cfg(test)]
#[path = "file_registry_tests.rs"]
mod tests;
