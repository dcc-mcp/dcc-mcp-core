//! MCP Streamable HTTP server for embedding in DCC software.

use super::*;

use std::sync::Arc;

use dcc_mcp_host::DccDispatcher;

/// MCP Streamable HTTP server for embedding in DCC software.
///
/// Example::
///
///     from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig
///
///     registry = ToolRegistry()
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
    pub(crate) registry: Arc<ToolRegistry>,
    pub(crate) dispatcher: Arc<ToolDispatcher>,
    pub(crate) catalog: Arc<SkillCatalog>,
    pub(crate) config: McpHttpConfig,
    pub(crate) runtime: Arc<Runtime>,
    /// Shared live metadata — written by Python via `update_scene()` /
    /// `update_gateway_metadata()`; propagated to FileRegistry each heartbeat.
    pub(crate) live_meta: Arc<RwLock<LiveMetaInner>>,
    /// Shared [`dcc_mcp_http::resources::ResourceRegistry`] (issue #730).
    ///
    /// Built at construction time using the same
    /// [`dcc_mcp_http::server::build_resource_registry`] the server would
    /// have used internally, so `server.resources()` returns the same
    /// registry that backs `/mcp` both before and after `start()`.
    pub(crate) resources: dcc_mcp_http::resources::ResourceRegistry,
    /// Optional DCC main-thread dispatcher attached via
    /// [`PyMcpHttpServer::attach_dispatcher`]. Each [`PyMcpHttpServer::start`]
    /// builds a fresh [`dcc_mcp_http::executor::DccExecutorHandle`] from this
    /// so gateway promotion (`start` after `shutdown`) keeps routing
    /// `tools/call` through the host queue.
    pub(crate) attached_dispatcher: parking_lot::Mutex<Option<Arc<dyn DccDispatcher>>>,
    /// Optional shared [`ReadinessProbe`] installed via
    /// [`PyMcpHttpServer::set_readiness_probe`] (issue #714). When
    /// present, it is wired into both the MCP `tools/call` gate and
    /// the REST `POST /v1/call` handler, so adapters only need to
    /// flip the bits on this one instance.
    pub(crate) readiness_probe:
        parking_lot::Mutex<Option<Arc<dyn dcc_mcp_skill_rest::ReadinessProbe>>>,
    /// Shared [`dcc_mcp_http::prompts::PromptRegistry`] (issue #792).
    ///
    /// Built at construction time using the same `PromptRegistry::new`
    /// the server would use internally, so `server.prompts()` returns
    /// the same registry that backs `/mcp` both before and after
    /// `start()`.
    pub(crate) prompts: dcc_mcp_http::prompts::PromptRegistry,
}

