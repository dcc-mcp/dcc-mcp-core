//! ActionRegistry — thread-safe registry for Action classes.
//!
//! Uses DashMap for lock-free concurrent reads, replacing the Python singleton pattern.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::py_json::json_value_to_pyobject;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::{DEFAULT_DCC, DEFAULT_VERSION};

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::default_schema;

/// Metadata about a registered Action (stored in Rust).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionMeta {
    /// Unique action identifier.
    pub name: String,
    /// Human-readable action description.
    pub description: String,
    /// Action category for grouping (e.g. "geometry", "pipeline").
    pub category: String,
    /// Searchable tags for discovery.
    pub tags: Vec<String>,
    /// Target DCC application (e.g. "maya", "blender").
    pub dcc: String,
    /// Semantic version string.
    pub version: String,
    /// JSON Schema for action input parameters.
    pub input_schema: serde_json::Value,
    /// JSON Schema for action output.
    pub output_schema: serde_json::Value,
    /// Optional path to the Python source file defining this action.
    pub source_file: Option<String>,
}

/// Thread-safe Action registry.
///
/// Unlike the Python singleton, each ActionManager can own its own registry,
/// eliminating cross-DCC pollution.
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ActionRegistry", from_py_object)
)]
#[derive(Debug, Clone)]
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
    #[must_use]
    pub fn new() -> Self {
        Self {
            actions: Arc::new(DashMap::new()),
            dcc_actions: Arc::new(DashMap::new()),
        }
    }

    /// Register an action with metadata.
    pub fn register_action(&self, meta: ActionMeta) {
        let name = meta.name.clone();
        let dcc = meta.dcc.clone();
        // Clone meta for the DCC map before moving it into `actions`.
        // This avoids a get-after-insert race where another thread could
        // overwrite the entry between insert and get.
        let meta_for_dcc = meta.clone();
        self.actions.insert(name.clone(), meta);
        self.dcc_actions
            .entry(dcc)
            .or_default()
            .insert(name, meta_for_dcc);
    }

    /// Get action metadata by name.
    #[must_use]
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
    #[must_use]
    pub fn list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.dcc_actions
            .get(dcc_name)
            .map(|dcc_map| dcc_map.iter().map(|r| r.key().clone()).collect())
            .unwrap_or_default()
    }

    /// List all registered DCC names.
    #[must_use]
    pub fn get_all_dccs(&self) -> Vec<String> {
        self.dcc_actions.iter().map(|r| r.key().clone()).collect()
    }

    /// Get all actions as metadata list.
    #[must_use]
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
    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns `true` if no actions are registered.
    #[must_use]
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
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (name, description="".to_string(), category="".to_string(), tags=vec![], dcc=DEFAULT_DCC.to_string(), version=DEFAULT_VERSION.to_string(), input_schema=None, output_schema=None, source_file=None))]
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
    ) {
        let input_schema = parse_schema_or_default(input_schema.as_deref(), "input_schema", &name);
        let output_schema =
            parse_schema_or_default(output_schema.as_deref(), "output_schema", &name);

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
        });
    }

    /// Get action metadata as dict.
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

    /// List all action names for a DCC.
    #[pyo3(name = "list_actions_for_dcc")]
    fn py_list_actions_for_dcc(&self, dcc_name: &str) -> Vec<String> {
        self.list_actions_for_dcc(dcc_name)
    }

    /// List all actions with metadata.
    #[pyo3(name = "list_actions")]
    #[pyo3(signature = (dcc_name=None))]
    fn py_list_actions(&self, py: Python, dcc_name: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
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

    fn __contains__(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    fn __repr__(&self) -> String {
        format!("ActionRegistry(actions={})", self.len())
    }
}

/// Parse a JSON schema string, falling back to [`default_schema`] on `None` or invalid JSON.
#[cfg(feature = "python-bindings")]
fn parse_schema_or_default(
    json: Option<&str>,
    field_name: &str,
    action_name: &str,
) -> serde_json::Value {
    match json {
        Some(s) => serde_json::from_str(s).unwrap_or_else(|e| {
            tracing::warn!("Invalid {field_name} JSON for '{action_name}': {e} — using default");
            default_schema().clone()
        }),
        None => default_schema().clone(),
    }
}

/// Convert [`ActionMeta`] to a Python dict via serde serialization.
///
/// This leverages the existing `#[derive(Serialize)]` on `ActionMeta` to avoid
/// manually enumerating every field — new fields are automatically included.
#[cfg(feature = "python-bindings")]
fn action_meta_to_py(py: Python, meta: &ActionMeta) -> PyResult<Py<PyAny>> {
    let json_val = serde_json::to_value(meta)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &json_val)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_action(name: &str, dcc: &str) -> ActionMeta {
        ActionMeta {
            name: name.into(),
            description: format!("{name} description"),
            category: "geometry".into(),
            tags: vec!["test".into()],
            dcc: dcc.into(),
            version: "1.0.0".into(),
            ..Default::default()
        }
    }

    // ── Happy path ─────────────────────────────────────────────────────────────

    #[test]
    fn test_registry_register_and_get() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("create_sphere", "maya"));

        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        assert!(reg.get_action("create_sphere", None).is_some());
        assert!(reg.get_action("create_sphere", Some("maya")).is_some());
    }

    #[test]
    fn test_registry_default_is_empty() {
        let reg = ActionRegistry::default();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn test_registry_get_returns_correct_metadata() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("delete_mesh", "blender"));

        let meta = reg.get_action("delete_mesh", None).unwrap();
        assert_eq!(meta.name, "delete_mesh");
        assert_eq!(meta.dcc, "blender");
        assert_eq!(meta.version, "1.0.0");
    }

    #[test]
    fn test_registry_list_actions_all() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("a1", "maya"));
        reg.register_action(make_action("a2", "maya"));
        reg.register_action(make_action("a3", "blender"));

        let all = reg.list_actions(None);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_registry_list_actions_for_dcc() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("ma1", "maya"));
        reg.register_action(make_action("ma2", "maya"));
        reg.register_action(make_action("bl1", "blender"));

        let maya_actions = reg.list_actions(Some("maya"));
        assert_eq!(maya_actions.len(), 2);
        let blender_actions = reg.list_actions(Some("blender"));
        assert_eq!(blender_actions.len(), 1);
    }

    #[test]
    fn test_registry_list_actions_for_dcc_names() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("x", "houdini"));

        let names = reg.list_actions_for_dcc("houdini");
        assert_eq!(names, vec!["x"]);
    }

    #[test]
    fn test_registry_get_all_dccs() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("a", "maya"));
        reg.register_action(make_action("b", "blender"));
        reg.register_action(make_action("c", "maya"));

        let mut dccs = reg.get_all_dccs();
        dccs.sort();
        assert_eq!(dccs, vec!["blender", "maya"]);
    }

    // ── Error / edge paths ──────────────────────────────────────────────────────

    #[test]
    fn test_registry_get_unknown_returns_none() {
        let reg = ActionRegistry::new();
        assert!(reg.get_action("nonexistent", None).is_none());
        assert!(reg.get_action("nonexistent", Some("maya")).is_none());
    }

    #[test]
    fn test_registry_get_action_wrong_dcc_returns_none() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("create_sphere", "maya"));
        assert!(reg.get_action("create_sphere", Some("blender")).is_none());
    }

    #[test]
    fn test_registry_list_for_unknown_dcc_empty() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("a", "maya"));
        assert!(reg.list_actions(Some("unknown_dcc")).is_empty());
        assert!(reg.list_actions_for_dcc("unknown_dcc").is_empty());
    }

    #[test]
    fn test_registry_reset_clears_all() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("a", "maya"));
        reg.register_action(make_action("b", "blender"));
        assert_eq!(reg.len(), 2);

        reg.reset();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.list_actions(None).is_empty());
        assert!(reg.get_all_dccs().is_empty());
    }

    #[test]
    fn test_registry_overwrite_existing_action() {
        let reg = ActionRegistry::new();
        reg.register_action(make_action("my_action", "maya"));
        let updated = ActionMeta {
            name: "my_action".into(),
            description: "updated description".into(),
            dcc: "maya".into(),
            version: "2.0.0".into(),
            ..Default::default()
        };
        reg.register_action(updated);

        // Latest version wins
        let meta = reg.get_action("my_action", None).unwrap();
        assert_eq!(meta.version, "2.0.0");
        assert_eq!(meta.description, "updated description");
        // len should still be 1
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_tags_preserved() {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "tagged_action".into(),
            dcc: "maya".into(),
            tags: vec!["sculpt".into(), "deform".into()],
            ..Default::default()
        });
        let meta = reg.get_action("tagged_action", None).unwrap();
        assert_eq!(meta.tags, vec!["sculpt", "deform"]);
    }

    #[test]
    fn test_registry_source_file_optional() {
        let reg = ActionRegistry::new();
        reg.register_action(ActionMeta {
            name: "no_source".into(),
            dcc: "blender".into(),
            source_file: None,
            ..Default::default()
        });
        let meta = reg.get_action("no_source", None).unwrap();
        assert!(meta.source_file.is_none());

        reg.register_action(ActionMeta {
            name: "with_source".into(),
            dcc: "blender".into(),
            source_file: Some("/path/to/action.py".into()),
            ..Default::default()
        });
        let meta2 = reg.get_action("with_source", None).unwrap();
        assert_eq!(meta2.source_file.as_deref(), Some("/path/to/action.py"));
    }

    // ── Serialization ───────────────────────────────────────────────────────────

    #[test]
    fn test_action_meta_serde_round_trip() {
        let meta = ActionMeta {
            name: "render_scene".into(),
            description: "Renders the active scene".into(),
            category: "rendering".into(),
            tags: vec!["render".into(), "output".into()],
            dcc: "houdini".into(),
            version: "3.1.0".into(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: serde_json::json!({"type": "string"}),
            source_file: Some("render.py".into()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: ActionMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn test_action_meta_default_serialization() {
        let meta = ActionMeta::default();
        let json = serde_json::to_string(&meta).unwrap();
        let back: ActionMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    // ── Concurrency ─────────────────────────────────────────────────────────────

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
                    name: format!("action_{i}"),
                    description: format!("Action {i}"),
                    dcc: "test".into(),
                    ..Default::default()
                });
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(reg.len(), 10);
    }

    #[test]
    fn test_registry_concurrent_reads_while_writing() {
        use std::sync::Arc;
        use std::thread;

        let reg = Arc::new(ActionRegistry::new());
        // Pre-populate
        for i in 0..5 {
            reg.register_action(make_action(&format!("pre_{i}"), "maya"));
        }

        let mut handles = vec![];
        // Readers
        for _ in 0..4 {
            let r = Arc::clone(&reg);
            handles.push(thread::spawn(move || {
                for _ in 0..20 {
                    let _ = r.list_actions(None);
                    let _ = r.get_all_dccs();
                }
            }));
        }
        // Writer
        {
            let r = Arc::clone(&reg);
            handles.push(thread::spawn(move || {
                for i in 0..5 {
                    r.register_action(make_action(&format!("new_{i}"), "blender"));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        // At least 5 pre-populated + up to 5 new
        assert!(reg.len() >= 5);
    }
}
