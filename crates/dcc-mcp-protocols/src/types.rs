//! MCP Protocol type definitions as Rust structs.
//!
//! These replace the Pydantic models with serde-backed #[pyclass] types.
//! Reference: https://modelcontextprotocol.io/specification/2025-11-25

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::DEFAULT_MIME_TYPE;
use serde::{Deserialize, Serialize};

/// Annotations for MCP Tool behavior hints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolAnnotations", eq, get_all, set_all)
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

    fn __repr__(&self) -> String {
        format!(
            "ToolAnnotations(title={:?}, read_only={:?}, destructive={:?}, idempotent={:?}, open_world={:?})",
            self.title,
            self.read_only_hint,
            self.destructive_hint,
            self.idempotent_hint,
            self.open_world_hint
        )
    }
}

/// MCP Tool definition schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ToolDefinition", eq, get_all, set_all)
)]
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

    fn __repr__(&self) -> String {
        format!("ToolDefinition(name={:?})", self.name)
    }
}

/// MCP Resource definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceDefinition", eq, get_all, set_all)
)]
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
    #[pyo3(signature = (uri, name, description, mime_type=DEFAULT_MIME_TYPE.to_string()))]
    fn new(uri: String, name: String, description: String, mime_type: String) -> Self {
        Self {
            uri,
            name,
            description,
            mime_type,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ResourceDefinition(name={:?}, uri={:?})",
            self.name, self.uri
        )
    }
}

/// MCP Resource Template definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceTemplateDefinition", eq, get_all, set_all)
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
    #[pyo3(signature = (uri_template, name, description, mime_type=DEFAULT_MIME_TYPE.to_string()))]
    fn new(uri_template: String, name: String, description: String, mime_type: String) -> Self {
        Self {
            uri_template,
            name,
            description,
            mime_type,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ResourceTemplateDefinition(name={:?}, uri_template={:?})",
            self.name, self.uri_template
        )
    }
}

/// MCP Prompt argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "PromptArgument", eq, get_all, set_all)
)]
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

    fn __repr__(&self) -> String {
        format!(
            "PromptArgument(name={:?}, required={})",
            self.name, self.required
        )
    }
}

/// MCP Prompt definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "PromptDefinition", eq, get_all, set_all)
)]
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

    fn __repr__(&self) -> String {
        format!("PromptDefinition(name={:?})", self.name)
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
