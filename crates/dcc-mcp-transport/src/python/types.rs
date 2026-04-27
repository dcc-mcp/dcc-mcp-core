//! Python-facing type definitions for the transport layer.
//!
//! Contains `PyServiceStatus`, `PyTransportAddress`, `PyTransportScheme`,
//! and `PyServiceEntry`.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyclass_enum, gen_stub_pymethods};
#[cfg(feature = "python-bindings")]
use std::collections::HashMap;

#[cfg(feature = "python-bindings")]
use crate::discovery::types::{ServiceEntry, ServiceStatus};
#[cfg(feature = "python-bindings")]
use crate::ipc::{TransportAddress, TransportScheme};

// ── PyServiceStatus ──

/// Python-facing enum for DCC service status.
///
/// ```python,ignore
/// from dcc_mcp_core import ServiceStatus
///
/// status = ServiceStatus.AVAILABLE
/// print(status)  # "AVAILABLE"
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ServiceStatus", eq, from_py_object)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

// ── PyTransportAddress ──

/// Python-facing transport address for DCC communication.
///
/// Represents a protocol-agnostic endpoint: TCP, Named Pipe, or Unix Socket.
///
/// ```python,ignore
/// from dcc_mcp_core import TransportAddress
///
/// # TCP address
/// addr = TransportAddress.tcp("127.0.0.1", 18812)
///
/// # Named Pipe (Windows)
/// addr = TransportAddress.named_pipe("dcc-mcp-maya-12345")
///
/// # Unix Socket (macOS/Linux)
/// addr = TransportAddress.unix_socket("/tmp/dcc-mcp-maya.sock")
///
/// # Auto-detect best local transport
/// addr = TransportAddress.default_local("maya", 12345)
///
/// # Parse from URI string
/// addr = TransportAddress.parse("tcp://127.0.0.1:9000")
///
/// print(addr.scheme)      # "tcp", "pipe", or "unix"
/// print(addr.is_local)    # True/False
/// print(str(addr))        # "tcp://127.0.0.1:18812"
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "TransportAddress", from_py_object)]
#[derive(Debug, Clone)]
pub struct PyTransportAddress {
    pub(super) inner: TransportAddress,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyTransportAddress {
    /// Create a TCP transport address.
    #[staticmethod]
    fn tcp(host: &str, port: u16) -> Self {
        Self {
            inner: TransportAddress::tcp(host, port),
        }
    }

    /// Create a Named Pipe transport address (Windows).
    ///
    /// Automatically prepends `\\.\pipe\` if not already present.
    #[staticmethod]
    fn named_pipe(name: &str) -> Self {
        Self {
            inner: TransportAddress::named_pipe(name),
        }
    }

    /// Create a Unix Domain Socket transport address (macOS/Linux).
    #[staticmethod]
    fn unix_socket(path: &str) -> Self {
        Self {
            inner: TransportAddress::unix_socket(path),
        }
    }

    /// Generate the optimal local transport for the current platform.
    ///
    /// Windows → Named Pipe, macOS/Linux → Unix Socket.
    #[staticmethod]
    fn default_local(dcc_type: &str, pid: u32) -> Self {
        Self {
            inner: TransportAddress::default_local(dcc_type, pid),
        }
    }

    /// Generate a default Named Pipe name for a DCC instance.
    #[staticmethod]
    fn default_pipe_name(dcc_type: &str, pid: u32) -> Self {
        Self {
            inner: TransportAddress::default_pipe_name(dcc_type, pid),
        }
    }

    /// Generate a default Unix Socket path for a DCC instance.
    #[staticmethod]
    fn default_unix_socket(dcc_type: &str, pid: u32) -> Self {
        Self {
            inner: TransportAddress::default_unix_socket(dcc_type, pid),
        }
    }

    /// Parse a transport address from a URI string.
    ///
    /// Supports ``tcp://host:port``, ``pipe://name``, and ``unix://path`` schemes.
    ///
    /// # Examples (Python)
    ///
    /// ```python,ignore
    /// addr = TransportAddress.parse("tcp://127.0.0.1:9000")
    /// assert addr.is_tcp
    ///
    /// addr2 = TransportAddress.parse("pipe://dcc-mcp-maya")
    /// assert addr2.is_named_pipe
    /// ```
    #[staticmethod]
    fn parse(s: &str) -> pyo3::PyResult<Self> {
        TransportAddress::parse(s)
            .map(|inner| Self { inner })
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Get the transport scheme name ("tcp", "pipe", or "unix").
    #[getter]
    fn scheme(&self) -> &str {
        self.inner.scheme()
    }

    /// Check if this is a local (same-machine) transport.
    #[getter]
    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    /// Check if this is a TCP address.
    #[getter]
    fn is_tcp(&self) -> bool {
        self.inner.is_tcp()
    }

    /// Check if this is a Named Pipe address.
    #[getter]
    fn is_named_pipe(&self) -> bool {
        self.inner.is_named_pipe()
    }

    /// Check if this is a Unix Domain Socket address.
    #[getter]
    fn is_unix_socket(&self) -> bool {
        self.inner.is_unix_socket()
    }

    /// Get the connection string (e.g. "tcp://127.0.0.1:18812").
    fn to_connection_string(&self) -> String {
        self.inner.to_connection_string()
    }

    fn __repr__(&self) -> String {
        format!("TransportAddress({})", self.inner.to_connection_string())
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }

    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.to_connection_string().hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(feature = "python-bindings")]
impl From<&TransportAddress> for PyTransportAddress {
    fn from(addr: &TransportAddress) -> Self {
        Self {
            inner: addr.clone(),
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&PyTransportAddress> for TransportAddress {
    fn from(addr: &PyTransportAddress) -> Self {
        addr.inner.clone()
    }
}

// ── PyTransportScheme ──

/// Python-facing transport selection strategy.
///
/// ```python,ignore
/// from dcc_mcp_core import TransportScheme
///
/// scheme = TransportScheme.AUTO          # Auto-detect best transport
/// scheme = TransportScheme.TCP_ONLY      # Always use TCP
/// scheme = TransportScheme.PREFER_IPC    # Prefer IPC, fallback to TCP
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass_enum)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "TransportScheme", eq, from_py_object)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyTransportScheme {
    /// Automatically select the best transport based on locality.
    #[pyo3(name = "AUTO")]
    Auto,
    /// Always use TCP.
    #[pyo3(name = "TCP_ONLY")]
    TcpOnly,
    /// Prefer Named Pipe on Windows, fall back to TCP.
    #[pyo3(name = "PREFER_NAMED_PIPE")]
    PreferNamedPipe,
    /// Prefer Unix Domain Socket, fall back to TCP.
    #[pyo3(name = "PREFER_UNIX_SOCKET")]
    PreferUnixSocket,
    /// Prefer any IPC, fall back to TCP.
    #[pyo3(name = "PREFER_IPC")]
    PreferIpc,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl PyTransportScheme {
    /// Select the optimal transport address for a connection.
    ///
    /// Args:
    ///     dcc_type: DCC application type (e.g. "maya").
    ///     host: Target host address.
    ///     port: Target port.
    ///     pid: Optional DCC process PID (needed for IPC naming).
    ///
    /// Returns:
    ///     TransportAddress — the selected transport endpoint.
    #[pyo3(signature = (dcc_type, host, port, pid=None))]
    fn select_address(
        &self,
        dcc_type: &str,
        host: &str,
        port: u16,
        pid: Option<u32>,
    ) -> PyTransportAddress {
        let scheme: TransportScheme = self.into();
        PyTransportAddress {
            inner: scheme.select_address(dcc_type, host, port, pid),
        }
    }

    fn __repr__(&self) -> String {
        format!("TransportScheme.{}", self.__str__())
    }

    fn __str__(&self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::TcpOnly => "TCP_ONLY",
            Self::PreferNamedPipe => "PREFER_NAMED_PIPE",
            Self::PreferUnixSocket => "PREFER_UNIX_SOCKET",
            Self::PreferIpc => "PREFER_IPC",
        }
    }
}

#[cfg(feature = "python-bindings")]
impl From<&PyTransportScheme> for TransportScheme {
    fn from(s: &PyTransportScheme) -> Self {
        match s {
            PyTransportScheme::Auto => TransportScheme::Auto,
            PyTransportScheme::TcpOnly => TransportScheme::TcpOnly,
            PyTransportScheme::PreferNamedPipe => TransportScheme::PreferNamedPipe,
            PyTransportScheme::PreferUnixSocket => TransportScheme::PreferUnixSocket,
            PyTransportScheme::PreferIpc => TransportScheme::PreferIpc,
        }
    }
}

// ── PyServiceEntry ──

/// Python-facing service entry representing a discovered DCC instance.
///
/// ```python,ignore
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
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg(feature = "python-bindings")]
#[pyclass(name = "ServiceEntry", from_py_object)]
#[derive(Debug, Clone)]
pub struct PyServiceEntry {
    /// DCC application type (e.g. "maya", "houdini", "blender").
    #[pyo3(get)]
    pub dcc_type: String,
    /// Unique instance identifier (UUID string).
    #[pyo3(get)]
    pub instance_id: String,
    /// Host address.
    #[pyo3(get)]
    pub host: String,
    /// Port number.
    #[pyo3(get)]
    pub port: u16,
    /// DCC application version (e.g. "2024.2").
    #[pyo3(get)]
    pub version: Option<String>,
    /// Currently active scene / document.
    ///
    /// For single-document DCCs this is the open file path.
    /// For multi-document apps (Photoshop) this is the **focused** document.
    #[pyo3(get)]
    pub scene: Option<String>,
    /// All documents currently open in this instance.
    ///
    /// Empty for DCCs that only support one document at a time.
    /// For multi-document apps each element is a file path.
    #[pyo3(get)]
    pub documents: Vec<String>,
    /// OS process ID of the DCC process.
    ///
    /// Useful for disambiguating two instances of the same DCC type
    /// that have the same scene open.
    #[pyo3(get)]
    pub pid: Option<u32>,
    /// Human-readable label for this instance (e.g. `"Maya-Rigging"`).
    ///
    /// Set by the bridge plugin at registration time.  Displayed by the
    /// agent when asking the user to choose between multiple instances.
    #[pyo3(get)]
    pub display_name: Option<String>,
    /// Arbitrary metadata.
    #[pyo3(get)]
    pub metadata: HashMap<String, String>,
    /// Current service status.
    #[pyo3(get)]
    pub status: PyServiceStatus,
    /// Transport address (None = TCP host:port).
    #[pyo3(get)]
    pub transport_address: Option<PyTransportAddress>,
    /// Last heartbeat timestamp in milliseconds since Unix epoch.
    ///
    /// Useful for `LazySessionPool` implementations to determine if a session
    /// has been idle too long and should be evicted.  Updated by
    /// :meth:`TransportManager.heartbeat`.
    #[pyo3(get)]
    pub last_heartbeat_ms: u64,
    /// Arbitrary DCC-specific extras as JSON-typed values.
    ///
    /// Exposed to Python via the [`extras`] property getter which returns
    /// a fresh `dict[str, Any]` with nested JSON values recursively converted.
    pub(super) extras: HashMap<String, serde_json::Value>,
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

    /// Check if this service uses an IPC transport.
    #[getter]
    fn is_ipc(&self) -> bool {
        self.transport_address
            .as_ref()
            .is_some_and(|addr| !addr.inner.is_tcp())
    }

    /// Get the effective transport address.
    fn effective_address(&self) -> PyTransportAddress {
        self.transport_address
            .clone()
            .unwrap_or_else(|| PyTransportAddress {
                inner: TransportAddress::tcp(&self.host, self.port),
            })
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
        dict.set_item("documents", &self.documents)?;
        dict.set_item("pid", self.pid)?;
        dict.set_item("display_name", &self.display_name)?;
        dict.set_item("metadata", &self.metadata)?;
        dict.set_item("status", self.status.__str__())?;
        dict.set_item("last_heartbeat_ms", self.last_heartbeat_ms)?;
        dict.set_item("extras", self.extras(py)?)?;
        if let Some(addr) = &self.transport_address {
            dict.set_item("transport_address", addr.to_connection_string())?;
        }
        Ok(dict.unbind().into_any())
    }

    /// Arbitrary DCC-specific extras as a `dict[str, Any]`.
    ///
    /// Nested JSON values (objects / arrays / numbers / booleans / nulls) are
    /// recursively converted into native Python types.  Returns an empty dict
    /// when no extras were registered.
    #[getter]
    fn extras<'py>(&self, py: Python<'py>) -> PyResult<Py<PyDict>> {
        let dict = PyDict::new(py);
        for (k, v) in &self.extras {
            dict.set_item(k, dcc_mcp_pybridge::py_json::json_value_to_bound_py(py, v)?)?;
        }
        Ok(dict.unbind())
    }
}

#[cfg(feature = "python-bindings")]
impl From<&ServiceEntry> for PyServiceEntry {
    fn from(entry: &ServiceEntry) -> Self {
        let last_heartbeat_ms = entry
            .last_heartbeat
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            dcc_type: entry.dcc_type.clone(),
            instance_id: entry.instance_id.to_string(),
            host: entry.host.clone(),
            port: entry.port,
            version: entry.version.clone(),
            scene: entry.scene.clone(),
            documents: entry.documents.clone(),
            pid: entry.pid,
            display_name: entry.display_name.clone(),
            metadata: entry.metadata.clone(),
            status: entry.status.into(),
            transport_address: entry
                .transport_address
                .as_ref()
                .map(PyTransportAddress::from),
            last_heartbeat_ms,
            extras: entry.extras.clone(),
        }
    }
}

