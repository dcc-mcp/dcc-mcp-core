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
    pub title: Option<String>,
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: Option<bool>,
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
        Self {
            title,
            read_only_hint,
            destructive_hint,
            idempotent_hint,
            open_world_hint,
        }
    }

    #[getter]
    fn get_title(&self) -> Option<String> {
        self.title.clone()
    }
    #[setter]
    fn set_title(&mut self, v: Option<String>) {
        self.title = v;
    }
    #[getter]
    fn get_read_only_hint(&self) -> Option<bool> {
        self.read_only_hint
    }
    #[setter]
    fn set_read_only_hint(&mut self, v: Option<bool>) {
        self.read_only_hint = v;
    }
    #[getter]
    fn get_destructive_hint(&self) -> Option<bool> {
        self.destructive_hint
    }
    #[setter]
    fn set_destructive_hint(&mut self, v: Option<bool>) {
        self.destructive_hint = v;
    }
    #[getter]
    fn get_idempotent_hint(&self) -> Option<bool> {
        self.idempotent_hint
    }
    #[setter]
    fn set_idempotent_hint(&mut self, v: Option<bool>) {
        self.idempotent_hint = v;
    }
    #[getter]
    fn get_open_world_hint(&self) -> Option<bool> {
        self.open_world_hint
    }
    #[setter]
    fn set_open_world_hint(&mut self, v: Option<bool>) {
        self.open_world_hint = v;
    }
}

/// MCP Tool definition schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ToolDefinition"))]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: String,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<String>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ToolDefinition {
    #[new]
    #[pyo3(signature = (name, description, input_schema, output_schema=None))]
    fn new(
        name: String,
        description: String,
        input_schema: String,
        output_schema: Option<String>,
    ) -> Self {
        Self {
            name,
            description,
            input_schema,
            output_schema,
        }
    }

    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }
    #[getter]
    fn get_input_schema(&self) -> &str {
        &self.input_schema
    }
    #[getter]
    fn get_output_schema(&self) -> Option<&str> {
        self.output_schema.as_deref()
    }

    fn __repr__(&self) -> String {
        format!("ToolDefinition(name={:?})", self.name)
    }
}

/// MCP Resource definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "ResourceDefinition"))]
pub struct ResourceDefinition {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceDefinition {
    #[new]
    #[pyo3(signature = (uri, name, description, mime_type="text/plain".to_string()))]
    fn new(uri: String, name: String, description: String, mime_type: String) -> Self {
        Self {
            uri,
            name,
            description,
            mime_type,
        }
    }

    #[getter]
    fn get_uri(&self) -> &str {
        &self.uri
    }
    #[setter]
    fn set_uri(&mut self, v: String) {
        self.uri = v;
    }
    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }
    #[getter]
    fn get_mime_type(&self) -> &str {
        &self.mime_type
    }
    #[setter]
    fn set_mime_type(&mut self, v: String) {
        self.mime_type = v;
    }
}

/// MCP Resource Template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceTemplateDefinition")
)]
pub struct ResourceTemplateDefinition {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceTemplateDefinition {
    #[new]
    #[pyo3(signature = (uri_template, name, description, mime_type="text/plain".to_string()))]
    fn new(uri_template: String, name: String, description: String, mime_type: String) -> Self {
        Self {
            uri_template,
            name,
            description,
            mime_type,
        }
    }

    #[getter]
    fn get_uri_template(&self) -> &str {
        &self.uri_template
    }
    #[setter]
    fn set_uri_template(&mut self, v: String) {
        self.uri_template = v;
    }
    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }
    #[getter]
    fn get_mime_type(&self) -> &str {
        &self.mime_type
    }
    #[setter]
    fn set_mime_type(&mut self, v: String) {
        self.mime_type = v;
    }
}

/// MCP Prompt argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "PromptArgument"))]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PromptArgument {
    #[new]
    #[pyo3(signature = (name, description, required=false))]
    fn new(name: String, description: String, required: bool) -> Self {
        Self {
            name,
            description,
            required,
        }
    }

    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
    }
    #[getter]
    fn get_required(&self) -> bool {
        self.required
    }
    #[setter]
    fn set_required(&mut self, v: bool) {
        self.required = v;
    }
}

/// MCP Prompt definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "python-bindings", pyclass(name = "PromptDefinition"))]
pub struct PromptDefinition {
    pub name: String,
    pub description: String,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PromptDefinition {
    #[new]
    fn new(name: String, description: String) -> Self {
        Self { name, description }
    }

    #[getter]
    fn get_name(&self) -> &str {
        &self.name
    }
    #[setter]
    fn set_name(&mut self, v: String) {
        self.name = v;
    }
    #[getter]
    fn get_description(&self) -> &str {
        &self.description
    }
    #[setter]
    fn set_description(&mut self, v: String) {
        self.description = v;
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
