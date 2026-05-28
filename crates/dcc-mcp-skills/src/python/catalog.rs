//! PyO3 bindings for `SkillCatalog`.

use std::sync::Arc;

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::catalog::{SkillCatalog, SkillSummary, helpers};
use dcc_mcp_actions::EventBus;
use dcc_mcp_actions::registry::ToolRegistry;
use dcc_mcp_models::SkillMetadata;

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl SkillCatalog {
    #[new]
    fn py_new(registry: ToolRegistry) -> Self {
        Self::new(Arc::new(registry))
    }

    /// Return the catalog lifecycle event bus.
    #[pyo3(name = "event_bus")]
    fn py_event_bus(&self) -> EventBus {
        self.event_bus()
    }

    /// Activate a tool group (enable every action in it).
    ///
    /// Returns the number of actions whose ``enabled`` flag changed.
    #[pyo3(name = "activate_group")]
    fn py_activate_group(&self, group_name: &str) -> usize {
        self.activate_group(group_name)
    }

    /// Deactivate a tool group.
    #[pyo3(name = "deactivate_group")]
    fn py_deactivate_group(&self, group_name: &str) -> usize {
        self.deactivate_group(group_name)
    }

    /// Return the list of currently-active tool groups.
    #[pyo3(name = "active_groups")]
    fn py_active_groups(&self) -> Vec<String> {
        self.active_groups()
    }

    /// List all declared groups as ``(skill_name, group_name, active)`` tuples.
    #[pyo3(name = "list_groups")]
    fn py_list_groups(&self) -> Vec<(String, String, bool)> {
        self.list_groups()
    }

    /// Discover skills from standard scan paths.
    ///
    /// Args:
    ///     extra_paths: Additional directories to scan.
    ///     dcc_name: DCC name filter (e.g. "maya", "blender").
    ///
    /// Returns the number of newly discovered skills.
    #[pyo3(name = "discover")]
    #[pyo3(signature = (extra_paths=None, dcc_name=None))]
    fn py_discover(&self, extra_paths: Option<Vec<String>>, dcc_name: Option<&str>) -> usize {
        self.discover(extra_paths.as_deref(), dcc_name)
    }

    /// Register a Python callable as the in-process script executor.
    ///
    /// When registered, skill scripts are executed inside the **current**
    /// Python interpreter (the one running inside the DCC application) rather
    /// than being spawned as a subprocess.  This is the correct behaviour for
    /// Maya, Blender, Houdini, and any other DCC that embeds its own Python —
    /// the callable receives the script path and a params dict and must return
    /// a JSON-serialisable dict.
    ///
    /// The callable signature must be::
    ///
    ///     def executor(script_path: str, params: dict, *, action_name: str,
    ///                  skill_name: str | None, thread_affinity: str,
    ///                  execution: str, timeout_hint_secs: int | None) -> dict:
    ///         ...
    ///
    /// Example (Maya adapter)::
    ///
    ///     def _maya_exec(script_path: str, params: dict) -> dict:
    ///         import importlib.util, sys
    ///         spec = importlib.util.spec_from_file_location("_skill_script", script_path)
    ///         mod = importlib.util.module_from_spec(spec)
    ///         mod.__mcp_params__ = params
    ///         spec.loader.exec_module(mod)
    ///         return getattr(mod, "__mcp_result__", {"success": True})
    ///
    ///     catalog.set_in_process_executor(_maya_exec)
    ///
    /// Pass ``None`` to revert to subprocess execution.
    #[pyo3(name = "set_in_process_executor")]
    fn py_set_in_process_executor(&self, executor: Option<Py<PyAny>>) -> PyResult<()> {
        match executor {
            None => {
                self.clear_in_process_executor();
            }
            Some(py_fn) => {
                let executor_fn =
                    move |script_path: String,
                          params: serde_json::Value,
                          context: crate::catalog::execute::ScriptExecutionContext|
                          -> Result<serde_json::Value, String> {
                        use dcc_mcp_models::ExecutionMode;
                        use dcc_mcp_pybridge::py_json::{
                            json_value_to_pyobject, py_any_to_json_value,
                        };
                        use pyo3::types::PyDict;
                        Python::try_attach(|py| {
                            let py_params = json_value_to_pyobject(py, &params)
                                .map_err(|e| format!("params → Python: {e}"))?;
                            let kwargs = PyDict::new(py);
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
                            let result = match py_fn.call(py, args, Some(&kwargs)) {
                                Ok(value) => value,
                                Err(err)
                                    if err.is_instance_of::<pyo3::exceptions::PyTypeError>(py) =>
                                {
                                    drop(err);
                                    py_fn
                                        .call1(
                                            py,
                                            (
                                                script_path,
                                                json_value_to_pyobject(py, &params)
                                                    .map_err(|e| format!("params → Python: {e}"))?,
                                            ),
                                        )
                                        .map_err(|e| format!("in-process executor failed: {e}"))?
                                }
                                Err(err) => {
                                    return Err(format!("in-process executor failed: {err}"));
                                }
                            };
                            let bound = result.into_bound(py);
                            py_any_to_json_value(&bound).map_err(|e| format!("result → JSON: {e}"))
                        })
                        .ok_or_else(|| "Python interpreter not attached".to_string())
                        .and_then(|r| r)
                    };
                self.set_in_process_executor(executor_fn);
            }
        }
        Ok(())
    }

    /// Register a callable that can mutate or veto skill metadata before load.
    ///
    /// The callable receives a detached ``SkillMetadata`` object. It may mutate
    /// that object and return ``None``, or return a replacement
    /// ``SkillMetadata``. Raising an exception vetoes the load. The skill name
    /// must remain unchanged.
    #[pyo3(name = "set_skill_load_transform")]
    fn py_set_skill_load_transform(
        &self,
        py: Python<'_>,
        transform: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        match transform {
            None => self.clear_skill_load_transform(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "skill load transform must be callable",
                    ));
                }
                let transform_ref = py_fn.clone_ref(py);
                self.set_skill_load_transform(move |metadata| {
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
                            bound.extract::<SkillMetadata>().map_err(|e| {
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
    #[pyo3(name = "clear_skill_load_transform")]
    fn py_clear_skill_load_transform(&self) {
        self.clear_skill_load_transform();
    }

    /// Register a callable that observes successfully registered skill tools.
    ///
    /// The callable receives ``(skill_metadata, registered_actions)``. Errors
    /// are logged as lifecycle events but do not roll back the already-loaded
    /// skill; use ``set_skill_load_transform`` when policy must veto a load.
    #[pyo3(name = "set_after_load_skill_hook")]
    fn py_set_after_load_skill_hook(
        &self,
        py: Python<'_>,
        hook: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        match hook {
            None => self.clear_after_load_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-load skill hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.set_after_load_hook(move |metadata, registered| {
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
    #[pyo3(name = "clear_after_load_skill_hook")]
    fn py_clear_after_load_skill_hook(&self) {
        self.clear_after_load_hook();
    }

    /// Register a callable invoked after a skill is unloaded (#1405).
    ///
    /// The callable receives ``(skill_name, unregistered_actions)``. Used
    /// by the persistence layer to evict the row from the on-disk store.
    #[pyo3(name = "set_after_unload_skill_hook")]
    fn py_set_after_unload_skill_hook(
        &self,
        py: Python<'_>,
        hook: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        match hook {
            None => self.clear_after_unload_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-unload skill hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.set_after_unload_hook(move |skill_name, unregistered| {
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
    #[pyo3(name = "clear_after_unload_skill_hook")]
    fn py_clear_after_unload_skill_hook(&self) {
        self.clear_after_unload_hook();
    }

    /// Register a callable invoked after a tool group is activated or
    /// deactivated (#1405). Receives ``(group_name, activated: bool)``.
    #[pyo3(name = "set_after_group_change_hook")]
    fn py_set_after_group_change_hook(
        &self,
        py: Python<'_>,
        hook: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        match hook {
            None => self.clear_after_group_change_hook(),
            Some(py_fn) => {
                if !py_fn.bind(py).is_callable() {
                    return Err(pyo3::exceptions::PyTypeError::new_err(
                        "after-group-change hook must be callable",
                    ));
                }
                let hook_ref = py_fn.clone_ref(py);
                self.set_after_group_change_hook(move |group_name, activated| {
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
    #[pyo3(name = "clear_after_group_change_hook")]
    fn py_clear_after_group_change_hook(&self) {
        self.clear_after_group_change_hook();
    }

    /// Replay a persisted set of loaded skills + active groups (#1405).
    ///
    /// ``state_json`` is the JSON-encoded ``PersistedCatalogState`` (the
    /// shape returned by ``LoadedStateStore.snapshot().to_json()`` plus
    /// ``json.dumps``).
    ///
    /// ``policy`` is one of ``"skip_on_drift"`` (default),
    /// ``"require_exact_version"``, or ``"ignore_version"``.
    ///
    /// Returns the ``ReplayReport`` as a JSON-encoded string so callers
    /// can parse it with ``json.loads`` without an extra Python
    /// conversion dependency.
    #[pyo3(name = "replay_loaded")]
    #[pyo3(signature = (state_json, policy = "skip_on_drift"))]
    fn py_replay_loaded(&self, state_json: &str, policy: &str) -> PyResult<String> {
        let parsed_state: crate::catalog::persistence::PersistedCatalogState =
            serde_json::from_str(state_json).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("invalid state JSON: {e}"))
            })?;
        let parsed_policy: crate::catalog::persistence::LoadReplayPolicy = match policy {
            "skip_on_drift" => crate::catalog::persistence::LoadReplayPolicy::SkipOnDrift,
            "require_exact_version" => {
                crate::catalog::persistence::LoadReplayPolicy::RequireExactVersion
            }
            "ignore_version" => crate::catalog::persistence::LoadReplayPolicy::IgnoreVersion,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "unknown LoadReplayPolicy: {other}"
                )));
            }
        };
        let report = self.replay_loaded(&parsed_state, parsed_policy);
        serde_json::to_string(&report).map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("serialise report: {e}"))
        })
    }

    /// Load a skill by name — registers its tools.
    ///
    /// Returns a list of registered action names.
    /// Raises ValueError if the skill is not found.
    #[pyo3(name = "load_skill")]
    fn py_load_skill(&self, skill_name: &str) -> PyResult<Vec<String>> {
        self.load_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Return a detached, mutable skill metadata object.
    ///
    /// Mutating the returned object does not affect catalog state until it is
    /// passed back to ``load_skill_object``.
    #[pyo3(name = "get_skill")]
    fn py_get_skill(&self, skill_name: &str) -> Option<SkillMetadata> {
        self.get_skill(skill_name)
    }

    /// Load a caller-supplied skill metadata object through core registration.
    ///
    /// This lets adapters adjust tool declarations at runtime without parsing
    /// or rewriting ``SKILL.md`` / ``tools.yaml`` sidecar files.
    #[pyo3(name = "load_skill_object")]
    fn py_load_skill_object(&self, metadata: SkillMetadata) -> PyResult<Vec<String>> {
        self.load_skill_object(metadata)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Unload a skill — removes its tools from the registry.
    ///
    /// Returns the number of actions removed.
    /// Raises ValueError if the skill is not loaded.
    #[pyo3(name = "unload_skill")]
    fn py_unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.unload_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Unified skill discovery (issue #340).
    ///
    /// Optional ``scope`` (str: "repo" |
    /// "user" | "system" | "admin") and ``limit``. Empty ``query`` with no
    /// other filters returns the top ``limit`` skills ordered by scope
    /// precedence (Admin > System > User > Repo) then name.
    #[pyo3(name = "search_skills")]
    #[pyo3(signature = (query=None, tags=vec![], dcc=None, scope=None, limit=None))]
    fn py_search_skills(
        &self,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
        scope: Option<&str>,
        limit: Option<usize>,
    ) -> PyResult<Vec<SkillSummary>> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let scope_enum = match scope {
            None => None,
            Some(s) => {
                Some(helpers::parse_scope_str(s).map_err(pyo3::exceptions::PyValueError::new_err)?)
            }
        };
        Ok(self.search_skills(query, &tag_refs, dcc, scope_enum, limit))
    }

    /// List all skills with their load status.
    #[pyo3(name = "list_skills")]
    #[pyo3(signature = (status=None))]
    fn py_list_skills(&self, status: Option<&str>) -> Vec<SkillSummary> {
        self.list_skills(status)
    }

    /// Get detailed info about a specific skill.
    ///
    /// Returns None if the skill is not found. The detail is returned as a
    /// Python dict (serialized via serde_json).
    #[pyo3(name = "get_skill_info")]
    fn py_get_skill_info(&self, py: Python<'_>, skill_name: &str) -> PyResult<Option<Py<PyAny>>> {
        use dcc_mcp_pybridge::py_json::json_value_to_pyobject;
        match self.get_skill_info(skill_name) {
            Some(info) => {
                let val = serde_json::to_value(&info)
                    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
                Ok(Some(json_value_to_pyobject(py, &val)?))
            }
            None => Ok(None),
        }
    }

    /// Number of skills in the catalog.
    fn __len__(&self) -> usize {
        self.len()
    }

    /// Whether the catalog is empty.
    fn __bool__(&self) -> bool {
        !self.is_empty()
    }

    /// Check if a skill is loaded.
    #[pyo3(name = "is_loaded")]
    fn py_is_loaded(&self, skill_name: &str) -> bool {
        self.is_loaded(skill_name)
    }

    /// Number of loaded skills.
    #[pyo3(name = "loaded_count")]
    fn py_loaded_count(&self) -> usize {
        self.loaded_count()
    }

    fn __repr__(&self) -> String {
        format!(
            "SkillCatalog(total={}, loaded={})",
            self.len(),
            self.loaded_count()
        )
    }
}
