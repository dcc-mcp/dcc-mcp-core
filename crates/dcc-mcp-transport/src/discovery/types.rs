//! Service discovery types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Custom deserializer for `SystemTime` that accepts both Unix timestamp numbers
/// (integer or float — as written by Python bridge plugins) and the Rust std serde
/// struct format `{"secs_since_epoch": N, "nanos_since_epoch": N}`.
fn deserialize_system_time<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    system_time_from_json_value(&value).map_err(serde::de::Error::custom)
}

/// Custom deserializer for `Option<SystemTime>` (used by `lease_expires_at`).
fn deserialize_optional_system_time<'de, D>(
    deserializer: D,
) -> Result<Option<SystemTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    system_time_from_json_value(&value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

fn system_time_from_json_value(value: &serde_json::Value) -> Result<SystemTime, String> {
    match value {
        serde_json::Value::Number(n) => {
            let secs_f64 = n.as_f64().ok_or_else(|| format!("invalid number: {n}"))?;
            #[allow(clippy::cast_possible_truncation)]
            #[allow(clippy::cast_sign_loss)]
            let secs = secs_f64.trunc() as u64;
            let nanos = ((secs_f64 - secs_f64.trunc()).abs() * 1e9) as u32;
            UNIX_EPOCH
                .checked_add(Duration::new(secs, nanos))
                .ok_or_else(|| format!("timestamp out of range: {secs_f64}"))
        }
        serde_json::Value::Object(obj) => {
            let secs = obj
                .get("secs_since_epoch")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "missing secs_since_epoch".to_string())?;
            let nanos = obj
                .get("nanos_since_epoch")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            UNIX_EPOCH
                .checked_add(Duration::new(secs, nanos))
                .ok_or_else(|| format!("timestamp out of range: {secs}.{nanos:09}"))
        }
        _ => Err(format!(
            "expected Unix timestamp number or {{secs_since_epoch, nanos_since_epoch}} object, got {value}"
        )),
    }
}

use crate::ipc::TransportAddress;

fn default_capacity() -> u32 {
    1
}

