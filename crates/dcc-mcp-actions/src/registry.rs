//! ActionRegistry — thread-safe registry for Action classes.
//!
//! Uses DashMap for lock-free concurrent reads, replacing the Python singleton pattern.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dashmap::DashMap;
use std::sync::Arc;

/// Metadata about a registered Action (stored in Rust).
#[derive(Debug, Clone)]
pub struct ActionMeta {
    pub name: String,
    pub internal_name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub dcc: String,
    pub version: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub source_file: Option<String>,
}

/// Thread-safe Action registry.
///
/// Unlike the Python singleton, each ActionManager can own its own registry,
/// eliminating cross-DCC pollution.
#[cfg_attr(feature = "python-bindings", pyclass(name = "ActionRegistry"))]
pub struct ActionRegistry {
    /// Main registry: action_name → ActionMeta
    actions: Arc<DashMap<String, ActionMeta>>,
    /// DCC-specific registry: dcc_name → { action_name → ActionMeta }
    dcc_actions: Arc<DashMap<String, DashMap<String, ActionMeta>>>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: Arc::new(DashMap::new()),
            dcc_actions: Arc::new(DashMap::new()),
        }
    }

    /// Register an action with metadata.
    pub fn register_action(&self, meta: ActionMeta) -> bool {
        let name = meta.name.clone();
        let dcc = meta.dcc.clone();

        // Register in main registry
        self.actions.insert(name.clone(), meta.clone());

        // Register in DCC-specific registry
        self.dcc_actions.entry(dcc).or_default().insert(name, meta);

        true
    }

    /// Get action metadata by name.
    pub fn get_action(&self, name: &str, dcc_name: Option<&str>) -> Option<ActionMeta> {
        if let Some(dcc) = dcc_name {
            if let Some(dcc_map) = self.dcc_actions.get(dcc) {
                return dcc_map.get(name).map(|r| r.value().clone());
            }
            return None;
        }
        self.actions.get(name).map(|r| r.value().clone())
    }

    /// List all actions for a DCC.
    pub fn list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.dcc_actions
            .get(dcc_name)
            .map(|dcc_map| dcc_map.iter().map(|r| r.key().clone()).collect())
            .unwrap_or_default()
    }

    /// List all registered DCC names.
    pub fn get_all_dccs(&self) -> Vec<String> {
        self.dcc_actions.iter().map(|r| r.key().clone()).collect()
    }

    /// Get all actions as metadata list.
    pub fn list_actions(&self, dcc_name: Option<&str>) -> Vec<ActionMeta> {
        if let Some(dcc) = dcc_name {
            return self
                .dcc_actions
                .get(dcc)
                .map(|dcc_map| dcc_map.iter().map(|r| r.value().clone()).collect())
                .unwrap_or_default();
        }
        self.actions.iter().map(|r| r.value().clone()).collect()
    }

    /// Clear all registered actions.
    pub fn reset(&self) {
        self.actions.clear();
        self.dcc_actions.clear();
    }

    /// Get number of registered actions.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ActionRegistry {
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    /// Register an action. Called from Python ActionManager.
    #[pyo3(signature = (name, description="".to_string(), category="".to_string(), tags=vec![], dcc="python".to_string(), version="1.0.0".to_string(), input_schema=None, output_schema=None, source_file=None))]
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
    ) -> bool {
        let input_schema = input_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));
        let output_schema = output_schema
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

        self.register_action(ActionMeta {
            name: name.clone(),
            internal_name: name,
            description,
            category,
            tags,
            dcc,
            version,
            input_schema,
            output_schema,
            source_file,
        })
    }

    /// Get action metadata as dict.
    #[pyo3(name = "get_action")]
    fn py_get_action(
        &self,
        py: Python,
        name: &str,
        dcc_name: Option<&str>,
    ) -> PyResult<Option<PyObject>> {
        Ok(self
            .get_action(name, dcc_name)
            .map(|meta| action_meta_to_py(py, &meta)))
    }

    /// List all action names for a DCC.
    #[pyo3(name = "list_actions_for_dcc")]
    fn py_list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.list_actions_for_dcc(dcc_name)
    }

    /// List all actions with metadata.
    #[pyo3(name = "list_actions")]
    fn py_list_actions(&self, py: Python, dcc_name: Option<&str>) -> Vec<PyObject> {
        self.list_actions(dcc_name)
            .iter()
            .map(|meta| action_meta_to_py(py, meta))
            .collect()
    }

    /// Get all registered DCC names.
    #[pyo3(name = "get_all_dccs")]
    fn py_get_all_dccs(&self) -> Vec<String> {
        self.get_all_dccs()
    }

    /// Reset the registry.
    #[pyo3(name = "reset")]
    fn py_reset(&self) {
        self.reset()
    }

    fn __len__(&self) -> usize {
        self.len()
    }

    fn __repr__(&self) -> String {
        format!("ActionRegistry(actions={})", self.len())
    }
}

#[cfg(feature = "python-bindings")]
fn action_meta_to_py(py: Python, meta: &ActionMeta) -> PyObject {
    use pyo3::types::PyDict;
    let dict = PyDict::new(py);
    let _ = dict.set_item("name", &meta.name);
    let _ = dict.set_item("internal_name", &meta.internal_name);
    let _ = dict.set_item("description", &meta.description);
    let _ = dict.set_item("category", &meta.category);
    let _ = dict.set_item("tags", &meta.tags);
    let _ = dict.set_item("dcc", &meta.dcc);
    let _ = dict.set_item("version", &meta.version);
    let _ = dict.set_item("source_file", meta.source_file.as_deref());
    // Serialize schemas as JSON strings for Python to parse
    let _ = dict.set_item("input_schema", meta.input_schema.to_string());
    let _ = dict.set_item("output_schema", meta.output_schema.to_string());
    dict.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register_and_get() {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "create_sphere".to_string(),
            internal_name: "create_sphere".to_string(),
            description: "Create a sphere".to_string(),
            category: "geometry".to_string(),
            tags: vec!["geometry".to_string()],
            dcc: "maya".to_string(),
            version: "1.0.0".to_string(),
            input_schema: serde_json::json!({}),
            output_schema: serde_json::json!({}),
            source_file: None,
        });

        assert_eq!(reg.len(), 1);
        assert!(reg.get_action("create_sphere", None).is_some());
        assert!(reg.get_action("create_sphere", Some("maya")).is_some());
        assert!(reg.get_action("create_sphere", Some("blender")).is_none());
    }

    #[test]
    fn test_registry_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let reg = Arc::new(ActionRegistry::new());
        let mut handles = vec![];

        for i in 0..10 {
            let reg = Arc::clone(&reg);
            handles.push(thread::spawn(move || {
                reg.register_action(ActionMeta {
                    name: format!("action_{}", i),
                    internal_name: format!("action_{}", i),
                    description: format!("Action {}", i),
                    category: "test".to_string(),
                    tags: vec![],
                    dcc: "test".to_string(),
                    version: "1.0.0".to_string(),
                    input_schema: serde_json::json!({}),
                    output_schema: serde_json::json!({}),
                    source_file: None,
                });
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(reg.len(), 10);
    }
}
