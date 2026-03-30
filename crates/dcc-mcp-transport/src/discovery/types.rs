//! Service discovery types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

/// Status of a discovered DCC service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// Service is available and accepting connections.
    Available,
    /// Service is busy (processing a request).
    Busy,
    /// Service is unreachable (health check failed).
    Unreachable,
    /// Service is shutting down.
    ShuttingDown,
}

impl Default for ServiceStatus {
    fn default() -> Self {
        Self::Available
    }
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => write!(f, "available"),
            Self::Busy => write!(f, "busy"),
            Self::Unreachable => write!(f, "unreachable"),
            Self::ShuttingDown => write!(f, "shutting_down"),
        }
    }
}

/// A discovered DCC service instance.
///
/// Keyed by `(dcc_type, instance_id)` — supports multiple instances of the same DCC type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    /// DCC application type (e.g. "maya", "houdini", "blender").
    pub dcc_type: String,
    /// Unique ID for this running instance.
    pub instance_id: Uuid,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// DCC application version (e.g. "2024.2").
    pub version: Option<String>,
    /// Currently open scene/file.
    pub scene: Option<String>,
    /// Arbitrary metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// When this service was registered.
    pub registered_at: SystemTime,
    /// Last heartbeat timestamp.
    pub last_heartbeat: SystemTime,
    /// Current status.
    #[serde(default)]
    pub status: ServiceStatus,
}

impl ServiceEntry {
    /// Create a new service entry with sensible defaults.
    pub fn new(dcc_type: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        let now = SystemTime::now();
        Self {
            dcc_type: dcc_type.into(),
            instance_id: Uuid::new_v4(),
            host: host.into(),
            port,
            version: None,
            scene: None,
            metadata: HashMap::new(),
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Available,
        }
    }

    /// Composite key for registry lookups.
    pub fn key(&self) -> ServiceKey {
        ServiceKey {
            dcc_type: self.dcc_type.clone(),
            instance_id: self.instance_id,
        }
    }

    /// Update the heartbeat timestamp.
    pub fn touch(&mut self) {
        self.last_heartbeat = SystemTime::now();
    }

    /// Check if the service is considered stale (no heartbeat within given duration).
    pub fn is_stale(&self, timeout: std::time::Duration) -> bool {
        self.last_heartbeat
            .elapsed()
            .map(|elapsed| elapsed > timeout)
            .unwrap_or(true)
    }
}

/// Composite key for service lookups: `(dcc_type, instance_id)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceKey {
    pub dcc_type: String,
    pub instance_id: Uuid,
}

impl std::fmt::Display for ServiceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.dcc_type, self.instance_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_service_entry_new() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        assert_eq!(entry.dcc_type, "maya");
        assert_eq!(entry.host, "127.0.0.1");
        assert_eq!(entry.port, 18812);
        assert_eq!(entry.status, ServiceStatus::Available);
        assert!(entry.version.is_none());
        assert!(entry.scene.is_none());
    }

    #[test]
    fn test_service_entry_key() {
        let entry = ServiceEntry::new("houdini", "localhost", 9090);
        let key = entry.key();
        assert_eq!(key.dcc_type, "houdini");
        assert_eq!(key.instance_id, entry.instance_id);
    }

    #[test]
    fn test_service_entry_staleness() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        // Should not be stale immediately
        assert!(!entry.is_stale(Duration::from_secs(1)));

        // Force an old heartbeat
        entry.last_heartbeat = SystemTime::now() - Duration::from_secs(10);
        assert!(entry.is_stale(Duration::from_secs(5)));
    }

    #[test]
    fn test_service_status_display() {
        assert_eq!(format!("{}", ServiceStatus::Available), "available");
        assert_eq!(format!("{}", ServiceStatus::ShuttingDown), "shutting_down");
    }

    #[test]
    fn test_service_entry_serialization() {
        let entry = ServiceEntry::new("blender", "127.0.0.1", 8080);
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: ServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.dcc_type, "blender");
        assert_eq!(deserialized.instance_id, entry.instance_id);
    }
}
