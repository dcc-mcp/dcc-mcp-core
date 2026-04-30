//! File-based service registry.
//!
//! Stores service entries as JSON in a registry directory.
//! Uses `(dcc_type, instance_id)` as key to support multiple instances per DCC type.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use dashmap::DashMap;
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
    /// Last-seen modification time of services.json.
    /// Used to detect external writes (hot-reload).
    last_mtime: Mutex<Option<SystemTime>>,
}

impl FileRegistry {
    /// Create a new file registry at the given directory.
    pub fn new(registry_dir: impl Into<PathBuf>) -> TransportResult<Self> {
        let registry_dir = registry_dir.into();
        fs::create_dir_all(&registry_dir).map_err(|e| {
            TransportError::RegistryFile(format!(
                "failed to create registry dir {}: {}",
                registry_dir.display(),
                e
            ))
        })?;

        let registry = Self {
            services: DashMap::new(),
            registry_dir,
            last_mtime: Mutex::new(None),
        };

        // Load existing entries
        registry.load_from_file()?;
        registry.update_mtime()?;

        Ok(registry)
    }

    /// Reload from file if another process has written to it since our last read (hot-reload).
    ///
    /// This is O(1) on the happy path: single `stat` syscall + mutex check.
    /// Only does actual file I/O when another process has modified services.json.
    fn reload_if_stale(&self) -> TransportResult<()> {
        let path = self.registry_file_path();

        // Quick stat to get current mtime
        let Ok(meta) = fs::metadata(&path) else {
            // File doesn't exist yet, nothing to reload
            return Ok(());
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

    /// Register a service.
    pub fn register(&self, entry: ServiceEntry) -> TransportResult<()> {
        let key = entry.key();
        tracing::info!(
            dcc_type = %entry.dcc_type,
            instance_id = %entry.instance_id,
            host = %entry.host,
            port = entry.port,
            "registering service"
        );
        self.services.insert(key, entry);
        self.flush_to_file()
    }

    /// Deregister a service by key.
    pub fn deregister(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>> {
        let removed = self.services.remove(key).map(|(_, entry)| entry);
        if removed.is_some() {
            tracing::info!(
                dcc_type = %key.dcc_type,
                instance_id = %key.instance_id,
                "deregistered service"
            );
            self.flush_to_file()?;
        }
        Ok(removed)
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
        self.services.iter().map(|r| r.value().clone()).collect()
    }

    /// Update heartbeat for a service.
    pub fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool> {
        let found = if let Some(mut entry) = self.services.get_mut(key) {
            entry.value_mut().touch();
            true
        } else {
            false
        };
        // flush_to_file calls list_all which iterates the DashMap;
        // the write guard from get_mut must be dropped first to avoid deadlock.
        if found {
            self.flush_to_file()?;
        }
        Ok(found)
    }

    /// Update status for a service.
    pub fn update_status(&self, key: &ServiceKey, status: ServiceStatus) -> TransportResult<bool> {
        let found = if let Some(mut entry) = self.services.get_mut(key) {
            entry.value_mut().status = status;
            true
        } else {
            false
        };
        if found {
            self.flush_to_file()?;
        }
        Ok(found)
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

        if changed {
            self.flush_to_file()?;
        }
        Ok(selected)
    }

    /// Release a pool lease. When `owner` is supplied it must match the holder.
    pub fn release_lease(
        &self,
        key: &ServiceKey,
        owner: Option<&str>,
    ) -> TransportResult<Option<ServiceEntry>> {
        let released = if let Some(mut entry) = self.services.get_mut(key) {
            let owner_matches =
                owner.is_none_or(|expected| entry.value().lease_owner.as_deref() == Some(expected));
            if owner_matches && entry.value().lease_owner.is_some() {
                entry.value_mut().clear_lease();
                Some(entry.value().clone())
            } else {
                None
            }
        } else {
            None
        };
        if released.is_some() {
            self.flush_to_file()?;
        }
        Ok(released)
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
        if found {
            self.flush_to_file()?;
        }
        Ok(found)
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

        if found {
            self.flush_to_file()?;
        }
        Ok(found)
    }

    /// Set the OS process ID for a registered service.
    ///
    /// Normally called once at registration time; exposed separately so that
    /// bridge plugins can set it after the initial [`register`] call if the PID
    /// was not known at startup.
    pub fn set_pid(&self, key: &ServiceKey, pid: u32) -> TransportResult<bool> {
        let found = if let Some(mut entry) = self.services.get_mut(key) {
            entry.value_mut().pid = Some(pid);
            entry.value_mut().touch();
            true
        } else {
            false
        };
        if found {
            self.flush_to_file()?;
        }
        Ok(found)
    }

    /// Remove stale services (no heartbeat within timeout).
    ///
    /// The gateway sentinel entry ([`GATEWAY_SENTINEL_DCC_TYPE`]) is
    /// **never** evicted here — its staleness is meaningful only if the
    /// gateway process itself is dead, which [`Self::prune_dead_pids`] handles
    /// via PID liveness probe. See issue #230.
    pub fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize> {
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

        if count > 0 {
            self.flush_to_file()?;
        }
        Ok(count)
    }

    /// Remove entries whose owning OS process is no longer running.
    ///
    /// Complements [`Self::cleanup_stale`]: a plugin that crashes during
    /// `initializePlugin` (after `bind_and_register` wrote its row but before
    /// the heartbeat task started) would otherwise leak a ghost entry for up
    /// to `stale_timeout` seconds. This check runs a PID liveness probe and
    /// removes entries with dead PIDs immediately — including the gateway
    /// sentinel, since a dead gateway process must not keep the sentinel alive.
    ///
    /// Entries without a `pid` set are left untouched (fail-open — we cannot
    /// probe what we cannot identify). See issue #227.
    pub fn prune_dead_pids(&self) -> TransportResult<usize> {
        let dead_keys: Vec<ServiceKey> = self
            .services
            .iter()
            .filter(|r| r.value().pid.is_some_and(|p| !is_pid_alive(p)))
            .map(|r| r.key().clone())
            .collect();

        let count = dead_keys.len();
        for key in &dead_keys {
            self.services.remove(key);
            tracing::info!(
                dcc_type = %key.dcc_type,
                instance_id = %key.instance_id,
                "removed ghost entry (owning process is dead)"
            );
        }

        if count > 0 {
            self.flush_to_file()?;
        }
        Ok(count)
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
        let evicted = self.prune_dead_pids()?;
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
    /// written file. On Windows an OS-level advisory lock is taken for the
    /// duration of the write to prevent competing processes from clobbering
    /// each other (issue #554).
    fn flush_to_file(&self) -> TransportResult<()> {
        let entries: Vec<ServiceEntry> = self.list_all();
        let content = serde_json::to_string_pretty(&entries).map_err(|e| {
            TransportError::Serialization(format!("failed to serialize registry: {}", e))
        })?;

        let path = self.registry_file_path();
        Self::write_atomic(&path, content)?;

        // Update cached mtime after write
        let _ = self.update_mtime();

        Ok(())
    }

    /// Atomically write `content` to `path` using a temp file + rename.
    ///
    /// The original implementation of issue #554 used an exclusive
    /// `share_mode(0)` advisory lock file plus a temp filename keyed only on
    /// the process id. Both choices caused regressions: concurrent in-process
    /// writers (sentinel heartbeat + multiple backend heartbeats) raced on
    /// the same temp path, and the exclusive lock file made one of two
    /// concurrent writers fail outright with `PermissionDenied` so the loser's
    /// entry never reached `services.json`. The downstream symptom was the
    /// gateway facade only seeing one of two backends in
    /// `test_gateway_facade_aggregation` (issue #560 follow-up).
    ///
    /// The lock has been removed in favour of two cheaper, cross-platform
    /// invariants that still satisfy the original "no half-written file"
    /// requirement of #554:
    ///
    /// 1. The temp filename is unique per write — process id, thread id, and
    ///    a process-wide monotonic counter — so two writers in the same
    ///    process never share a temp path.
    /// 2. The temp file is renamed onto the target with a small bounded retry
    ///    loop, because Windows can return `PermissionDenied` /
    ///    `AccessDenied` if a concurrent reader has the target file briefly
    ///    open. POSIX `rename` is already atomic and never needs the retry,
    ///    but the loop is harmless on other platforms.
    fn write_atomic(path: &PathBuf, content: String) -> TransportResult<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let pid = std::process::id();
        let tid = format!("{:?}", std::thread::current().id());
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = dir.join(format!(".tmp.{pid}.{tid}.{seq}.services.json"));

        fs::write(&temp_path, content).map_err(|e| {
            TransportError::RegistryFile(format!(
                "failed to write temp file {}: {}",
                temp_path.display(),
                e
            ))
        })?;

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
        let path = self.registry_file_path();
        if !path.exists() {
            return Ok(());
        }

        let content = Self::read_with_retry(&path)?;

        if content.trim().is_empty() {
            return Ok(());
        }

        let entries: Vec<ServiceEntry> = serde_json::from_str(&content).map_err(|e| {
            TransportError::RegistryFile(format!("failed to parse {}: {}", path.display(), e))
        })?;

        for entry in entries {
            let key = entry.key();
            self.services.insert(key, entry);
        }

        tracing::debug!(count = self.services.len(), "loaded services from file");
        Ok(())
    }

    /// Read the registry file with a short bounded retry to tolerate the
    /// Windows "file briefly held by another process" `PermissionDenied`
    /// race that can happen during a concurrent `rename`.
    fn read_with_retry(path: &PathBuf) -> TransportResult<String> {
        const MAX_ATTEMPTS: u32 = 5;
        const BACKOFF_MS: u64 = 5;
        let mut last_err: Option<std::io::Error> = None;
        for attempt in 0..MAX_ATTEMPTS {
            match fs::read_to_string(path) {
                Ok(s) => return Ok(s),
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
