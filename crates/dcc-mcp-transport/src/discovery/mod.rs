//! Service discovery — unified registry supporting file-based and future mDNS strategies.

pub mod file_registry;
pub mod types;

use std::time::Duration;

use crate::error::TransportResult;
use types::{ServiceEntry, ServiceKey, ServiceStatus};

/// Trait for service discovery strategies.
pub trait ServiceDiscovery: Send + Sync {
    /// Register a service.
    fn register(&self, entry: ServiceEntry) -> TransportResult<()>;

    /// Deregister a service by key.
    fn deregister(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>>;

    /// Get a service entry by key.
    fn get(&self, key: &ServiceKey) -> Option<ServiceEntry>;

    /// List all instances for a given DCC type.
    fn list_instances(&self, dcc_type: &str) -> Vec<ServiceEntry>;

    /// List all registered services.
    fn list_all(&self) -> Vec<ServiceEntry>;

    /// Update heartbeat for a service.
    fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool>;

    /// Update status for a service.
    fn update_status(&self, key: &ServiceKey, status: ServiceStatus) -> TransportResult<bool>;

    /// Remove stale services.
    fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize>;

    /// Get the number of registered services.
    fn len(&self) -> usize;

    /// Check if the registry is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Implement the trait for FileRegistry
impl ServiceDiscovery for file_registry::FileRegistry {
    fn register(&self, entry: ServiceEntry) -> TransportResult<()> {
        self.register(entry)
    }

    fn deregister(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>> {
        self.deregister(key)
    }

    fn get(&self, key: &ServiceKey) -> Option<ServiceEntry> {
        self.get(key)
    }

    fn list_instances(&self, dcc_type: &str) -> Vec<ServiceEntry> {
        self.list_instances(dcc_type)
    }

    fn list_all(&self) -> Vec<ServiceEntry> {
        self.list_all()
    }

    fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool> {
        self.heartbeat(key)
    }

    fn update_status(&self, key: &ServiceKey, status: ServiceStatus) -> TransportResult<bool> {
        self.update_status(key, status)
    }

    fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize> {
        self.cleanup_stale(timeout)
    }

    fn len(&self) -> usize {
        self.len()
    }
}

/// Unified service registry that delegates to a discovery strategy.
pub struct ServiceRegistry {
    strategy: Box<dyn ServiceDiscovery>,
}

impl ServiceRegistry {
    /// Create a new service registry with the given discovery strategy.
    pub fn new(strategy: Box<dyn ServiceDiscovery>) -> Self {
        Self { strategy }
    }

    /// Create a file-based service registry.
    pub fn file_based(registry_dir: impl Into<std::path::PathBuf>) -> TransportResult<Self> {
        let file_registry = file_registry::FileRegistry::new(registry_dir)?;
        Ok(Self::new(Box::new(file_registry)))
    }

    /// Register a service.
    pub fn register(&self, entry: ServiceEntry) -> TransportResult<()> {
        self.strategy.register(entry)
    }

    /// Deregister a service by key.
    pub fn deregister(&self, key: &ServiceKey) -> TransportResult<Option<ServiceEntry>> {
        self.strategy.deregister(key)
    }

    /// Get a service entry by key.
    pub fn get(&self, key: &ServiceKey) -> Option<ServiceEntry> {
        self.strategy.get(key)
    }

    /// List all instances for a given DCC type.
    pub fn list_instances(&self, dcc_type: &str) -> Vec<ServiceEntry> {
        self.strategy.list_instances(dcc_type)
    }

    /// List all registered services.
    pub fn list_all(&self) -> Vec<ServiceEntry> {
        self.strategy.list_all()
    }

    /// Update heartbeat for a service.
    pub fn heartbeat(&self, key: &ServiceKey) -> TransportResult<bool> {
        self.strategy.heartbeat(key)
    }

    /// Update status for a service.
    pub fn update_status(&self, key: &ServiceKey, status: ServiceStatus) -> TransportResult<bool> {
        self.strategy.update_status(key, status)
    }

    /// Remove stale services.
    pub fn cleanup_stale(&self, timeout: Duration) -> TransportResult<usize> {
        self.strategy.cleanup_stale(timeout)
    }

    /// Get the number of registered services.
    pub fn len(&self) -> usize {
        self.strategy.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.strategy.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_registry_file_based() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ServiceRegistry::file_based(dir.path()).unwrap();

        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        registry.register(entry).unwrap();

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.list_instances("maya").len(), 1);
    }

    #[test]
    fn test_service_registry_multiple_instances() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ServiceRegistry::file_based(dir.path()).unwrap();

        registry
            .register(ServiceEntry::new("maya", "127.0.0.1", 18812))
            .unwrap();
        registry
            .register(ServiceEntry::new("maya", "127.0.0.1", 18813))
            .unwrap();
        registry
            .register(ServiceEntry::new("blender", "127.0.0.1", 9090))
            .unwrap();

        assert_eq!(registry.len(), 3);
        assert_eq!(registry.list_instances("maya").len(), 2);
        assert_eq!(registry.list_instances("blender").len(), 1);
        assert_eq!(registry.list_instances("houdini").len(), 0);
    }
}
