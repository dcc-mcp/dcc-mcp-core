//! PyO3 bindings for the MCP HTTP server.

use pyo3::prelude::*;
use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::{
    config::McpHttpConfig,
    server::{McpHttpServer, ServerHandle},
};
use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_skills::SkillCatalog;

/// Python-visible MCP HTTP server configuration.
///
/// Example::
///
///     from dcc_mcp_core import McpHttpConfig
///     config = McpHttpConfig(port=8765, server_name="my-dcc")
#[pyclass(name = "McpHttpConfig", skip_from_py_object)]
#[derive(Clone)]
pub struct PyMcpHttpConfig {
    pub(crate) inner: McpHttpConfig,
}

#[pymethods]
impl PyMcpHttpConfig {
    /// Create a new config. ``port=0`` binds to any available port.
    #[new]
    #[pyo3(signature = (port=8765, server_name=None, server_version=None, enable_cors=false, request_timeout_ms=30000))]
    fn new(
        port: u16,
        server_name: Option<String>,
        server_version: Option<String>,
        enable_cors: bool,
        request_timeout_ms: u64,
    ) -> Self {
        let mut cfg = McpHttpConfig::new(port);
        if let Some(name) = server_name {
            cfg.server_name = name;
        }
        if let Some(ver) = server_version {
            cfg.server_version = ver;
        }
        cfg.enable_cors = enable_cors;
        cfg.request_timeout_ms = request_timeout_ms;
        Self { inner: cfg }
    }

    #[getter]
    fn port(&self) -> u16 {
        self.inner.port
    }

    #[getter]
    fn server_name(&self) -> &str {
        &self.inner.server_name
    }

    #[getter]
    fn server_version(&self) -> &str {
        &self.inner.server_version
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpConfig(port={}, name={})",
            self.inner.port, self.inner.server_name
        )
    }
}

/// Handle returned by `McpHttpServer.start()`.
///
/// Example::
///
///     handle = server.start()
///     # ... later ...
///     handle.shutdown()
#[pyclass(name = "ServerHandle", skip_from_py_object)]
pub struct PyServerHandle {
    inner: Option<ServerHandle>,
    runtime: Arc<Runtime>,
    pub port: u16,
    pub bind_addr: String,
}

#[pymethods]
impl PyServerHandle {
    /// The actual port the server is listening on.
    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// The bind address (e.g. ``127.0.0.1:8765``).
    #[getter]
    fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    /// The full MCP endpoint URL.
    fn mcp_url(&self) -> String {
        format!("http://{}/mcp", self.bind_addr)
    }

    /// Gracefully shut down the server.
    fn shutdown(&mut self) {
        if let Some(handle) = self.inner.take() {
            self.runtime.block_on(handle.shutdown());
        }
    }

    /// Signal shutdown without blocking.
    fn signal_shutdown(&self) {
        if let Some(handle) = &self.inner {
            handle.signal_shutdown();
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ServerHandle(addr={}, running={})",
            self.bind_addr,
            self.inner.is_some()
        )
    }
}

/// MCP Streamable HTTP server for embedding in DCC software.
///
/// Example::
///
///     from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig
///
///     registry = ActionRegistry()
///     registry.register("get_scene_info", description="Get scene info", category="scene")
///
///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
///     handle = server.start()
///     print(f"MCP server at {handle.mcp_url()}")
///     # MCP Host connects to http://127.0.0.1:8765/mcp
///
///     # Shutdown:
///     handle.shutdown()
#[pyclass(name = "McpHttpServer", skip_from_py_object)]
pub struct PyMcpHttpServer {
    registry: Arc<ActionRegistry>,
    catalog: Arc<SkillCatalog>,
    config: McpHttpConfig,
    runtime: Arc<Runtime>,
}

