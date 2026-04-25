//! MCP Streamable HTTP server for embedding in DCC software.

use super::*;

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
    pub(crate) registry: Arc<ActionRegistry>,
    pub(crate) dispatcher: Arc<ActionDispatcher>,
    pub(crate) catalog: Arc<SkillCatalog>,
    pub(crate) config: McpHttpConfig,
    pub(crate) runtime: Arc<Runtime>,
    /// Shared live metadata — written by Python via `update_scene()` /
    /// `update_gateway_metadata()`; propagated to FileRegistry each heartbeat.
    pub(crate) live_meta: Arc<RwLock<LiveMetaInner>>,
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
        let dispatcher = Arc::new(ActionDispatcher::new((*reg).clone()));
        // Wire the catalog to the same dispatcher so load_skill auto-registers handlers
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            reg.clone(),
            dispatcher.clone(),
        ));

        let live_meta = Arc::new(RwLock::new(LiveMetaInner {
            scene: cfg.scene.clone(),
            version: cfg.dcc_version.clone(),
            ..Default::default()
        }));
        Ok(Self {
            registry: reg,
            dispatcher,
            catalog,
            config: cfg,
            runtime: Arc::new(runtime),
            live_meta,
        })
    }

    /// Start the server and return a :class:`McpServerHandle`.
    ///
    /// This call returns immediately; the server runs in a background thread.
    fn start(&self) -> PyResult<PyServerHandle> {
        let server = McpHttpServer::with_catalog(
            self.registry.clone(),
            self.catalog.clone(),
            self.config.clone(),
        )
        .with_dispatcher(self.dispatcher.clone())
        .with_live_meta(self.live_meta.clone());
        let handle = self
            .runtime
            .block_on(server.start())
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let port = handle.port;
        let bind_addr = handle.bind_addr.clone();
        let is_gateway = handle.is_gateway;

        Ok(PyServerHandle {
            inner: Some(handle),
            runtime: self.runtime.clone(),
            port,
            bind_addr,
            is_gateway,
            live_meta: self.live_meta.clone(),
        })
    }

    /// Register a Python callable as the handler for ``action_name``.
    ///
    /// The callable receives a single argument: a dict of action parameters.
    /// It must return a JSON-serialisable value.
    ///
    /// Example::
    ///
    ///     server.register_handler("get_scene_info", lambda params: {"scene": "untitled"})
    ///
    /// Raises:
    ///     TypeError: If ``handler`` is not callable.
    #[pyo3(signature = (action_name, handler))]
    fn register_handler(
        &self,
        py: Python<'_>,
        action_name: &str,
        handler: Py<PyAny>,
    ) -> PyResult<()> {
        if !handler.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "handler must be callable",
            ));
        }
        // Store a Rust closure in the dispatcher that calls the Python callable.
        // The closure re-acquires the GIL via Python::attach (pyo3 0.28+)
        // and converts both params and return values through serde_json so the
        // Python-side contract matches ActionDispatcher: dict/list/scalars in,
        // JSON-serialisable values out.
        let handler_ref = handler.clone_ref(py);
        self.dispatcher
            .register_handler(action_name, move |params| {
                Python::attach(|gil| {
                    use dcc_mcp_utils::py_json::{json_value_to_bound_py, py_any_to_json_value};

                    let py_params = json_value_to_bound_py(gil, &params)
                        .map_err(|e| format!("failed to convert params: {e}"))?;
                    let raw = handler_ref
                        .call1(gil, (py_params,))
                        .map_err(|e| format!("handler error: {e}"))?;
                    py_any_to_json_value(raw.bind(gil)).map_err(|e| e.to_string())
                })
            });
        Ok(())
    }

    /// Return ``True`` if a handler is registered for ``action_name``.
    #[pyo3(signature = (action_name))]
    fn has_handler(&self, action_name: &str) -> bool {
        self.dispatcher.has_handler(action_name)
    }

    /// The server's :class:`ToolRegistry`.
    ///
    /// Returned value shares the underlying storage with the server —
    /// ``register()`` calls on it will update the tools exposed via
    /// ``tools/list``. Must be populated **before** calling :meth:`start`.
    #[getter]
    fn registry(&self) -> ActionRegistry {
        (*self.registry).clone()
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
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Unload a skill — removes its tools from the ActionRegistry.
    ///
    /// Returns the number of actions removed.
    /// Raises ``ValueError`` if the skill is not loaded.
    fn unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.catalog
            .unload_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// List all skills with their load status.
    #[pyo3(signature = (status=None))]
    fn list_skills(&self, py: Python<'_>, status: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        self.catalog
            .list_skills(status)
            .into_iter()
            .map(|s| {
                let val = serde_json::to_value(&s)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                json_value_to_pyobject(py, &val)
            })
            .collect::<PyResult<Vec<Py<PyAny>>>>()
    }

    /// Unified skill discovery — search by query, tags, DCC, scope, and/or limit.
    #[pyo3(signature = (query=None, tags=vec![], dcc=None, scope=None, limit=None))]
    fn search_skills(
        &self,
        py: Python<'_>,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
        scope: Option<&str>,
        limit: Option<usize>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let scope_enum = match scope {
            None => None,
            Some(s) => {
                let sc = match s.to_ascii_lowercase().as_str() {
                    "repo" => dcc_mcp_models::SkillScope::Repo,
                    "user" => dcc_mcp_models::SkillScope::User,
                    "system" => dcc_mcp_models::SkillScope::System,
                    "admin" => dcc_mcp_models::SkillScope::Admin,
                    _ => {
                        return Err(pyo3::exceptions::PyValueError::new_err(format!(
                            "invalid scope: {s:?} — expected one of: repo, user, system, admin"
                        )));
                    }
                };
                Some(sc)
            }
        };
        self.catalog
            .search_skills(query, &tag_refs, dcc, scope_enum, limit)
            .into_iter()
            .map(|s| {
                let val = serde_json::to_value(&s)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                json_value_to_pyobject(py, &val)
            })
            .collect::<PyResult<Vec<Py<PyAny>>>>()
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
