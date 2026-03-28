//! MCP Protocol type definitions as Rust structs.
//!
//! These replace the Pydantic models with serde-backed #[pyclass] types.
//! Reference: https://modelcontextprotocol.io/specification/2025-11-25

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use serde::{Deserialize, Serialize};

/// Annotations for MCP Tool behavior hints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ToolAnnotations"))]
pub struct ToolAnnotations {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub title: Option<String>,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: Option<bool>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolAnnotations {
    #[new]
    #[pyo3(signature = (title=None, read_only_hint=None, destructive_hint=None, idempotent_hint=None, open_world_hint=None))]
    fn new(
        title: Option<String>,
        read_only_hint: Option<bool>,
        destructive_hint: Option<bool>,
        idempotent_hint: Option<bool>,
        open_world_hint: Option<bool>,
    ) -> Self {
        Self { title, read_only_hint, destructive_hint, idempotent_hint, open_world_hint }
    }
}

/// MCP Tool definition schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ToolDefinition"))]
pub struct ToolDefinition {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub description: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get))]
    #[serde(rename = "inputSchema")]
    pub input_schema: String, // JSON string
    #[cfg_attr(feature = "python-bindings", pyo3(get))]
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolDefinition {
    #[new]
    #[pyo3(signature = (name, description, input_schema, output_schema=None))]
    fn new(name: String, description: String, input_schema: String, output_schema: Option<String>) -> Self {
        Self { name, description, input_schema, output_schema }
    }

    fn __repr__(&self) -> String {
        format!("ToolDefinition(name={:?})", self.name)
    }
}

/// MCP Resource definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ResourceDefinition"))]
pub struct ResourceDefinition {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub uri: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub description: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceDefinition {
    #[new]
    #[pyo3(signature = (uri, name, description, mime_type="text/plain".to_string()))]
    fn new(uri: String, name: String, description: String, mime_type: String) -> Self {
        Self { uri, name, description, mime_type }
    }
}

/// MCP Resource Template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ResourceTemplateDefinition"))]
pub struct ResourceTemplateDefinition {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub description: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceTemplateDefinition {
    #[new]
    #[pyo3(signature = (uri_template, name, description, mime_type="text/plain".to_string()))]
    fn new(uri_template: String, name: String, description: String, mime_type: String) -> Self {
        Self { uri_template, name, description, mime_type }
    }
}

/// MCP Prompt argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "PromptArgument"))]
pub struct PromptArgument {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub description: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub required: bool,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PromptArgument {
    #[new]
    #[pyo3(signature = (name, description, required=false))]
    fn new(name: String, description: String, required: bool) -> Self {
        Self { name, description, required }
    }
}

/// MCP Prompt definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "PromptDefinition"))]
pub struct PromptDefinition {
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub name: String,
    #[cfg_attr(feature = "python-bindings", pyo3(get, set))]
    pub description: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PromptDefinition {
    #[new]
    fn new(name: String, description: String) -> Self {
        Self { name, description }
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
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("create_sphere"));
    }
}