#[cfg(feature = "python-bindings")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::types::ServiceEntry;

    mod test_py_service_entry {
        use super::*;

        #[test]
        fn test_last_heartbeat_ms_is_recent() {
            let rust_entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
            let py_entry = PyServiceEntry::from(&rust_entry);

            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;

            // last_heartbeat_ms must be a valid Unix epoch ms (within 2 seconds of now)
            assert!(py_entry.last_heartbeat_ms > 0);
            assert!(now_ms.abs_diff(py_entry.last_heartbeat_ms) < 2000);
        }

        #[test]
        fn test_last_heartbeat_ms_reflects_touch() {
            let mut rust_entry = ServiceEntry::new("houdini", "127.0.0.1", 9090);
            // Force old heartbeat
            rust_entry.last_heartbeat =
                std::time::SystemTime::now() - std::time::Duration::from_secs(60);

            let py_before = PyServiceEntry::from(&rust_entry);

            // Touch updates heartbeat
            rust_entry.touch();
            let py_after = PyServiceEntry::from(&rust_entry);

            assert!(py_after.last_heartbeat_ms > py_before.last_heartbeat_ms);
        }

        #[test]
        fn test_is_ipc_false_for_tcp_entry() {
            let rust_entry = ServiceEntry::new("blender", "127.0.0.1", 8080);
            let py_entry = PyServiceEntry::from(&rust_entry);
            assert!(!py_entry.is_ipc());
        }

        #[test]
        fn test_is_ipc_true_for_named_pipe() {
            use crate::ipc::TransportAddress;
            let addr = TransportAddress::named_pipe("test-pipe");
            let rust_entry = ServiceEntry::with_address("maya", addr);
            let py_entry = PyServiceEntry::from(&rust_entry);
            assert!(py_entry.is_ipc());
        }

        #[test]
        fn test_effective_address_falls_back_to_tcp() {
            let rust_entry = ServiceEntry::new("maya", "10.0.0.1", 18812);
            let py_entry = PyServiceEntry::from(&rust_entry);
            let addr = py_entry.effective_address();
            assert!(addr.inner.is_tcp());
        }
    }
}
