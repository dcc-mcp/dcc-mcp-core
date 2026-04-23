use super::*;

#[pymethods]
impl SkillCatalog {
    #[new]
    fn py_new(registry: ActionRegistry) -> Self {
        Self::new(Arc::new(registry))
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
    ///     def executor(script_path: str, params: dict) -> dict:
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
    fn py_set_in_process_executor(&mut self, executor: Option<Py<PyAny>>) -> PyResult<()> {
        match executor {
            None => {
                self.script_executor = None;
            }
            Some(py_fn) => {
                let executor_fn = move |script_path: String,
                                        params: serde_json::Value|
                      -> Result<serde_json::Value, String> {
                    use dcc_mcp_utils::py_json::{json_value_to_pyobject, py_any_to_json_value};
                    Python::try_attach(|py| {
                        let py_params = json_value_to_pyobject(py, &params)
                            .map_err(|e| format!("params → Python: {e}"))?;
                        let result = py_fn
                            .call1(py, (script_path, py_params))
                            .map_err(|e| format!("in-process executor failed: {e}"))?;
                        let bound = result.into_bound(py);
                        py_any_to_json_value(&bound).map_err(|e| format!("result → JSON: {e}"))
                    })
                    .ok_or_else(|| "Python interpreter not attached".to_string())
                    .and_then(|r| r)
                };
                self.script_executor = Some(Arc::new(executor_fn));
            }
        }
        Ok(())
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

    /// Unload a skill — removes its tools from the registry.
    ///
    /// Returns the number of actions removed.
    /// Raises ValueError if the skill is not loaded.
    #[pyo3(name = "unload_skill")]
    fn py_unload_skill(&self, skill_name: &str) -> PyResult<usize> {
        self.unload_skill(skill_name)
            .map_err(pyo3::exceptions::PyValueError::new_err)
    }

    /// Search for skills matching criteria.
    #[pyo3(name = "find_skills")]
    #[pyo3(signature = (query=None, tags=vec![], dcc=None))]
    fn py_find_skills(
        &self,
        query: Option<&str>,
        tags: Vec<String>,
        dcc: Option<&str>,
    ) -> Vec<SkillSummary> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.find_skills(query, &tag_refs, dcc)
    }

    /// Unified skill discovery (issue #340).
    ///
    /// Superset of ``find_skills`` with optional ``scope`` (str: "repo" |
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
        use dcc_mcp_utils::py_json::json_value_to_pyobject;
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
