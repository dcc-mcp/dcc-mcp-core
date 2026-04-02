//! Python bindings for the transport layer via PyO3.
//!
//! Exposes `PyTransportManager`, `PyServiceEntry`, and `PyServiceStatus` as Python classes.
//! Async operations are bridged to synchronous calls via an internal Tokio runtime.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::config::{PoolConfig, SessionConfig, TransportConfig};
#[cfg(feature = "python-bindings")]
use crate::discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};
#[cfg(feature = "python-bindings")]
use crate::transport::TransportManager;

// ── PyServiceStatus ──

/// Python-facing enum for DCC service status.
///
/// ```python
/// from dcc_mcp_core import ServiceStatus
///
/// status = ServiceStatus.AVAILABLE
/// print(status)  # "AVAILABLE"
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ServiceStatus", eq)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyServiceStatus {
    /// Service is available and accepting connections.
    #[pyo3(name = "AVAILABLE")]
    Available,
    /// Service is busy (processing a request).
    #[pyo3(name = "BUSY")]
    Busy,
    /// Service is unreachable (health check failed).
    #[pyo3(name = "UNREACHABLE")]
    Unreachable,
    /// Service is shutting down.
    #[pyo3(name = "SHUTTING_DOWN")]
    ShuttingDown,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyServiceStatus {
    fn __repr__(&self) -> String {
        format!("ServiceStatus.{}", self.__str__())
    }

    fn __str__(&self) -> &'static str {
        match self {
            Self::Available => "AVAILABLE",
            Self::Busy => "BUSY",
            Self::Unreachable => "UNREACHABLE",
            Self::ShuttingDown => "SHUTTING_DOWN",
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<ServiceStatus> for PyServiceStatus {
    fn from(s: ServiceStatus) -> Self {
        match s {
            ServiceStatus::Available => PyServiceStatus::Available,
            ServiceStatus::Busy => PyServiceStatus::Busy,
            ServiceStatus::Unreachable => PyServiceStatus::Unreachable,
            ServiceStatus::ShuttingDown => PyServiceStatus::ShuttingDown,
        }
    }
}

// ── PyServiceEntry ──

/// Python-facing service entry representing a discovered DCC instance.
///
/// ```python
/// from dcc_mcp_core import TransportManager
///
/// transport = TransportManager("/path/to/registry")
/// instance_id = transport.register_service("maya", "127.0.0.1", 18812)
/// entry = transport.get_service("maya", instance_id)
/// print(entry.dcc_type)      # "maya"
/// print(entry.host)          # "127.0.0.1"
/// print(entry.port)          # 18812
/// print(entry.status)        # ServiceStatus.AVAILABLE
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ServiceEntry", get_all)]
#[derive(Debug, Clone)]
pub struct PyServiceEntry {
    /// DCC application type (e.g. "maya", "houdini", "blender").
    pub dcc_type: String,
    /// Unique instance identifier (UUID string).
    pub instance_id: String,
    /// Host address.
    pub host: String,
    /// Port number.
    pub port: u16,
    /// DCC application version (e.g. "2024.2").
    pub version: Option<String>,
    /// Currently open scene/file.
    pub scene: Option<String>,
    /// Arbitrary metadata.
    pub metadata: HashMap<String, String>,
    /// Current service status.
    pub status: PyServiceStatus,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyServiceEntry {
    fn __repr__(&self) -> String {
        format!(
            "ServiceEntry(dcc_type={:?}, host={:?}, port={}, instance_id={:?}, status={})",
            self.dcc_type,
            self.host,
            self.port,
            self.instance_id,
            self.status.__str__()
        )
    }

    /// Convert to a dictionary for backward compatibility.
    fn to_dict(&self, py: Python) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("dcc_type", &self.dcc_type)?;
        dict.set_item("instance_id", &self.instance_id)?;
        dict.set_item("host", &self.host)?;
        dict.set_item("port", self.port)?;
        dict.set_item("version", &self.version)?;
        dict.set_item("scene", &self.scene)?;
        dict.set_item("metadata", &self.metadata)?;
        dict.set_item("status", self.status.__str__())?;
        Ok(dict.unbind().into_any())
    }
}

#[cfg(feature = "python-bindings")]
impl From<&ServiceEntry> for PyServiceEntry {
    fn from(entry: &ServiceEntry) -> Self {
        Self {
            dcc_type: entry.dcc_type.clone(),
            instance_id: entry.instance_id.to_string(),
            host: entry.host.clone(),
            port: entry.port,
            version: entry.version.clone(),
            scene: entry.scene.clone(),
            metadata: entry.metadata.clone(),
            status: entry.status.into(),
        }
    }
}

// ── PyTransportManager ──

/// Python-facing TransportManager.
///
/// Wraps the Rust `TransportManager` with a Tokio runtime for async→sync bridging.
///
/// ```python
/// from dcc_mcp_core import TransportManager
///
/// transport = TransportManager("/path/to/registry")
/// transport.register_service("maya", "127.0.0.1", 18812)
/// instances = transport.list_instances("maya")
/// session_id = transport.get_or_create_session("maya")
/// transport.shutdown()
/// ```
#[cfg(feature = "python-bindings")]
#[pyclass(name = "TransportManager")]
pub struct PyTransportManager {
    inner: TransportManager,
    runtime: tokio::runtime::Runtime,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyTransportManager {
    /// Create a new TransportManager.
    ///
    /// Args:
    ///     registry_dir: Directory for the file-based service registry.
    ///     max_connections_per_dcc: Maximum connections per DCC type (default: 10).
    ///     idle_timeout: Session idle timeout in seconds (default: 300).
    ///     heartbeat_interval: Heartbeat interval in seconds (default: 5).
    ///     connect_timeout: Connection timeout in seconds (default: 10).
    ///     reconnect_max_retries: Max reconnection retries (default: 3).
    #[new]
    #[pyo3(signature = (
        registry_dir,
        max_connections_per_dcc=10,
        idle_timeout=300,
        heartbeat_interval=5,
        connect_timeout=10,
        reconnect_max_retries=3,
    ))]
    fn py_new(
        registry_dir: &str,
        max_connections_per_dcc: usize,
        idle_timeout: u64,
        heartbeat_interval: u64,
        connect_timeout: u64,
        reconnect_max_retries: u32,
    ) -> PyResult<Self> {
        let config = TransportConfig {
            pool: PoolConfig {
                max_connections_per_type: max_connections_per_dcc,
                ..Default::default()
            },
            session: SessionConfig {
                idle_timeout: std::time::Duration::from_secs(idle_timeout),
                reconnect_max_retries,
                heartbeat_interval: std::time::Duration::from_secs(heartbeat_interval),
                ..Default::default()
            },
            connect_timeout: std::time::Duration::from_secs(connect_timeout),
            heartbeat_interval: std::time::Duration::from_secs(heartbeat_interval),
        };

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "failed to create tokio runtime: {}",
                e
            ))
        })?;

        let inner = TransportManager::new(config, registry_dir)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        Ok(Self { inner, runtime })
    }

    // ── Service Discovery ──

    /// Register a DCC service instance.
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. "maya", "houdini").
    ///     host: Host address (e.g. "127.0.0.1").
    ///     port: Port number.
    ///     version: DCC version string (optional).
    ///     scene: Currently open scene/file (optional).
    ///     metadata: Arbitrary metadata dict (optional).
    ///
    /// Returns:
    ///     The instance_id (UUID string) of the registered service.
    #[pyo3(name = "register_service")]
    #[pyo3(signature = (dcc_type, host, port, version=None, scene=None, metadata=None))]
    fn py_register_service(
        &self,
        dcc_type: &str,
        host: &str,
        port: u16,
        version: Option<String>,
        scene: Option<String>,
        metadata: Option<HashMap<String, String>>,
    ) -> PyResult<String> {
        let mut entry = ServiceEntry::new(dcc_type, host, port);
        entry.version = version;
        entry.scene = scene;
        if let Some(md) = metadata {
            entry.metadata = md;
        }
        let instance_id = entry.instance_id.to_string();
        self.inner
            .register_service(entry)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(instance_id)
    }

    /// Deregister a DCC service instance.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///
    /// Returns:
    ///     True if the service was found and removed.
    #[pyo3(name = "deregister_service")]
    fn py_deregister_service(&self, dcc_type: &str, instance_id: &str) -> PyResult<bool> {
        let uuid = uuid::Uuid::parse_str(instance_id)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {}", e)))?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        let result = self
            .inner
            .deregister_service(&key)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(result.is_some())
    }

    /// List all instances for a given DCC type.
    ///
    /// Returns:
    ///     List of ServiceEntry objects.
    #[pyo3(name = "list_instances")]
    fn py_list_instances(&self, dcc_type: &str) -> Vec<PyServiceEntry> {
        self.inner
            .list_instances(dcc_type)
            .iter()
            .map(PyServiceEntry::from)
            .collect()
    }

    /// List all registered services.
    ///
    /// Returns:
    ///     List of ServiceEntry objects.
    #[pyo3(name = "list_all_services")]
    fn py_list_all_services(&self) -> Vec<PyServiceEntry> {
        self.inner
            .list_all_services()
            .iter()
            .map(PyServiceEntry::from)
            .collect()
    }

    /// Get a specific service entry by DCC type and instance ID.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Instance UUID string.
    ///
    /// Returns:
    ///     ServiceEntry if found, None otherwise.
    #[pyo3(name = "get_service")]
    fn py_get_service(
        &self,
        dcc_type: &str,
        instance_id: &str,
    ) -> PyResult<Option<PyServiceEntry>> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        Ok(self
            .inner
            .get_service(&key)
            .as_ref()
            .map(PyServiceEntry::from))
    }

    /// Update heartbeat for a service.
    #[pyo3(name = "heartbeat")]
    fn py_heartbeat(&self, dcc_type: &str, instance_id: &str) -> PyResult<bool> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        self.inner
            .heartbeat(&key)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    // ── Session Management ──

    /// Get or create a session for a DCC instance (lazy creation).
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///     instance_id: Specific instance UUID (optional). If None, picks first available.
    ///
    /// Returns:
    ///     Session ID (UUID string).
    #[pyo3(name = "get_or_create_session")]
    #[pyo3(signature = (dcc_type, instance_id=None))]
    fn py_get_or_create_session(
        &self,
        dcc_type: &str,
        instance_id: Option<&str>,
    ) -> PyResult<String> {
        let uuid = instance_id.map(parse_uuid).transpose()?;
        let session_id = self
            .inner
            .get_or_create_session(dcc_type, uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(session_id.to_string())
    }

    /// Get session info by ID.
    ///
    /// Returns:
    ///     Dict with session info, or None if not found.
    #[pyo3(name = "get_session")]
    fn py_get_session(&self, py: Python, session_id: &str) -> PyResult<Option<Py<PyAny>>> {
        let uuid = parse_uuid(session_id)?;
        Ok(self.inner.get_session(&uuid).map(|s| session_to_py(py, &s)))
    }

    /// Record a successful request on a session.
    #[pyo3(name = "record_success")]
    fn py_record_success(&self, session_id: &str, latency_ms: u64) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .record_request_success(&uuid, std::time::Duration::from_millis(latency_ms));
        Ok(())
    }

    /// Record a failed request on a session.
    #[pyo3(name = "record_error")]
    fn py_record_error(&self, session_id: &str, latency_ms: u64, error: &str) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .record_request_error(&uuid, std::time::Duration::from_millis(latency_ms), error);
        Ok(())
    }

    /// Begin reconnection. Returns backoff duration in milliseconds.
    #[pyo3(name = "begin_reconnect")]
    fn py_begin_reconnect(&self, session_id: &str) -> PyResult<u64> {
        let uuid = parse_uuid(session_id)?;
        let backoff = self
            .inner
            .begin_reconnect(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(backoff.as_millis() as u64)
    }

    /// Mark reconnection as successful.
    #[pyo3(name = "reconnect_success")]
    fn py_reconnect_success(&self, session_id: &str) -> PyResult<()> {
        let uuid = parse_uuid(session_id)?;
        self.inner
            .reconnect_success(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Close a session.
    #[pyo3(name = "close_session")]
    fn py_close_session(&self, session_id: &str) -> PyResult<bool> {
        let uuid = parse_uuid(session_id)?;
        let result = self
            .inner
            .close_session(&uuid)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(result.is_some())
    }

    /// List all active sessions.
    #[pyo3(name = "list_sessions")]
    fn py_list_sessions(&self, py: Python) -> Vec<Py<PyAny>> {
        self.inner
            .list_sessions()
            .iter()
            .map(|s| session_to_py(py, s))
            .collect()
    }

    /// List sessions for a specific DCC type.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///
    /// Returns:
    ///     List of session info dicts for the given DCC type.
    #[pyo3(name = "list_sessions_for_dcc")]
    fn py_list_sessions_for_dcc(&self, py: Python, dcc_type: &str) -> Vec<Py<PyAny>> {
        self.inner
            .list_sessions_for_dcc(dcc_type)
            .iter()
            .map(|s| session_to_py(py, s))
            .collect()
    }

    /// Get the number of active sessions.
    #[pyo3(name = "session_count")]
    fn py_session_count(&self) -> usize {
        self.inner.session_count()
    }

    // ── Connection Pool ──

    /// Acquire a connection (async bridged to sync).
    #[pyo3(name = "acquire_connection")]
    #[pyo3(signature = (dcc_type, instance_id=None))]
    fn py_acquire_connection(&self, dcc_type: &str, instance_id: Option<&str>) -> PyResult<String> {
        let uuid = instance_id.map(parse_uuid).transpose()?;
        let conn_id = self
            .runtime
            .block_on(self.inner.acquire_connection(dcc_type, uuid))
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(conn_id.to_string())
    }

    /// Release a connection back to the pool.
    #[pyo3(name = "release_connection")]
    fn py_release_connection(&self, dcc_type: &str, instance_id: &str) -> PyResult<()> {
        let uuid = parse_uuid(instance_id)?;
        let key = ServiceKey {
            dcc_type: dcc_type.to_string(),
            instance_id: uuid,
        };
        self.inner.release_connection(&key);
        Ok(())
    }

    /// Get pool size.
    #[pyo3(name = "pool_size")]
    fn py_pool_size(&self) -> usize {
        self.inner.pool_size()
    }

    /// Get the number of connections for a specific DCC type.
    ///
    /// Args:
    ///     dcc_type: DCC application type.
    ///
    /// Returns:
    ///     Number of pooled connections for the given DCC type.
    #[pyo3(name = "pool_count_for_dcc")]
    fn py_pool_count_for_dcc(&self, dcc_type: &str) -> usize {
        self.inner.pool_count_for_dcc(dcc_type)
    }

    // ── Lifecycle ──

    /// Cleanup stale services, idle sessions, and evict idle connections.
    ///
    /// Returns:
    ///     Tuple of (stale_services, closed_sessions, evicted_connections).
    #[pyo3(name = "cleanup")]
    fn py_cleanup(&self) -> PyResult<(usize, usize, usize)> {
        self.inner
            .cleanup()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Gracefully shut down the transport.
    #[pyo3(name = "shutdown")]
    fn py_shutdown(&self) {
        self.inner.shutdown();
    }

    /// Check if the transport is shut down.
    #[pyo3(name = "is_shutdown")]
    fn py_is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    fn __repr__(&self) -> String {
        format!(
            "TransportManager(services={}, sessions={}, pool={})",
            self.inner.list_all_services().len(),
            self.inner.session_count(),
            self.inner.pool_size(),
        )
    }

    fn __len__(&self) -> usize {
        self.inner.session_count()
    }
}

// ── Helper functions ──

#[cfg(feature = "python-bindings")]
fn parse_uuid(s: &str) -> PyResult<uuid::Uuid> {
    uuid::Uuid::parse_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {}", e)))
}

#[cfg(feature = "python-bindings")]
fn session_to_py(py: Python, session: &crate::session::Session) -> Py<PyAny> {
    let dict = PyDict::new(py);
    let _ = dict.set_item("id", session.id.to_string());
    let _ = dict.set_item("dcc_type", &session.dcc_type);
    let _ = dict.set_item("instance_id", session.instance_id.to_string());
    let _ = dict.set_item("host", &session.host);
    let _ = dict.set_item("port", session.port);
    let _ = dict.set_item("state", session.state.to_string());
    let _ = dict.set_item("request_count", session.metrics.request_count);
    let _ = dict.set_item("error_count", session.metrics.error_count);
    let _ = dict.set_item("avg_latency_ms", session.metrics.avg_latency_ms());
    let _ = dict.set_item("error_rate", session.metrics.error_rate());
    let _ = dict.set_item("reconnect_attempts", session.reconnect_attempts);
    dict.unbind().into_any()
}
