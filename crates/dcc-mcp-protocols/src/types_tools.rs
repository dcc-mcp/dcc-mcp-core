//! MCP tool type definitions.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde::{Deserialize, Serialize};

/// Annotations for MCP Tool behavior hints.
///
/// Per MCP spec (2025-11-25), tools MAY include annotations that describe
/// their destructive/idempotent nature and open-world safety.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolAnnotations", eq, get_all, set_all, from_py_object)
)]
pub struct ToolAnnotations {
    pub title: Option<String>,
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
    #[serde(rename = "deferredHint")]
    pub deferred_hint: Option<bool>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolAnnotations {
    #[new]
    #[pyo3(signature = (title=None, read_only_hint=None, destructive_hint=None, idempotent_hint=None, open_world_hint=None, deferred_hint=None))]
    fn new(
        title: Option<String>,
        read_only_hint: Option<bool>,
        destructive_hint: Option<bool>,
        idempotent_hint: Option<bool>,
        open_world_hint: Option<bool>,
        deferred_hint: Option<bool>,
    ) -> Self {
        Self {
            title,
            read_only_hint,
            destructive_hint,
            idempotent_hint,
            open_world_hint,
            deferred_hint,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ToolAnnotations(title={:?}, read_only={:?}, destructive={:?}, idempotent={:?}, open_world={:?}, deferred={:?})",
            self.title,
            self.read_only_hint,
            self.destructive_hint,
            self.idempotent_hint,
            self.open_world_hint,
            self.deferred_hint
        )
    }
}

/// MCP Tool definition schema.
///
/// Per MCP spec (2025-11-25), a tool has a name, description, input/output schemas,
/// and optional annotations providing behavioral hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolDefinition", eq, get_all, set_all, from_py_object)
)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: String,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<String>,
    /// Optional behavioral annotations for this tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolDefinition {
    #[new]
    #[pyo3(signature = (name, description, input_schema, output_schema=None, annotations=None))]
    fn new(
        name: String,
        description: String,
        input_schema: String,
        output_schema: Option<String>,
        annotations: Option<ToolAnnotations>,
    ) -> Self {
        Self {
            name,
            description,
            input_schema,
            output_schema,
            annotations,
        }
    }

    fn __repr__(&self) -> String {
        format!("ToolDefinition(name={:?})", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_annotations_default() {
        let ann = ToolAnnotations::default();
        assert!(ann.title.is_none());
    }

    #[test]
    fn test_tool_definition_serialize() {
        let td = ToolDefinition {
            name: "create_sphere".to_string(),
            description: "Create a sphere".to_string(),
            input_schema: r#"{"type":"object"}"#.to_string(),
            output_schema: None,
            annotations: None,
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("create_sphere"));
        assert!(!json.contains("annotations"));
    }

    #[test]
    fn test_tool_definition_with_annotations() {
        let td = ToolDefinition {
            name: "delete_scene".to_string(),
            description: "Delete the current scene".to_string(),
            input_schema: r#"{"type":"object"}"#.to_string(),
            output_schema: None,
            annotations: Some(ToolAnnotations {
                title: Some("Delete Scene".to_string()),
                read_only_hint: Some(false),
                destructive_hint: Some(true),
                idempotent_hint: Some(true),
                open_world_hint: None,
                deferred_hint: Some(true),
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("\"annotations\""));
        assert!(json.contains("\"destructiveHint\":true"));
        assert!(json.contains("\"deferredHint\":true"));
    }

    #[test]
    fn test_tool_definition_roundtrip() {
        let td = ToolDefinition {
            name: "read_scene".to_string(),
            description: "Read the scene".to_string(),
            input_schema: r#"{"type":"object"}"#.to_string(),
            output_schema: Some(r#"{"type":"string"}"#.to_string()),
            annotations: Some(ToolAnnotations {
                title: None,
                read_only_hint: Some(true),
                destructive_hint: None,
                idempotent_hint: None,
                open_world_hint: None,
                deferred_hint: None,
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, td);
    }
}