#[pymethods]
impl PyMcpHttpServer {
    /// Create a new MCP HTTP server.
    ///
    /// Args:
    ///     registry: An ``ToolRegistry`` with registered DCC actions.
    ///     config: A ``McpHttpConfig``. If omitted, defaults to port 8765.
    #[new]
    #[pyo3(signature = (registry, config=None))]
    fn new(registry: &ToolRegistry, config: Option<&PyMcpHttpConfig>) -> PyResult<Self> {
        let cfg = config.map(|c| c.inner.clone()).unwrap_or_default();

        let runtime = super::build_python_runtime()?;

        let reg = Arc::new(registry.clone());
        let dispatcher = Arc::new(ToolDispatcher::new((*reg).clone()));
        // Wire the catalog to the same dispatcher so load_skill auto-registers handlers
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            reg.clone(),
            dispatcher.clone(),
        ));

        let live_meta = Arc::new(RwLock::new(LiveMetaInner {
            scene: cfg.scene().clone(),
            version: cfg.dcc_version().clone(),
            ..Default::default()
        }));
        // Issue #730 — build the ResourceRegistry up-front and share it
        // between this Python handle and the inner McpHttpServer when
        // start() runs. Using the canonical `build_resource_registry`
        // keeps artefact-store wiring consistent with the Rust path.
        let resources = dcc_mcp_http::server::build_resource_registry(&cfg);
        // Issue #792 — build the PromptRegistry up-front and share it
        // between this Python handle and the inner McpHttpServer when
        // start() runs.
        let prompts = dcc_mcp_http::prompts::PromptRegistry::new(cfg.features.enable_prompts);
        Ok(Self {
            registry: reg,
            dispatcher,
            catalog,
            config: cfg,
            runtime: Arc::new(runtime),
            live_meta,
            resources,
            prompts,
            attached_dispatcher: parking_lot::Mutex::new(None),
            readiness_probe: parking_lot::Mutex::new(None),
        })
    }

    /// Route every ``tools/call`` through the given dispatcher's main-thread queue.
    ///
    /// ``dispatcher`` must be a :class:`~dcc_mcp_core.host.QueueDispatcher`
    /// or :class:`~dcc_mcp_core.host.BlockingDispatcher`. Once attached,
    /// every synchronous ``tools/call`` handler runs on the thread that
    /// drains the dispatcher (typically the DCC main thread, or the
    /// :class:`~dcc_mcp_core.host.StandaloneHost` driver thread in tests).
    ///
    /// Must be called **before** the first :meth:`start`. Re-attaching after
    /// the server has started is rejected with :class:`RuntimeError` — hot
    /// swap is out of scope for this API and belongs to a dedicated
    /// lifecycle method we may add later.
    ///
    /// Args:
    ///     dispatcher: a ``QueueDispatcher`` or ``BlockingDispatcher``.
    ///
    /// Raises:
    ///     TypeError: dispatcher is not one of the supported types.
    ///     RuntimeError: ``attach_dispatcher`` was already called once
    ///         on this server. Build a fresh ``McpHttpServer`` to swap
    ///         the backing dispatcher.
    fn attach_dispatcher(&self, py: Python<'_>, dispatcher: Py<PyAny>) -> PyResult<()> {
        use dcc_mcp_host::python::{PyBlockingDispatcher, PyQueueDispatcher};

        let bound = dispatcher.bind(py);
        let shared: Arc<dyn dcc_mcp_host::DccDispatcher> =
            if let Ok(queue) = bound.cast::<PyQueueDispatcher>() {
                queue.borrow().arc_inner()
            } else if let Ok(blocking) = bound.cast::<PyBlockingDispatcher>() {
                blocking.borrow().arc_inner()
            } else {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "attach_dispatcher expects a QueueDispatcher or BlockingDispatcher",
                ));
            };

        let mut slot = self.attached_dispatcher.lock();
        if slot.is_some() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "attach_dispatcher was already called on this McpHttpServer — \
                 build a fresh server to swap dispatchers",
            ));
        }
        *slot = Some(shared);
        tracing::info!(
            "McpHttpServer: main-thread dispatcher attached — tools/call will \
             route through DccDispatcher::post"
        );
        Ok(())
    }

    /// Start the server and return a :class:`McpServerHandle`.
    ///
    /// This call returns immediately; the server runs in a background thread.
    fn start(&self) -> PyResult<PyServerHandle> {
        let mut server = McpHttpServer::with_catalog(
            self.registry.clone(),
            self.catalog.clone(),
            self.config.clone(),
        )
        .with_dispatcher(self.dispatcher.clone())
        .with_live_meta(self.live_meta.clone())
        .with_resources(self.resources.clone())
        .with_prompts(self.prompts.clone());
        // Rebuild the host-bridge executor on every start so gateway
        // promotion (second `start()` after `shutdown`) still wires
        // `with_executor` — the previous handle dropped the old bridge task.
        if let Some(dispatcher) = self.attached_dispatcher.lock().as_ref().cloned() {
            let depth = self.config.bridge_queue_depth();
            let bridge_runtime = self.runtime.handle();
            let executor = dcc_mcp_http::host_bridge::dispatcher_to_executor_handle_with_capacity(
                dispatcher,
                bridge_runtime,
                depth,
            );
            server = server
                .with_executor(executor)
                .with_host_bridge_runtime(bridge_runtime.clone());
        }
        // Issue #714 — propagate the shared readiness probe into the
        // Rust server so both `/mcp` and `/v1/call` consult it.
        if let Some(probe) = self.readiness_probe.lock().as_ref().cloned() {
            server = server.with_readiness(probe);
        }
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
            shutdown_on_drop: self.config.shutdown_on_drop(),
        })
    }

    /// Install a shared :class:`ReadinessProbe` that gates DCC-touching
    /// ``tools/call`` and ``POST /v1/call`` dispatches (issue #714).
    ///
    /// Call this **before** :meth:`start`. The same probe instance
    /// backs both the MCP and REST surfaces, so a single
    /// ``probe.set_dispatcher_ready(True); probe.set_dcc_ready(True)``
    /// from the DCC adapter's boot-complete hook flips base routing
    /// readiness for every surface at once. Adapters that support
    /// main-thread tools should also flip the host execution bridge and
    /// main-thread executor bits once those paths are usable.
    ///
    /// Without a probe installed, the server defaults to the legacy
    /// fully-ready behaviour — tests and standalone servers are
    /// unaffected.
    ///
    /// Args:
    ///     probe: A :class:`dcc_mcp_core.ReadinessProbe` instance.
    #[pyo3(signature = (probe))]
    fn set_readiness_probe(&self, probe: PyRef<'_, super::PyReadinessProbe>) -> PyResult<()> {
        *self.readiness_probe.lock() = Some(probe.as_dyn());
        tracing::info!(
            "McpHttpServer: readiness probe installed — /mcp and /v1/call \
             will share it (issue #714)"
        );
        Ok(())
    }

    /// Register a Python callable as the handler for ``action_name``.
    ///
    /// The callable receives a single argument: a dict of action parameters.
    /// It must return a JSON-serialisable value.
    ///
    /// Args:
    ///     action_name: The MCP tool name.
    ///     handler: The Python callable.
    ///     thread_affinity: Optional routing hint — ``"any"`` (default)
    ///         runs the handler on a Tokio worker via ``spawn_blocking``;
    ///         ``"main"`` routes it through the attached
    ///         :class:`~dcc_mcp_core.host.DccDispatcher` so it executes
    ///         on the DCC main thread (issue #716). If the action is
    ///         already registered in the backing :class:`ToolRegistry`,
    ///         the existing ``ToolMeta.thread_affinity`` is overwritten.
    ///         If no ``ToolMeta`` exists yet, the kwarg is recorded as
    ///         a best-effort — register the action first via
    ///         ``ToolRegistry.register(...)`` or let ``load_skill()``
    ///         create it.
    ///
    /// Example::
    ///
    ///     server.register_handler("get_scene_info", lambda params: {"scene": "untitled"})
    ///     server.register_handler("bake_lighting", bake_fn, thread_affinity="main")
    ///
    /// Raises:
    ///     TypeError: If ``handler`` is not callable.
    ///     ValueError: If ``thread_affinity`` is not ``"any"`` or
    ///         ``"main"`` (case-insensitive).
    #[pyo3(signature = (action_name, handler, thread_affinity=None))]
    fn register_handler(
        &self,
        py: Python<'_>,
        action_name: &str,
        handler: Py<PyAny>,
        thread_affinity: Option<&str>,
    ) -> PyResult<()> {
        if !handler.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "handler must be callable",
            ));
        }

        // If the caller asked for a specific affinity, patch the existing
        // ToolMeta in the registry so the sync `tools/call` path (#716)
        // routes accordingly. Parse up front so an invalid string surfaces
        // before any Python-side state is mutated.
        if let Some(affinity_str) = thread_affinity {
            let parsed = dcc_mcp_models::ThreadAffinity::parse(affinity_str).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "thread_affinity must be 'any' or 'main' (got {affinity_str:?})"
                ))
            })?;
            // Fetch-patch-reregister: `register_action` is an upsert and
            // takes an owned `ToolMeta`, so cloning is the simplest way
            // to mutate a single field without racing concurrent writers.
            // If the action isn't registered yet, we silently skip —
            // the handler itself does not belong in the action registry,
            // and `load_skill()` / `ToolRegistry.register()` will install
            // an `ToolMeta` with the correct affinity at the right moment.
            if let Some(mut meta) = self.registry.get_action(action_name, None) {
                meta.thread_affinity = parsed;
                self.registry.register_action(meta);
            } else {
                tracing::debug!(
                    action = action_name,
                    affinity = %parsed,
                    "register_handler: no ToolMeta yet — affinity kwarg recorded as best-effort"
                );
            }
        }

        // Store a Rust closure in the dispatcher that calls the Python callable.
        // The closure re-acquires the GIL via Python::attach (pyo3 0.28+)
        // and converts both params and return values through serde_json so the
        // Python-side contract matches ToolDispatcher: dict/list/scalars in,
        // JSON-serialisable values out.
        let handler_ref = handler.clone_ref(py);
        self.dispatcher
            .register_handler(action_name, move |params| {
                Python::attach(|gil| {
                    use dcc_mcp_pybridge::py_json::{json_value_to_bound_py, py_any_to_json_value};

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

    /// Register a Python callable as the **in-process** script executor.
    ///
    /// This is the recommended way to wire DCC-specific execution into the
    /// Skills-First workflow (issues #464, #465).  Call this **before** any
    /// ``load_skill()`` calls so that all skill handlers are registered with
    /// the in-process path from the start, eliminating the timing race that
    /// occurred when handlers were overridden one-by-one after loading.
    ///
    /// The callable receives two positional arguments plus execution metadata:
    ///
    /// - ``script_path`` (``str``) — absolute path to the skill's ``.py`` file.
    /// - ``params`` (``dict``) — tool input parameters.
    ///
    /// - ``action_name`` (kw-only ``str``) — registered MCP tool name.
    /// - ``skill_name`` (kw-only ``str | None``) — owning skill name.
    /// - ``thread_affinity`` (kw-only ``"main" | "any"``).
    /// - ``execution`` (kw-only ``"sync" | "async"``).
    /// - ``timeout_hint_secs`` (kw-only ``int | None``).
    ///
    /// It must return a JSON-serialisable value (``dict``, ``list``, scalar…).
    /// Legacy two-argument callables remain supported.
    ///
    /// When called, ``load_skill()`` will register **in-process** handlers for
    /// every tool in the loaded skill instead of spawning subprocesses.
    ///
    /// Example::
    ///
    ///     import runpy
    ///
    ///     def my_executor(script_path, params):
    ///         ns = runpy.run_path(script_path, init_globals={"params": params})
    ///         return ns.get("result", {})
    ///
    ///     server = create_skill_server("maya")
    ///     server.set_in_process_executor(my_executor)
    ///     server.load_skill("maya-scene")   # handlers: in-process ✓
    ///
    /// Raises:
    ///     TypeError: If ``executor`` is not callable.
    #[pyo3(signature = (executor))]
    fn set_in_process_executor(&self, py: Python<'_>, executor: Py<PyAny>) -> PyResult<()> {
        if !executor.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "executor must be callable",
            ));
        }
        let executor_ref = executor.clone_ref(py);
        self.catalog
            .set_in_process_executor(move |script_path, params, context| {
                Python::attach(|gil| {
                    use dcc_mcp_models::ExecutionMode;
                    use dcc_mcp_pybridge::py_json::{json_value_to_bound_py, py_any_to_json_value};
                    use pyo3::types::PyDict;

                    let py_params = json_value_to_bound_py(gil, &params)
                        .map_err(|e| format!("failed to convert params: {e}"))?;
                    let kwargs = PyDict::new(gil);
                    kwargs
                        .set_item("action_name", &context.action_name)
                        .map_err(|e| format!("executor kwargs: {e}"))?;
                    kwargs
                        .set_item("skill_name", context.skill_name.as_deref())
                        .map_err(|e| format!("executor kwargs: {e}"))?;
                    kwargs
                        .set_item("thread_affinity", context.thread_affinity.as_str())
                        .map_err(|e| format!("executor kwargs: {e}"))?;
                    kwargs
                        .set_item(
                            "execution",
                            match context.execution {
                                ExecutionMode::Sync => "sync",
                                ExecutionMode::Async => "async",
                            },
                        )
                        .map_err(|e| format!("executor kwargs: {e}"))?;
                    kwargs
                        .set_item("timeout_hint_secs", context.timeout_hint_secs)
                        .map_err(|e| format!("executor kwargs: {e}"))?;
                    let args = (script_path.as_str(), py_params);
                    let raw = match executor_ref.call(gil, args, Some(&kwargs)) {
                        Ok(value) => value,
                        Err(err) if err.is_instance_of::<pyo3::exceptions::PyTypeError>(gil) => {
                            drop(err);
                            let py_params = json_value_to_bound_py(gil, &params)
                                .map_err(|e| format!("failed to convert params: {e}"))?;
                            executor_ref
                                .call1(gil, (script_path, py_params))
                                .map_err(|e| format!("executor error: {e}"))?
                        }
                        Err(err) => return Err(format!("executor error: {err}")),
                    };
                    py_any_to_json_value(raw.bind(gil)).map_err(|e| e.to_string())
                })
            });
        tracing::info!(
            "McpHttpServer: in-process executor registered — \
             load_skill() will use in-process handlers (issue #464)"
        );
        Ok(())
    }

    /// Remove the in-process executor, reverting future ``load_skill()`` calls
    /// to subprocess execution.
    ///
    /// Already-loaded skills retain their existing handlers; call
    /// ``unload_skill()`` and ``load_skill()`` again to switch them.
    fn clear_in_process_executor(&self) {
        self.catalog.clear_in_process_executor();
    }

    /// Register a Python callable that can mutate or veto skill metadata before load.
    ///
    /// The callable receives a detached ``SkillMetadata`` object. It may mutate
    /// that object and return ``None``, or return a replacement
    /// ``SkillMetadata``. The transform runs for direct Python
    /// ``load_skill()``, MCP ``tools/call load_skill``, and REST
    /// ``POST /v1/load_skill`` because all paths share this server's
    /// ``SkillCatalog``.
    #[pyo3(signature = (transform))]
    fn set_skill_load_transform(
        &self,
        py: Python<'_>,
        transform: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        match transform {
            None => self.catalog.clear_skill_load_transform(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "skill load transform must be callable",
                    ));
                }
                let transform_ref = py_fn.clone_ref(py);
                self.catalog.set_skill_load_transform(move |metadata| {
                    Python::try_attach(|gil| {
                        let py_metadata = Py::new(gil, metadata)
                            .map_err(|e| format!("metadata to Python: {e}"))?;
                        let result = transform_ref
                            .call1(gil, (py_metadata.clone_ref(gil),))
                            .map_err(|e| format!("skill load transform failed: {e}"))?;
                        let bound = result.bind(gil);
                        if bound.is_none() {
                            Ok(py_metadata.borrow(gil).clone())
                        } else {
                            bound
                                .extract::<dcc_mcp_models::SkillMetadata>()
                                .map_err(|e| {
                                    format!("skill load transform returned invalid metadata: {e}")
                                })
                        }
                    })
                    .ok_or_else(|| "Python interpreter not attached".to_string())
                    .and_then(|r| r)
                });
            }
        }
        Ok(())
    }

    /// Clear any registered skill-load transform.
    fn clear_skill_load_transform(&self) {
        self.catalog.clear_skill_load_transform();
    }

    /// Register a Python callable that observes a successful skill load.
    ///
    /// The callable receives ``(skill_metadata, registered_actions)``. Errors
    /// are logged through the catalog lifecycle event stream but do not undo an
    /// otherwise successful load.
    #[pyo3(signature = (hook))]
    fn set_after_load_skill_hook(&self, py: Python<'_>, hook: Option<Py<PyAny>>) -> PyResult<()> {
        match hook {
            None => self.catalog.clear_after_load_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-load skill hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.catalog
                    .set_after_load_hook(move |metadata, registered| {
                        Python::try_attach(|gil| {
                            let py_metadata = Py::new(gil, metadata.clone())
                                .map_err(|e| format!("metadata to Python: {e}"))?;
                            hook_ref
                                .call1(gil, (py_metadata, registered.to_vec()))
                                .map(|_| ())
                                .map_err(|e| format!("after-load skill hook failed: {e}"))
                        })
                        .ok_or_else(|| "Python interpreter not attached".to_string())
                        .and_then(|r| r)
                    });
            }
        }
        Ok(())
    }

    /// Clear any registered after-load skill hook.
    fn clear_after_load_skill_hook(&self) {
        self.catalog.clear_after_load_hook();
    }

    /// Register an after-unload observer (#1405). Receives
    /// ``(skill_name, unregistered_actions)``.
    #[pyo3(signature = (hook))]
    fn set_after_unload_skill_hook(&self, py: Python<'_>, hook: Option<Py<PyAny>>) -> PyResult<()> {
        match hook {
            None => self.catalog.clear_after_unload_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-unload skill hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.catalog
                    .set_after_unload_hook(move |skill_name, unregistered| {
                        Python::try_attach(|gil| {
                            hook_ref
                                .call1(gil, (skill_name.to_string(), unregistered.to_vec()))
                                .map(|_| ())
                                .map_err(|e| format!("after-unload skill hook failed: {e}"))
                        })
                        .ok_or_else(|| "Python interpreter not attached".to_string())
                        .and_then(|r| r)
                    });
            }
        }
        Ok(())
    }

    /// Clear any registered after-unload skill hook.
    fn clear_after_unload_skill_hook(&self) {
        self.catalog.clear_after_unload_hook();
    }

    /// Register an after-group-change observer (#1405). Receives
    /// ``(group_name, activated: bool)``.
    #[pyo3(signature = (hook))]
    fn set_after_group_change_hook(&self, py: Python<'_>, hook: Option<Py<PyAny>>) -> PyResult<()> {
        match hook {
            None => self.catalog.clear_after_group_change_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-group-change hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.catalog
                    .set_after_group_change_hook(move |group_name, activated| {
                        Python::try_attach(|gil| {
                            hook_ref
                                .call1(gil, (group_name.to_string(), activated))
                                .map(|_| ())
                                .map_err(|e| format!("after-group-change hook failed: {e}"))
                        })
                        .ok_or_else(|| "Python interpreter not attached".to_string())
                        .and_then(|r| r)
                    });
            }
        }
        Ok(())
    }

    /// Clear any registered after-group-change hook.
    fn clear_after_group_change_hook(&self) {
        self.catalog.clear_after_group_change_hook();
    }

    /// Replay a persisted state on the inner catalog (#1405).
    ///
    /// ``state_json`` is the JSON-encoded ``PersistedCatalogState``;
    /// ``policy`` is ``"skip_on_drift"`` / ``"require_exact_version"``
    /// / ``"ignore_version"``. Returns the ``ReplayReport`` as JSON.
    #[pyo3(signature = (state_json, policy = "skip_on_drift"))]
    fn replay_loaded_skills(&self, state_json: &str, policy: &str) -> PyResult<String> {
        let parsed_state: dcc_mcp_skills::catalog::persistence::PersistedCatalogState =
            serde_json::from_str(state_json).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid state JSON: {e}"))
            })?;
        let parsed_policy: dcc_mcp_skills::catalog::persistence::LoadReplayPolicy = match policy {
            "skip_on_drift" => dcc_mcp_skills::catalog::persistence::LoadReplayPolicy::SkipOnDrift,
            "require_exact_version" => {
                dcc_mcp_skills::catalog::persistence::LoadReplayPolicy::RequireExactVersion
            }
            "ignore_version" => {
                dcc_mcp_skills::catalog::persistence::LoadReplayPolicy::IgnoreVersion
            }
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "unknown LoadReplayPolicy: {other}"
                )));
            }
        };
        let report = self.catalog.replay_loaded(&parsed_state, parsed_policy);
        serde_json::to_string(&report).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("serialise report: {e}"))
        })
    }

    /// The server's :class:`ToolRegistry`.
    ///
    /// Returned value shares the underlying storage with the server —
    /// ``register()`` calls on it will update the tools exposed via
    /// ``tools/list``. Must be populated **before** calling :meth:`start`.
    #[getter]
    fn registry(&self) -> ToolRegistry {
        (*self.registry).clone()
    }

    /// Access the server's :class:`ResourceHandle` for pushing scene
    /// snapshots, registering custom producers, and wiring output
    /// buffers (issue #730).
    ///
    /// The returned handle is a thin wrapper around the shared
    /// [`dcc_mcp_http::resources::ResourceRegistry`]: mutations take effect
    /// immediately and are reflected in ``resources/list`` /
    /// ``resources/read`` both before and after :meth:`start`.
    ///
    /// Example::
    ///
    ///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
    ///     server.resources().set_scene({"nodes": []})
    ///     server.resources().register_producer(
    ///         "maya-cmds://",
    ///         lambda uri: {"mimeType": "text/plain", "text": "ls -l"},
    ///     )
    ///     handle = server.start()
    fn resources(&self) -> super::resources_handle::PyResourceHandle {
        super::resources_handle::PyResourceHandle::new(self.resources.clone())
    }

    /// Access the server's prompt registry (issue #792).
    ///
    /// Returns a handle that can register, unregister, or clear
    /// prompts programmatically from Python. Registered prompts
    /// appear in ``prompts/list`` and can be rendered via
    /// ``prompts/get``.
    ///
    /// Example::
    ///
    ///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
    ///     server.prompts().register_prompt(
    ///         name="bake_animation",
    ///         template="Bake from {{start}} to {{end}}",
    ///         description="Bake animation",
    ///         arguments=[{"name": "start", "required": True},
    ///                    {"name": "end",   "required": True}],
    ///     )
    ///     handle = server.start()
    ///     # prompts/list now includes "bake_animation"
    fn prompts(&self) -> super::prompts_handle::PyPromptHandle {
        super::prompts_handle::PyPromptHandle::new(self.prompts.clone())
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

    /// Load a skill by name — registers its tools in the ToolRegistry.
    ///
    /// Returns the list of registered action names.
    /// Raises ``ValueError`` if the skill is not found.
    fn load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.catalog
            .load_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Return a detached, mutable skill metadata object.
    ///
    /// Mutating the returned object does not affect catalog state until it is
    /// passed back to ``load_skill_object``.
    fn get_skill(&self, skill_name: &str) -> Option<dcc_mcp_models::SkillMetadata> {
        self.catalog.get_skill(skill_name)
    }

    /// Load a caller-supplied skill metadata object through core registration.
    ///
    /// Adapters can use this to adjust tool declarations at runtime without
    /// rewriting ``SKILL.md`` / ``tools.yaml`` files.
    fn load_skill_object(&self, metadata: dcc_mcp_models::SkillMetadata) -> PyResult<Vec<String>> {
        self.catalog
            .load_skill_object(metadata)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Unload a skill — removes its tools from the ToolRegistry.
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
        use dcc_mcp_pybridge::py_json::json_value_to_pyobject;
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
        use dcc_mcp_pybridge::py_json::json_value_to_pyobject;
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let scope_enum = match scope {
            None => None,
            Some(s) => {
                let sc = match s.to_ascii_lowercase().as_str() {
                    "repo" => dcc_mcp_models::SkillScope::Repo,
                    "user" => dcc_mcp_models::SkillScope::User,
                    "team" => dcc_mcp_models::SkillScope::Team,
                    "system" => dcc_mcp_models::SkillScope::System,
                    "admin" => dcc_mcp_models::SkillScope::Admin,
                    _ => {
                        return Err(pyo3::exceptions::PyValueError::new_err(format!(
                            "invalid scope: {s:?} — expected one of: repo, user, team, system, admin"
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
        use dcc_mcp_pybridge::py_json::json_value_to_pyobject;
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
            self.config.server_name(),
            self.config.port()
        )
    }
}