fn is_default_capacity(capacity: &u32) -> bool {
    *capacity == default_capacity()
}

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
    /// Service process is alive but its embedded DCC host is still
    /// initialising (`GET /v1/readyz` returns `503` with `dcc=false`
    /// or `dispatcher=false`). Introduced in #713 to distinguish the
    /// "Maya main thread busy with plugin init" window from a truly
    /// dead backend — the row stays in the registry but no traffic
    /// should be routed to it until readiness flips green.
    Booting,
    /// Service has already been marked stale by a probe or registry owner.
    /// This is stronger than heartbeat age and must be treated as unroutable
    /// immediately, even before the gateway's stale timeout elapses.
    Stale,
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available => write!(f, "available"),
            Self::Busy => write!(f, "busy"),
            Self::Unreachable => write!(f, "unreachable"),
            Self::ShuttingDown => write!(f, "shutting_down"),
            Self::Booting => write!(f, "booting"),
            Self::Stale => write!(f, "stale"),
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`).
    ///
    /// Recorded on the `__gateway__` sentinel alongside the embedded
    /// `dcc-mcp-http` crate version so gateway election can compare both
    /// (issue maya#137).  Plain DCC rows may also set this — agents use it
    /// to disambiguate two adapter releases serving the same DCC type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_version: Option<String>,
    /// DCC type the adapter is bound to (e.g. `"maya"`).
    ///
    /// On the `__gateway__` sentinel this is the host DCC of the gateway
    /// owner — used as the third tiebreaker so a real-DCC adapter wins
    /// over a generic standalone server (issue maya#137).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_dcc: Option<String>,
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
    /// OS-held sentinel lock file for crash-resilient liveness checks.
    ///
    /// The owning process holds an exclusive lock while registered. Readers can
    /// take the lock only after the owner exits or crashes, avoiding PID reuse
    /// races while remaining backward-compatible with rows that omit this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentinel_path: Option<std::path::PathBuf>,
    /// Human-readable label for this instance.
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
    #[serde(deserialize_with = "deserialize_system_time")]
    pub registered_at: SystemTime,
    /// Last heartbeat timestamp.
    #[serde(deserialize_with = "deserialize_system_time")]
    pub last_heartbeat: SystemTime,
    /// Current status.
    #[serde(default)]
    pub status: ServiceStatus,
    /// Optional pool capacity for this instance. Defaults to a single lease.
    #[serde(
        default = "default_capacity",
        skip_serializing_if = "is_default_capacity"
    )]
    pub capacity: u32,
    /// Current lease owner, when this instance is reserved for a workflow/client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_owner: Option<String>,
    /// Current job id associated with the lease or busy operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_job_id: Option<String>,
    /// Wall-clock expiry for the current lease.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_system_time"
    )]
    pub lease_expires_at: Option<SystemTime>,
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
            adapter_version: None,
            adapter_dcc: None,
            scene: None,
            documents: Vec::new(),
            pid: Some(std::process::id()),
            sentinel_path: None,
            display_name: None,
            metadata: HashMap::new(),
            extras: HashMap::new(),
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Available,
            capacity: default_capacity(),
            lease_owner: None,
            current_job_id: None,
            lease_expires_at: None,
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
            adapter_version: None,
            adapter_dcc: None,
            scene: None,
            documents: Vec::new(),
            pid: Some(std::process::id()),
            sentinel_path: None,
            display_name: None,
            metadata: HashMap::new(),
            extras: HashMap::new(),
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Available,
            capacity: default_capacity(),
            lease_owner: None,
            current_job_id: None,
            lease_expires_at: None,
        }
    }

    /// Override the owning process PID (useful when registering on behalf of a bridge).
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Stamp the adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`).
    ///
    /// Set on the gateway sentinel so peers can apply the second-tier
    /// election comparison (issue maya#137).
    pub fn with_adapter_version(mut self, version: impl Into<String>) -> Self {
        self.adapter_version = Some(version.into());
        self
    }

    /// Stamp the DCC type the adapter is bound to (e.g. `"maya"`).
    ///
    /// Drives the third-tier "prefer real DCC over unknown standalone"
    /// tiebreaker in gateway election (issue maya#137).
    pub fn with_adapter_dcc(mut self, dcc: impl Into<String>) -> Self {
        self.adapter_dcc = Some(dcc.into());
        self
    }

    /// Set optional pool capacity for this service entry.
    pub fn with_capacity(mut self, capacity: u32) -> Self {
        self.capacity = capacity.max(1);
        self
    }

    /// Human-readable identifier of the form `{dcc}@{version}-{short8}`.
    ///
    /// Examples (RFC #998 Addendum B): `maya@2026-abc12345`,
    /// `houdini@20.5-deadbeef`, `figma@unknown-cafef00d`.
    ///
    /// Derived from `dcc_type`, `version`, and the first 8 hex chars of
    /// `instance_id`. Used by agent-facing surfaces (gateway resources,
    /// admin UI, structured logs, instance-disambiguation prompts) so a
    /// user reading a registry row sees DCC + version + short ID
    /// without having to cross-reference three separate fields.
    ///
    /// When `version` is `None`, the literal `unknown` substitutes.
    ///
    /// # Stability
    ///
    /// The 8-char hex short ID must match
    /// `dcc_mcp_gateway_core::naming::instance_short` byte-for-byte
    /// so a `display_id` and the cursor-safe tool slug for the same
    /// instance always reference the same 8 hex characters. Tests
    /// pin this in [`tests::display_id_short_matches_gateway_naming`].
    ///
    /// # Not serialised
    ///
    /// `display_id` is **derived** rather than stored — `services.json`
    /// shapes round-trip unchanged. Callers that need the value on a
    /// remote surface should embed it explicitly in the response JSON
    /// (e.g. `gateway://instances` does so via [`crate`]-side helpers).
    #[must_use]
    pub fn display_id(&self) -> String {
        let version = self.version.as_deref().unwrap_or("unknown");
        let mut short = self.instance_id.simple().to_string();
        short.truncate(8);
        format!("{}@{}-{}", self.dcc_type, version, short)
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

    /// Whether the current lease has passed its wall-clock expiry.
    pub fn lease_expired(&self, now: SystemTime) -> bool {
        self.lease_expires_at
            .is_some_and(|expires_at| expires_at <= now)
    }

    /// Clear any active lease and mark the instance available.
    pub fn clear_lease(&mut self) {
        self.lease_owner = None;
        self.current_job_id = None;
        self.lease_expires_at = None;
        self.status = ServiceStatus::Available;
        self.touch();
    }

    /// Reserve this instance for a workflow/client.
    pub fn acquire_lease(
        &mut self,
        owner: impl Into<String>,
        current_job_id: Option<String>,
        lease_expires_at: Option<SystemTime>,
    ) {
        self.lease_owner = Some(owner.into());
        self.current_job_id = current_job_id;
        self.lease_expires_at = lease_expires_at;
        self.status = ServiceStatus::Busy;
        self.touch();
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

    // Issue maya#137: adapter_version and adapter_dcc must round-trip
    // through the on-disk JSON and stay absent from the wire format when
    // unset, preserving the existing services.json shape.
    #[test]
    fn test_service_entry_adapter_metadata_roundtrip() {
        let entry = ServiceEntry::new("__gateway__", "127.0.0.1", 9765)
            .with_adapter_version("0.3.0")
            .with_adapter_dcc("maya");
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"adapter_version\":\"0.3.0\""));
        assert!(json.contains("\"adapter_dcc\":\"maya\""));

        let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.adapter_version.as_deref(), Some("0.3.0"));
        assert_eq!(parsed.adapter_dcc.as_deref(), Some("maya"));
    }

    #[test]
    fn test_service_entry_adapter_metadata_omitted_when_unset() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("\"adapter_version\""),
            "unset adapter_version must be skipped: {json}"
        );
        assert!(
            !json.contains("\"adapter_dcc\""),
            "unset adapter_dcc must be skipped: {json}"
        );
    }

    #[test]
    fn test_service_entry_pool_fields_roundtrip_when_set() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812).with_capacity(2);
        entry.acquire_lease(
            "workflow-1",
            Some("job-1".to_string()),
            Some(SystemTime::now() + Duration::from_secs(60)),
        );

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"capacity\":2"));
        assert!(json.contains("\"lease_owner\":\"workflow-1\""));
        assert!(json.contains("\"current_job_id\":\"job-1\""));

        let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.capacity, 2);
        assert_eq!(parsed.status, ServiceStatus::Busy);
        assert_eq!(parsed.lease_owner.as_deref(), Some("workflow-1"));
        assert_eq!(parsed.current_job_id.as_deref(), Some("job-1"));
        assert!(parsed.lease_expires_at.is_some());
    }

    #[test]
    fn test_service_entry_default_pool_fields_omitted() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("\"capacity\""));
        assert!(!json.contains("\"lease_owner\""));
        assert!(!json.contains("\"current_job_id\""));
        assert!(!json.contains("\"lease_expires_at\""));
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

    // ── display_id (RFC #998 Addendum B) ───────────────────────────────

    #[test]
    fn display_id_renders_dcc_at_version_dash_short() {
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        entry.instance_id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        entry.version = Some("2026".to_string());
        assert_eq!(entry.display_id(), "maya@2026-abcdef01");
    }

    #[test]
    fn display_id_falls_back_to_unknown_when_version_missing() {
        let mut entry = ServiceEntry::new("houdini", "127.0.0.1", 9100);
        entry.instance_id = Uuid::parse_str("ffeeddccbbaa99887766554433221100").unwrap();
        entry.version = None;
        assert_eq!(entry.display_id(), "houdini@unknown-ffeeddcc");
    }

    #[test]
    fn display_id_short_matches_gateway_naming() {
        // Pinning the 8-char prefix contract — must stay byte-for-byte
        // aligned with `dcc_mcp_gateway_core::naming::instance_short`
        // (8 leading hex chars of the simple-form UUID). If
        // `instance_short` ever changes its length or alphabet, this
        // test fails loudly so both surfaces get re-aligned in lock
        // step rather than silently drifting.
        let entry = ServiceEntry::new("blender", "127.0.0.1", 18765);
        let id_simple = entry.instance_id.simple().to_string();
        let derived = entry.display_id();
        let short_segment = derived.split('-').next_back().expect("dash present");
        assert_eq!(short_segment.len(), 8);
        assert_eq!(short_segment, &id_simple[..8]);
        assert!(
            short_segment.chars().all(|c| c.is_ascii_hexdigit()),
            "short segment must be ASCII hex, got {short_segment}",
        );
    }

    #[test]
    fn display_id_includes_at_sign_and_dash_separators() {
        let mut entry = ServiceEntry::new("photoshop", "127.0.0.1", 7777);
        entry.version = Some("25.0.0".to_string());
        let display = entry.display_id();
        // Exactly one `@` and exactly one `-` between dcc / version /
        // short. Pin so future refactors don't introduce a third
        // separator (e.g. for prefix), which would break agent UI
        // parsers downstream.
        assert_eq!(display.matches('@').count(), 1);
        assert_eq!(display.matches('-').count(), 1);
        let at_pos = display.find('@').unwrap();
        let dash_pos = display.find('-').unwrap();
        assert!(at_pos < dash_pos, "@ must precede dash: {display}");
    }

    #[test]
    fn display_id_round_trips_through_json_without_being_serialised() {
        // `display_id` is **derived**, not stored. Round-tripping a
        // ServiceEntry through JSON must not contain a `display_id`
        // field — keeps services.json shapes backward-compatible.
        let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        entry.version = Some("2024".to_string());
        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("\"display_id\""),
            "display_id must NOT be serialised; raw JSON: {json}"
        );
        let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
        // The method still works on the deserialised entry.
        assert!(parsed.display_id().starts_with("maya@2024-"));
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

    // ── SystemTime deserializer compatibility (float timestamp fix) ────

    /// Python bridge plugins write `registered_at` / `last_heartbeat` as
    /// float Unix timestamps (e.g. `1712345678.123456`).  The custom
    /// deserializer must accept integer, float, and the Rust std struct
    /// format.
    #[test]
    fn test_system_time_deserialize_from_integer() {
        let now = SystemTime::now();
        let secs = now
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let json = serde_json::json!({
            "dcc_type": "maya",
            "instance_id": "00000000-0000-0000-0000-000000000001",
            "host": "127.0.0.1",
            "port": 18812,
            "registered_at": secs,
            "last_heartbeat": secs,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let got_secs = entry
            .registered_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(got_secs, secs);
    }

    #[test]
    fn test_system_time_deserialize_from_float() {
        let now = SystemTime::now();
        let secs = now
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let json = serde_json::json!({
            "dcc_type": "maya",
            "instance_id": "00000000-0000-0000-0000-000000000001",
            "host": "127.0.0.1",
            "port": 18812,
            "registered_at": secs,
            "last_heartbeat": secs,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let got_secs = entry
            .registered_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(got_secs, secs.trunc() as u64);
    }

    #[test]
    fn test_system_time_deserialize_from_float_with_subsecond() {
        let json = serde_json::json!({
            "dcc_type": "blender",
            "instance_id": "00000000-0000-0000-0000-000000000002",
            "host": "127.0.0.1",
            "port": 8080,
            "registered_at": 1712345678.5,
            "last_heartbeat": 1712345678.5,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let duration = entry
            .registered_at
            .duration_since(UNIX_EPOCH)
            .unwrap();
        assert_eq!(duration.as_secs(), 1712345678);
        assert_eq!(duration.subsec_nanos(), 500_000_000);
    }

    #[test]
    fn test_system_time_deserialize_from_rust_std_struct_format() {
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap();
        let json = serde_json::json!({
            "dcc_type": "houdini",
            "instance_id": "00000000-0000-0000-0000-000000000003",
            "host": "127.0.0.1",
            "port": 9090,
            "registered_at": {
                "secs_since_epoch": duration.as_secs(),
                "nanos_since_epoch": duration.subsec_nanos(),
            },
            "last_heartbeat": {
                "secs_since_epoch": duration.as_secs(),
                "nanos_since_epoch": duration.subsec_nanos(),
            },
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let got_secs = entry
            .registered_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(got_secs, duration.as_secs());
    }

    #[test]
    fn test_optional_system_time_deserialize_from_null() {
        let json = serde_json::json!({
            "dcc_type": "maya",
            "instance_id": "00000000-0000-0000-0000-000000000004",
            "host": "127.0.0.1",
            "port": 18812,
            "registered_at": 1712345678u64,
            "last_heartbeat": 1712345678u64,
            "lease_expires_at": null,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        assert!(entry.lease_expires_at.is_none());
    }

    #[test]
    fn test_optional_system_time_deserialize_from_float() {
        let json = serde_json::json!({
            "dcc_type": "maya",
            "instance_id": "00000000-0000-0000-0000-000000000005",
            "host": "127.0.0.1",
            "port": 18812,
            "registered_at": 1712345678u64,
            "last_heartbeat": 1712345678u64,
            "lease_expires_at": 1712345700.25,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let expires = entry.lease_expires_at.unwrap();
        let duration = expires.duration_since(UNIX_EPOCH).unwrap();
        assert_eq!(duration.as_secs(), 1712345700);
        assert_eq!(duration.subsec_nanos(), 250_000_000);
    }

    /// Roundtrip through the Rust std format must survive (backward compat
    /// with existing services.json written by Rust processes).
    #[test]
    fn test_system_time_full_roundtrip_rust_format() {
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: ServiceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.dcc_type, entry.dcc_type);
        assert_eq!(parsed.instance_id, entry.instance_id);
        assert!(parsed.registered_at <= SystemTime::now());
        assert!(parsed.last_heartbeat <= SystemTime::now());
    }

    /// Roundtrip through a Python-style float timestamp must survive.
    #[test]
    fn test_system_time_roundtrip_float_format() {
        let secs_f64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let json = serde_json::json!({
            "dcc_type": "maya",
            "instance_id": "00000000-0000-0000-0000-000000000006",
            "host": "127.0.0.1",
            "port": 18812,
            "registered_at": secs_f64,
            "last_heartbeat": secs_f64,
        });
        let entry: ServiceEntry = serde_json::from_value(json).unwrap();
        let got_secs = entry
            .registered_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(got_secs, secs_f64.trunc() as u64);
    }
}
