//! Service discovery types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

use crate::ipc::TransportAddress;

/// `dcc_type` used for the gateway sentinel entry in the `FileRegistry`.
///
/// The sentinel entry carries the current gateway's version so that newly
/// started instances can compare themselves against the running gateway and
/// decide whether to enter challenger mode.
///
/// Defined at the transport layer so lower layers (e.g. `FileRegistry::cleanup_stale`)
/// can special-case it without depending on `dcc-mcp-http`.
pub const GATEWAY_SENTINEL_DCC_TYPE: &str = "__gateway__";

/// Status of a discovered DCC service instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    /// Service is available and accepting connections.
    #[default]
    Available,
    /// Service is busy (processing a request).
    Busy,
    /// Service is unreachable (health check failed).
    Unreachable,
    /// Service is shutting down.
    ShuttingDown,
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
///
/// ## Transport address
///
/// The `transport_address` field specifies the preferred communication channel:
/// - **TCP** (default): `host:port` — works cross-machine
/// - **Named Pipe** (Windows): sub-millisecond latency for same-machine
/// - **Unix Socket** (macOS/Linux): sub-0.1ms latency for same-machine
///
/// The legacy `host` and `port` fields are always populated for backward compatibility.
/// When `transport_address` is set, it takes precedence over `host:port`.
///
/// ## Multi-document support
///
/// Applications like Photoshop or After Effects can have several documents open at once.
/// `scene` always holds the **currently active** document; `documents` holds the full list.
/// For single-document DCCs (Maya, Blender, Houdini), `documents` is either empty or
/// contains just the same path as `scene`.
///
/// ## Disambiguation
///
/// When multiple instances of the same DCC type are running (e.g. two Maya sessions
/// working on different scenes), agents use `display_name` and `pid` to tell them apart:
///
/// ```text
/// maya @ 127.0.0.1:18812  pid=1234  scene=character.ma  display_name="Maya-Rig"
/// maya @ 127.0.0.1:18813  pid=5678  scene=character.ma  display_name="Maya-Anim"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    /// DCC application type (e.g. "maya", "houdini", "blender").
    pub dcc_type: String,
    /// Unique ID for this running instance.
    pub instance_id: Uuid,
    /// Host address (kept for backward compatibility).
    pub host: String,
    /// Port number (kept for backward compatibility).
    pub port: u16,
    /// Transport address — preferred communication channel.
    ///
    /// When `None`, falls back to `host:port` TCP connection.
    /// When `Some`, this address takes precedence over `host:port`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_address: Option<TransportAddress>,
    /// DCC application version (e.g. "2024.2").
    pub version: Option<String>,
    /// Currently active scene / document.
    ///
    /// For single-document DCCs (Maya, Blender) this is the open file path.
    /// For multi-document apps (Photoshop) this is the **focused** document.
    pub scene: Option<String>,
    /// All documents currently open in this instance.
    ///
    /// Empty for DCCs that only support one document at a time.
    /// For multi-document apps each element is a file path.
    /// The active document is also reflected in `scene`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,
    /// OS process ID of the DCC process.
    ///
    /// Used to disambiguate two instances of the same DCC type that have the
    /// same scene open (e.g. two Maya sessions reviewing the same asset).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Human-readable label for this instance.
    ///
    /// Set by the DCC plugin at registration time (e.g. `"Maya-Rigging"`,
    /// `"PS-Marketing"`).  Displayed by the agent when asking the user to
    /// choose between multiple instances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Arbitrary metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Arbitrary DCC-specific extras as JSON-typed values.
    ///
    /// Unlike [`metadata`] which is restricted to strings, `extras` allows
    /// nested objects / arrays / numbers / booleans.  Use for WebView / bridge
    /// specific fields such as `cdp_port`, `url`, `window_title`, `host_dcc`.
    ///
    /// Round-trips losslessly through `services.json` (JSON value preserved).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extras: HashMap<String, serde_json::Value>,
    /// When this service was registered.
    pub registered_at: SystemTime,
    /// Last heartbeat timestamp.
    pub last_heartbeat: SystemTime,
    /// Current status.
    #[serde(default)]
    pub status: ServiceStatus,
}

impl ServiceEntry {
    /// Create a new service entry with TCP transport (sensible defaults).
    ///
    /// `pid` is auto-populated with [`std::process::id()`] so the registry can
    /// reap ghost entries when the owning process crashes (see
    /// [`FileRegistry::prune_dead_pids`](super::file_registry::FileRegistry::prune_dead_pids)).
    /// Override via [`ServiceEntry::with_pid`] when registering on behalf of
    /// another process (bridge scenarios).
    pub fn new(dcc_type: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        let now = SystemTime::now();
        Self {
            dcc_type: dcc_type.into(),
            instance_id: Uuid::new_v4(),
            host: host.into(),
            port,
            transport_address: None,
            version: None,
            scene: None,
            documents: Vec::new(),
            pid: Some(std::process::id()),
            display_name: None,
            metadata: HashMap::new(),
            extras: HashMap::new(),
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Available,
        }
    }

    /// Create a new service entry with a specific transport address.
    ///
    /// `pid` is auto-populated with [`std::process::id()`]; see [`ServiceEntry::new`].
    pub fn with_address(dcc_type: impl Into<String>, address: TransportAddress) -> Self {
        let (host, port) = match &address {
            TransportAddress::Tcp { host, port } => (host.clone(), *port),
            // For IPC transports, use placeholder host/port for backward compat
            TransportAddress::NamedPipe { .. } => ("127.0.0.1".to_string(), 0),
            TransportAddress::UnixSocket { .. } => ("127.0.0.1".to_string(), 0),
        };
        let now = SystemTime::now();
        Self {
            dcc_type: dcc_type.into(),
            instance_id: Uuid::new_v4(),
            host,
            port,
            transport_address: Some(address),
            version: None,
            scene: None,
            documents: Vec::new(),
            pid: Some(std::process::id()),
            display_name: None,
            metadata: HashMap::new(),
            extras: HashMap::new(),
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Available,
        }
    }

