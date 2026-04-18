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

        // Compare with cached mtime
        let mut cached = self.last_mtime.lock().unwrap();
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
            tracing::debug!("FileRegistry hot-reloaded from disk");
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

        let mut cached = self.last_mtime.lock().unwrap();
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

    /// Load services from the JSON file into memory.
    fn load_from_file(&self) -> TransportResult<()> {
        let path = self.registry_file_path();
        if !path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            TransportError::RegistryFile(format!("failed to read {}: {}", path.display(), e))
        })?;

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

    /// Flush the in-memory services to the JSON file.
    fn flush_to_file(&self) -> TransportResult<()> {
        let entries: Vec<ServiceEntry> = self.list_all();
        let content = serde_json::to_string_pretty(&entries).map_err(|e| {
            TransportError::Serialization(format!("failed to serialize registry: {}", e))
        })?;

        let path = self.registry_file_path();
        fs::write(&path, content).map_err(|e| {
            TransportError::RegistryFile(format!("failed to write {}: {}", path.display(), e))
        })?;

        // Update cached mtime after write
        let _ = self.update_mtime();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_file_registry_register_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let entry1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let entry2 = ServiceEntry::new("maya", "127.0.0.1", 18813);
        let entry3 = ServiceEntry::new("blender", "127.0.0.1", 9090);

        registry.register(entry1).unwrap();
        registry.register(entry2).unwrap();
        registry.register(entry3).unwrap();

        assert_eq!(registry.len(), 3);

        let maya_instances = registry.list_instances("maya");
        assert_eq!(maya_instances.len(), 2);

        let blender_instances = registry.list_instances("blender");
        assert_eq!(blender_instances.len(), 1);
    }

    #[test]
    fn test_file_registry_deregister() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        registry.register(entry).unwrap();
        assert_eq!(registry.len(), 1);

        let removed = registry.deregister(&key).unwrap();
        assert!(removed.is_some());
        assert!(registry.is_empty());
    }

    #[test]
    fn test_file_registry_persistence() {
        let dir = tempfile::tempdir().unwrap();

        let instance_id;
        {
            let registry = FileRegistry::new(dir.path()).unwrap();
            let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
            instance_id = entry.instance_id;
            registry.register(entry).unwrap();
        }

        // Reload from file
        let registry = FileRegistry::new(dir.path()).unwrap();
        assert_eq!(registry.len(), 1);
        let entries = registry.list_instances("maya");
        assert_eq!(entries[0].instance_id, instance_id);
    }

    #[test]
    fn test_file_registry_heartbeat() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        registry.register(entry).unwrap();

        assert!(registry.heartbeat(&key).unwrap());

        // Non-existent key
        let fake_key = ServiceKey {
            dcc_type: "nuke".to_string(),
            instance_id: Uuid::new_v4(),
        };
        assert!(!registry.heartbeat(&fake_key).unwrap());
    }

    #[test]
    fn test_file_registry_cleanup_stale() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        // Force old heartbeat
        entry.last_heartbeat = std::time::SystemTime::now() - std::time::Duration::from_secs(100);
        registry.register(entry).unwrap();

        let cleaned = registry
            .cleanup_stale(std::time::Duration::from_secs(10))
            .unwrap();
        assert_eq!(cleaned, 1);
        assert!(registry.is_empty());
    }

    // Regression test for issue #230: cleanup_stale must never evict the gateway sentinel,
    // even when its heartbeat appears stale, because that record is the source of truth
    // for "who is the gateway" and a live but non-heartbeating sentinel is valid.
    #[test]
    fn test_file_registry_cleanup_stale_preserves_gateway_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
        sentinel.last_heartbeat =
            std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        registry.register(sentinel).unwrap();

        let mut stale_instance = ServiceEntry::new("maya", "127.0.0.1", 18812);
        stale_instance.last_heartbeat =
            std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        registry.register(stale_instance).unwrap();

        let cleaned = registry
            .cleanup_stale(std::time::Duration::from_secs(30))
            .unwrap();
        // Only the maya row gets evicted; sentinel survives.
        assert_eq!(cleaned, 1);
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.list_instances(GATEWAY_SENTINEL_DCC_TYPE).len(),
            1,
            "gateway sentinel must not be evicted by cleanup_stale"
        );
    }

    // Regression test for issue #227: ghost rows from a crashed DCC process must be reaped.
    #[test]
    fn test_file_registry_prune_dead_pids() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        // Live entry (auto-populated pid == our own process id).
        let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let live_key = live.key();
        registry.register(live).unwrap();

        // Ghost entry with a clearly-dead PID.
        // u32::MAX is a reserved sentinel on every OS we target.
        let ghost = ServiceEntry::new("maya", "127.0.0.1", 18813).with_pid(u32::MAX);
        let ghost_key = ghost.key();
        registry.register(ghost).unwrap();

        let pruned = registry.prune_dead_pids().unwrap();
        assert_eq!(pruned, 1, "exactly one ghost entry should be pruned");
        assert!(registry.get(&live_key).is_some(), "live entry must remain");
        assert!(
            registry.get(&ghost_key).is_none(),
            "ghost entry must be removed"
        );
    }

    #[test]
    fn test_file_registry_prune_dead_pids_skips_unknown_pid() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        // Entry with pid explicitly cleared → liveness unknown, must not be pruned.
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        entry.pid = None;
        registry.register(entry).unwrap();

        let pruned = registry.prune_dead_pids().unwrap();
        assert_eq!(pruned, 0);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_file_registry_multiple_instances_same_dcc() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        // Register multiple Maya instances — this is the critical fix
        for port in 18812..18815 {
            let entry = ServiceEntry::new("maya", "127.0.0.1", port);
            registry.register(entry).unwrap();
        }

        assert_eq!(registry.len(), 3);
        let maya_instances = registry.list_instances("maya");
        assert_eq!(maya_instances.len(), 3);

        // Each should have a unique port
        let ports: Vec<u16> = maya_instances.iter().map(|e| e.port).collect();
        assert!(ports.contains(&18812));
        assert!(ports.contains(&18813));
        assert!(ports.contains(&18814));
    }

    #[test]
    fn test_file_registry_hot_reload() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        // Register entry in process A
        let entry_a = ServiceEntry::new("maya", "127.0.0.1", 18812);
        registry.register(entry_a).unwrap();
        assert_eq!(registry.len(), 1);

        // Small sleep to ensure filesystem mtime granularity is observed
        // (on some systems, mtime has 1-second or coarser precision)
        std::thread::sleep(Duration::from_millis(100));

        // Simulate external write by another process: create a new registry instance
        // that writes a new entry to the same file
        {
            let registry_b = FileRegistry::new(dir.path()).unwrap();
            let entry_b = ServiceEntry::new("blender", "127.0.0.1", 8888);
            registry_b.register(entry_b).unwrap();
        }

        // Process A should detect the new entry via hot-reload
        let all = registry.list_all();
        assert_eq!(all.len(), 2, "hot-reload should discover external entry");

        let maya = registry.list_instances("maya");
        assert_eq!(maya.len(), 1);

        let blender = registry.list_instances("blender");
        assert_eq!(blender.len(), 1);
    }

    #[test]
    fn test_file_registry_hot_reload_is_lazy() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        // Register initial entry
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        registry.register(entry).unwrap();

        // Multiple list_all() calls on unchanged file should all hit fast path
        for _ in 0..5 {
            let _ = registry.list_all();
        }

        // All calls should succeed without error
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_file_registry_update_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let registry = FileRegistry::new(dir.path()).unwrap();

        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        registry.register(entry).unwrap();

        // Initially no scene
        let e = registry.get(&key).unwrap();
        assert!(e.scene.is_none());
        assert!(e.version.is_none());

        // Update scene
        assert!(
            registry
                .update_metadata(&key, Some("my_scene.ma"), None)
                .unwrap()
        );
        let e = registry.get(&key).unwrap();
        assert_eq!(e.scene.as_deref(), Some("my_scene.ma"));
        assert!(e.version.is_none());

        // Update version
        assert!(registry.update_metadata(&key, None, Some("2025")).unwrap());
        let e = registry.get(&key).unwrap();
        assert_eq!(e.scene.as_deref(), Some("my_scene.ma"));
        assert_eq!(e.version.as_deref(), Some("2025"));

        // Update both
        assert!(
            registry
                .update_metadata(&key, Some("other.ma"), Some("2026"))
                .unwrap()
        );
        let e = registry.get(&key).unwrap();
        assert_eq!(e.scene.as_deref(), Some("other.ma"));
        assert_eq!(e.version.as_deref(), Some("2026"));

        // Clear scene with empty string
        assert!(registry.update_metadata(&key, Some(""), None).unwrap());
        let e = registry.get(&key).unwrap();
        assert!(e.scene.is_none());

        // Non-existent key
        let fake_key = ServiceKey {
            dcc_type: "nuke".to_string(),
            instance_id: Uuid::new_v4(),
        };
        assert!(
            !registry
                .update_metadata(&fake_key, Some("x"), None)
                .unwrap()
        );
    }
}