#[pymethods]
impl PyMcpHttpServer {
    /// Create a new MCP HTTP server.
    ///
    /// Args:
    ///     registry: An ``ActionRegistry`` with registered DCC actions.
    ///     config: A ``McpHttpConfig``. If omitted, defaults to port 8765.
    #[new]
    #[pyo3(signature = (registry, config=None))]
    fn new(registry: &ActionRegistry, config: Option<&PyMcpHttpConfig>) -> PyResult<Self> {
        let cfg = config.map(|c| c.inner.clone()).unwrap_or_default();

        let runtime =
            Runtime::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let reg = Arc::new(registry.clone());
        let catalog = Arc::new(SkillCatalog::new(reg.clone()));

        Ok(Self {
            registry: reg,
            catalog,
            config: cfg,
            runtime: Arc::new(runtime),
        })
    }

    /// Start the server and return a :class:`ServerHandle`.
    ///
    /// This call returns immediately; the server runs in a background thread.
    fn start(&self) -> PyResult<PyServerHandle> {
        let server = McpHttpServer::with_catalog(
            self.registry.clone(),
            self.catalog.clone(),
            self.config.clone(),
        );
        let handle = self
            .runtime
            .block_on(server.start())
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let port = handle.port;
        let bind_addr = handle.bind_addr.clone();

        Ok(PyServerHandle {
            inner: Some(handle),
            runtime: self.runtime.clone(),
            port,
            bind_addr,
        })
    }

    /// Access the server's SkillCatalog for progressive skill loading.
    ///
    /// Returns a debug representation of the catalog state (total/loaded counts).
    /// Use ``discover()``, ``load_skill()``, ``list_skills()`` etc. directly on
    /// the server object to interact with skills.
    #[getter]
    fn catalog(&self) -> String {
        format!(
            "SkillCatalog(total={}, loaded={})",
            self.catalog.len(),
            self.catalog.loaded_count()
        )
    }

    /// Discover skills from standard scan paths.
    ///
    /// Args:
    ///     extra_paths: Additional directories to scan.
    ///     dcc_name: DCC name filter (e.g. ``"maya"``).
    ///
    /// Returns the number of newly discovered skills.
    #[pyo3(signature = (extra_paths=None, dcc_name=None))]
    fn discover(&self, extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> usize {
        self.catalog.discover(extra_paths.as_deref(), dcc_name)
    }

    /// Load a skill by name — registers its tools in the ActionRegistry.
    ///
    /// Returns the list of registered action names.
    /// Raises ``ValueError`` if the skill is not found.
    fn load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.catalog
            .load_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Unload a skill — removes its tools from the ActionRegistry.
    ///
    /// Returns the number of actions removed.
    /// Raises ``ValueError`` if the skill is not loaded.
    fn unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.catalog
            .unload_skill(skill_name)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
    }

    /// Search for skills matching the given criteria.
    #[pyo3(signature = (query=None, tags=vec![], dcc=None))]
    fn find_skills(
        &self,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
    ) -> Vec<dcc_mcp_skills::SkillSummary> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.catalog.find_skills(query, &tag_refs, dcc)
    }

    /// List all skills with their load status.
    #[pyo3(signature = (status=None))]
    fn list_skills(&self, status: Option<&str>) -> Vec<dcc_mcp_skills::SkillSummary> {
        self.catalog.list_skills(status)
    }

    /// Get detailed info about a specific skill as a Python dict.
    ///
    /// Returns ``None`` if the skill is not found.
    fn get_skill_info(&self, py: Python<'_>, skill_name: &str) -> PyResult<Option<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        match self.catalog.get_skill_info(skill_name) {
            Some(info) => {
                let val = serde_json::to_value(&info)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(Some(json_value_to_pyobject(py, &val)?))
            }
            None => Ok(None),
        }
    }

    /// Check if a skill is loaded.
    fn is_loaded(&self, skill_name: &str) -> bool {
        self.catalog.is_loaded(skill_name)
    }

    /// Number of loaded skills.
    fn loaded_count(&self) -> usize {
        self.catalog.loaded_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "McpHttpServer(name={}, port={})",
            self.config.server_name, self.config.port
        )
    }
}

/// Register all Python classes in this module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyMcpHttpConfig>()?;
    m.add_class::<PyMcpHttpServer>()?;
    m.add_class::<PyServerHandle>()?;
    Ok(())
}
