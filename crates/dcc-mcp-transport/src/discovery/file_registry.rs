//! File-based service registry.
//!
//! Stores service entries as JSON in a registry directory.
//! Uses `(dcc_type, instance_id)` as key to support multiple instances per DCC type.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use dashmap::DashMap;
use tracing;

use super::types::{ServiceEntry, ServiceKey, ServiceStatus};
use crate::error::{TransportError, TransportResult};

/// File name for the registry JSON.
const REGISTRY_FILE: &str = "services.json";

/// File-based service registry with instance-level keying.
///
/// Key improvement over the Python implementation: uses `(dcc_type, instance_id)` as key
/// instead of `dcc_type` alone, enabling multiple instances of the same DCC type.
pub struct FileRegistry {
    /// In-memory cache of services.
    services: DashMap<ServiceKey, ServiceEntry>,
    /// Directory where registry file is stored.
    registry_dir: PathBuf,
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
        };

        // Load existing entries
        registry.load_from_file()?;

        Ok(registry)
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
        self.services
            .iter()
            .filter(|r| r.value().dcc_type == dcc_type)
            .map(|r| r.value().clone())
            .collect()
    }

    /// List all registered services.
    pub fn list_all(&self) -> Vec<ServiceEntry> {
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

    /// Remove stale services (no heartbeat within timeout).
    pub fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize> {
        let stale_keys: Vec<ServiceKey> = self
            .services
            .iter()
            .filter(|r| r.value().is_stale(timeout))
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
}