    /// Override the owning process PID (useful when registering on behalf of a bridge).
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Get the effective transport address.
    ///
    /// Returns the `transport_address` if set, otherwise constructs a TCP address
    /// from `host` and `port`.
    pub fn effective_address(&self) -> TransportAddress {
        self.transport_address
            .clone()
            .unwrap_or_else(|| TransportAddress::tcp(&self.host, self.port))
    }

    /// Check if this service uses an IPC transport (Named Pipe or Unix Socket).
    pub fn is_ipc(&self) -> bool {
        self.transport_address
            .as_ref()
            .is_some_and(|addr| !addr.is_tcp())
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
        assert!(entry.transport_address.is_none());
        assert!(entry.extras.is_empty());
        // pid is auto-populated with the current process id (ghost-entry prevention).
        assert_eq!(entry.pid, Some(std::process::id()));
    }

    #[test]
    fn test_service_entry_with_pid_override() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812).with_pid(42);
        assert_eq!(entry.pid, Some(42));
    }

    #[test]
    fn test_service_entry_extras_roundtrip() {
        let mut entry = ServiceEntry::new("webview", "127.0.0.1", 3000);
        entry
            .extras
            .insert("cdp_port".into(), serde_json::json!(9222));
        entry
            .extras
            .insert("url".into(), serde_json::json!("http://localhost:3000"));
        entry.extras.insert(
            "capabilities".into(),
            serde_json::json!({"scene": false, "timeline": true}),
        );

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.extras, entry.extras);
        assert_eq!(parsed.extras["cdp_port"], serde_json::json!(9222));
        assert_eq!(
            parsed.extras["capabilities"]["timeline"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn test_service_entry_empty_extras_omitted_from_json() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let json = serde_json::to_string(&entry).unwrap();
        // `skip_serializing_if = "HashMap::is_empty"` keeps services.json small
        // when no extras were set — preserves backward-compatible file format.
        assert!(
            !json.contains("\"extras\""),
            "empty extras should be omitted, got: {}",
            json
        );
    }

    #[test]
    fn test_service_entry_with_address_tcp() {
        let addr = TransportAddress::tcp("10.0.0.1", 9090);
        let entry = ServiceEntry::with_address("blender", addr.clone());
        assert_eq!(entry.dcc_type, "blender");
        assert_eq!(entry.host, "10.0.0.1");
        assert_eq!(entry.port, 9090);
        assert_eq!(entry.transport_address, Some(addr));
    }

    #[test]
    fn test_service_entry_with_address_named_pipe() {
        let addr = TransportAddress::named_pipe("dcc-mcp-maya-12345");
        let entry = ServiceEntry::with_address("maya", addr.clone());
        assert_eq!(entry.host, "127.0.0.1");
        assert_eq!(entry.port, 0);
        assert_eq!(entry.transport_address, Some(addr));
        assert!(entry.is_ipc());
    }

    #[test]
    fn test_service_entry_with_address_unix_socket() {
        let addr = TransportAddress::unix_socket("/tmp/dcc-mcp-maya.sock");
        let entry = ServiceEntry::with_address("maya", addr.clone());
        assert_eq!(entry.host, "127.0.0.1");
        assert_eq!(entry.port, 0);
        assert!(entry.is_ipc());
    }

    #[test]
    fn test_effective_address_with_transport() {
        let addr = TransportAddress::named_pipe("test-pipe");
        let entry = ServiceEntry::with_address("maya", addr.clone());
        assert_eq!(entry.effective_address(), addr);
    }

    #[test]
    fn test_effective_address_fallback_tcp() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let effective = entry.effective_address();
        assert!(effective.is_tcp());
        assert_eq!(effective.tcp_parts(), Some(("127.0.0.1", 18812)));
    }

    #[test]
    fn test_is_ipc_false_for_tcp() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        assert!(!entry.is_ipc());
    }

    #[test]
    fn test_is_ipc_false_for_tcp_transport_address() {
        let addr = TransportAddress::tcp("10.0.0.1", 9090);
        let entry = ServiceEntry::with_address("maya", addr);
        assert!(!entry.is_ipc());
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
        // transport_address should be None and not serialized
        assert!(deserialized.transport_address.is_none());
    }

    #[test]
    fn test_service_entry_serialization_with_address() {
        let addr = TransportAddress::named_pipe("test");
        let entry = ServiceEntry::with_address("maya", addr.clone());
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: ServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.transport_address, Some(addr));
        assert!(deserialized.is_ipc());
    }

    #[test]
    fn test_service_entry_last_heartbeat_is_recent() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let heartbeat_ms = entry
            .last_heartbeat
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // Should be within 1 second of now
        assert!(now_ms.abs_diff(heartbeat_ms) < 1000);
    }

    #[test]
    fn test_service_entry_touch_updates_heartbeat() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        // Force old heartbeat
        entry.last_heartbeat = SystemTime::now() - Duration::from_secs(60);
        let old_ms = entry
            .last_heartbeat
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        entry.touch();

        let new_ms = entry
            .last_heartbeat
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(new_ms > old_ms);
    }
}
