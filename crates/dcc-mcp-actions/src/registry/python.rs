use super::*;
use dcc_mcp_models::{ExecutionMode, NextTools, ThreadAffinity, ToolAnnotations};

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl ActionRegistry {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[pyo3(name = "register_batch")]
    fn py_register_batch(&self, actions: Vec<pyo3::Bound<'_, pyo3::types::PyAny>>) {
        for item in &actions {
            let Ok(dict) = item.cast::<pyo3::types::PyDict>() else {
                continue;
            };
            let name: String = dict
                .get_item("name")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let description: String = dict
                .get_item("description")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let category: String = dict
                .get_item("category")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let tags: Vec<String> = dict
                .get_item("tags")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let dcc: String = dict
                .get_item("dcc")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_else(|| DEFAULT_DCC.to_string());
            let version: String = dict
                .get_item("version")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_else(|| DEFAULT_VERSION.to_string());
            let input_schema_str: Option<String> = dict
                .get_item("input_schema")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let output_schema_str: Option<String> = dict
                .get_item("output_schema")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let source_file: Option<String> = dict
                .get_item("source_file")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let skill_name: Option<String> = dict
                .get_item("skill_name")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let group: String = dict
                .get_item("group")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let enabled: bool = dict
                .get_item("enabled")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or(true);
            let required_capabilities: Vec<String> = dict
                .get_item("required_capabilities")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok())
                .unwrap_or_default();
            let execution_str: Option<String> = dict
                .get_item("execution")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let execution = match execution_str.as_deref() {
                None | Some("sync") => ExecutionMode::Sync,
                Some("async") => ExecutionMode::Async,
                Some(other) => {
                    tracing::warn!(
                        "Invalid execution mode {other:?} for '{name}' — defaulting to sync"
                    );
                    ExecutionMode::Sync
                }
            };
            let timeout_hint_secs: Option<u32> = dict
                .get_item("timeout_hint_secs")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let thread_affinity_str: Option<String> = dict
                .get_item("thread_affinity")
                .ok()
                .flatten()
                .and_then(|v| v.extract().ok());
            let thread_affinity = match thread_affinity_str.as_deref() {
                None => ThreadAffinity::Any,
                Some(value) => match ThreadAffinity::parse(value) {
                    Some(affinity) => affinity,
                    None => {
                        tracing::warn!(
                            "Invalid thread_affinity {value:?} for '{name}' — defaulting to 'any'"
                        );
                        ThreadAffinity::Any
                    }
                },
            };

            let input_schema =
                parse_schema_or_default(input_schema_str.as_deref(), "input_schema", &name);
            let output_schema =
                parse_schema_or_default(output_schema_str.as_deref(), "output_schema", &name);

            self.register_action(ActionMeta {
                name,
                description,
                category,
                tags,
                dcc,
                version,
                input_schema,
                output_schema,
                source_file,
                skill_name,
                group,
                enabled,
                required_capabilities,
                execution,
                timeout_hint_secs,
                thread_affinity,
                annotations: ToolAnnotations::default(),
                next_tools: NextTools::default(),
            });
        }
    }

    #[pyo3(name = "unregister")]
    #[pyo3(signature = (name, dcc_name=None))]
    fn py_unregister(&self, name: &str, dcc_name: Option<&str>) -> bool {
        self.unregister(name, dcc_name)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (name, description="".to_string(), category="".to_string(), tags=vec![], dcc=DEFAULT_DCC.to_string(), version=DEFAULT_VERSION.to_string(), input_schema=None, output_schema=None, source_file=None, skill_name=None, group="".to_string(), enabled=true, required_capabilities=None, execution="sync".to_string(), timeout_hint_secs=None, thread_affinity="any".to_string()))]
    fn register(
        &self,
        name: String,
        description: String,
        category: String,
        tags: Vec<String>,
        dcc: String,
        version: String,
        input_schema: Option<String>,
        output_schema: Option<String>,
        source_file: Option<String>,
        skill_name: Option<String>,
        group: String,
        enabled: bool,
        required_capabilities: Option<Vec<String>>,
        execution: String,
        timeout_hint_secs: Option<u32>,
        thread_affinity: String,
    ) -> pyo3::PyResult<()> {
        let input_schema = parse_schema_or_default(input_schema.as_deref(), "input_schema", &name);
        let output_schema =
            parse_schema_or_default(output_schema.as_deref(), "output_schema", &name);
        let execution = match execution.as_str() {
            "sync" => ExecutionMode::Sync,
            "async" => ExecutionMode::Async,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "execution must be 'sync' or 'async' (got {other:?})",
                )));
            }
        };
        let thread_affinity = ThreadAffinity::parse(&thread_affinity).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "thread_affinity must be 'any' or 'main' (got {thread_affinity:?})"
            ))
        })?;

        self.register_action(ActionMeta {
            name,
            description,
            category,
            tags,
            dcc,
            version,
            input_schema,
            output_schema,
            source_file,
            skill_name,
            group,
            enabled,
            required_capabilities: required_capabilities.unwrap_or_default(),
            execution,
            timeout_hint_secs,
            thread_affinity,
            annotations: ToolAnnotations::default(),
            next_tools: NextTools::default(),
        });
        Ok(())
    }

    #[pyo3(name = "set_group_enabled")]
    fn py_set_group_enabled(&self, group: &str, enabled: bool) -> usize {
        self.set_group_enabled(group, enabled)
    }

    #[pyo3(name = "set_action_enabled")]
    fn py_set_action_enabled(&self, name: &str, enabled: bool) -> bool {
        self.set_action_enabled(name, enabled)
    }

    #[pyo3(name = "list_actions_enabled")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_list_actions_enabled(
        &self,
        py: Python,
        dcc_name: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        self.list_actions_enabled(dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    #[pyo3(name = "list_actions_in_group")]
    fn py_list_actions_in_group(&self, py: Python, group: &str) -> PyResult<Vec<Py<PyAny>>> {
        self.list_actions_in_group(group)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    #[pyo3(name = "list_groups")]
    fn py_list_groups(&self) -> Vec<String> {
        self.list_groups()
    }

    #[pyo3(name = "get_action")]
    #[pyo3(signature = (name, dcc_name=None))]
    fn py_get_action(
        &self,
        py: Python,
        name: &str,
        dcc_name: Option<&str>,
    ) -> PyResult<Option<Py<PyAny>>> {
        self.get_action(name, dcc_name)
            .map(|meta| action_meta_to_py(py, &meta))
            .transpose()
    }

    #[pyo3(name = "list_actions_for_dcc")]
    fn py_list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.list_actions_for_dcc(dcc_name)
    }

    #[pyo3(name = "list_actions")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_list_actions(&self, py: Python, dcc_name: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
        self.list_actions(dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    #[pyo3(name = "get_all_dccs")]
    fn py_get_all_dccs(&self) -> Vec<String> {
        self.get_all_dccs()
    }

    #[pyo3(name = "search_actions")]
    #[pyo3(signature = (category=None, tags=vec![], dcc_name=None))]
    fn py_search_actions(
        &self,
        py: Python,
        category: Option<&str>,
        tags: Vec<String>,
        dcc_name: Option<&str>,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.search_actions(category, &tag_refs, dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    #[pyo3(name = "get_categories")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_get_categories(&self, dcc_name: Option<&str>) -> Vec<String> {
        self.get_categories(dcc_name)
    }

    #[pyo3(name = "get_tags")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_get_tags(&self, dcc_name: Option<&str>) -> Vec<String> {
        self.get_tags(dcc_name)
    }

    #[pyo3(name = "count_actions")]
    #[pyo3(signature = (category=None, tags=vec![], dcc_name=None))]
    fn py_count_actions(
        &self,
        category: Option<&str>,
        tags: Vec<String>,
        dcc_name: Option<&str>,
    ) -> usize {
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        self.count_actions(category, &tag_refs, dcc_name)
    }

    #[pyo3(name = "reset")]
    fn py_reset(&self) {
        self.reset()
    }

    fn __len__(&self) -> usize {
        self.len()
    }

    fn __contains__(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    fn __repr__(&self) -> String {
        format!("ToolRegistry(actions={})", self.len())
    }
}

#[cfg(feature = "python-bindings")]
fn parse_schema_or_default(
    json: Option<&str>,
    field_name: &str,
    action_name: &str,
) -> serde_json::Value {
    match json {
        Some(schema) => serde_json::from_str(schema).unwrap_or_else(|err| {
            tracing::warn!("Invalid {field_name} JSON for '{action_name}': {err} — using default");
            default_schema().clone()
        }),
        None => default_schema().clone(),
    }
}

#[cfg(feature = "python-bindings")]
fn action_meta_to_py(py: Python, meta: &ActionMeta) -> PyResult<Py<PyAny>> {
    let json_val = serde_json::to_value(meta)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &json_val)
}
