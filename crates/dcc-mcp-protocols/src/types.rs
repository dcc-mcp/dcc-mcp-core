//! MCP Protocol type definitions as Rust structs.
//!
//! Serde-backed `#[pyclass]` types exposed to Python via PyO3.
//! Reference: https://modelcontextprotocol.io/specification/2025-11-25

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

#[cfg(feature = "python-bindings")]
use dcc_mcp_utils::constants::DEFAULT_MIME_TYPE;
use serde::{Deserialize, Serialize};

/// Annotations for MCP Tool behavior hints.
/// Annotations for MCP Tool behavior hints.
///
/// Per MCP spec (2025-11-25), tools MAY include annotations that describe
/// their destructive/idempotent nature and open-world safety.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
///
/// Per MCP spec (2025-11-25), a tool has a name, description, input/output schemas,
/// and optional annotations providing behavioral hints.
/// MCP Tool definition schema.
///
/// Per MCP spec (2025-11-25), a tool has a name, description, input/output schemas,
/// and optional annotations providing behavioral hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Annotations for MCP Resource behavior hints.
///
/// Per MCP spec (2025-11-25), resources MAY include annotations that describe
/// their audience (user/assistant) and priority level.
/// Annotations for MCP Resource behavior hints.
///
/// Per MCP spec (2025-11-25), resources MAY include annotations that describe
/// their audience (user/assistant) and priority level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceAnnotations", get_all, set_all, from_py_object)
)]
pub struct ResourceAnnotations {
    /// Describes who the intended audience is.
    /// Each element may be "user" or "assistant".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audience: Vec<String>,
    /// Priority hint for ordering (0.0 = lowest, 1.0 = highest).
    pub priority: Option<f64>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceAnnotations {
    #[new]
    #[pyo3(signature = (audience=vec![], priority=None))]
    fn new(audience: Vec<String>, priority: Option<f64>) -> Self {
        Self { audience, priority }
    }

    fn __repr__(&self) -> String {
        format!(
            "ResourceAnnotations(audience={:?}, priority={:?})",
            self.audience, self.priority
        )
    }
}

/// MCP Resource definition.
///
/// Per MCP spec (2025-11-25), a resource has a URI, name, description, MIME type,
/// and optional annotations providing audience/priority hints.
/// MCP Resource definition.
///
/// Per MCP spec (2025-11-25), a resource has a URI, name, description, MIME type,
/// and optional annotations providing audience/priority hints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceDefinition", get_all, set_all, from_py_object)
)]
pub struct ResourceDefinition {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ResourceAnnotations>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceDefinition {
    #[new]
    #[pyo3(signature = (uri, name, description, mime_type=DEFAULT_MIME_TYPE.to_string(), annotations=None))]
    fn new(
        uri: String,
        name: String,
        description: String,
        mime_type: String,
        annotations: Option<ResourceAnnotations>,
    ) -> Self {
        Self {
            uri,
            name,
            description,
            mime_type,
            annotations,
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
///
/// Per MCP spec (2025-11-25), a resource template has a URI template, name,
/// description, MIME type, and optional annotations.
/// MCP Resource Template definition.
///
/// Per MCP spec (2025-11-25), a resource template has a URI template, name,
/// description, MIME type, and optional annotations.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "ResourceTemplateDefinition", get_all, set_all, from_py_object)
)]
pub struct ResourceTemplateDefinition {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for this resource template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ResourceAnnotations>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl ResourceTemplateDefinition {
    #[new]
    #[pyo3(signature = (uri_template, name, description, mime_type=DEFAULT_MIME_TYPE.to_string(), annotations=None))]
    fn new(
        uri_template: String,
        name: String,
        description: String,
        mime_type: String,
        annotations: Option<ResourceAnnotations>,
    ) -> Self {
        Self {
            uri_template,
            name,
            description,
            mime_type,
            annotations,
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
/// MCP Prompt argument.
///
/// Describes a single named argument that a prompt accepts, including
/// whether it is required or optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "PromptArgument", eq, get_all, set_all, from_py_object)
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
///
/// Per MCP spec (2025-11-25), a prompt MAY declare typed arguments that
/// the client should collect before invoking the prompt.
/// MCP Prompt definition.
///
/// Per MCP spec (2025-11-25), a prompt MAY declare typed arguments that
/// the client should collect before invoking the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "PromptDefinition", eq, get_all, set_all, from_py_object)
)]
pub struct PromptDefinition {
    pub name: String,
    pub description: String,
    /// Optional list of arguments the prompt accepts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<PromptArgument>,
}

#[cfg(feature = "python-bindings")]
#[pymethods]
impl PromptDefinition {
    #[new]
    #[pyo3(signature = (name, description, arguments=vec![]))]
    fn new(name: String, description: String, arguments: Vec<PromptArgument>) -> Self {
        Self {
            name,
            description,
            arguments,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "PromptDefinition(name={:?}, arguments={})",
            self.name,
            self.arguments.len()
        )
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
        // annotations should be omitted when None
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
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("\"annotations\""));
        assert!(json.contains("\"destructiveHint\":true"));
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
            }),
        };
        let json = serde_json::to_string(&td).unwrap();
        let deserialized: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, td);
    }

    #[test]
    fn test_prompt_definition_with_arguments() {
        let pd = PromptDefinition {
            name: "review_code".to_string(),
            description: "Review code for issues".to_string(),
            arguments: vec![
                PromptArgument {
                    name: "language".to_string(),
                    description: "Programming language".to_string(),
                    required: true,
                },
                PromptArgument {
                    name: "style".to_string(),
                    description: "Review style".to_string(),
                    required: false,
                },
            ],
        };
        let json = serde_json::to_string(&pd).unwrap();
        assert!(json.contains("\"arguments\""));
        assert!(json.contains("\"language\""));
        assert!(json.contains("\"required\":true"));

        let deserialized: PromptDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.arguments.len(), 2);
        assert_eq!(deserialized.arguments[0].name, "language");
        assert!(deserialized.arguments[0].required);
    }

    #[test]
    fn test_prompt_definition_empty_arguments() {
        let pd = PromptDefinition {
            name: "simple".to_string(),
            description: "A simple prompt".to_string(),
            arguments: vec![],
        };
        let json = serde_json::to_string(&pd).unwrap();
        // Empty arguments should be omitted
        assert!(!json.contains("arguments"));
    }

    #[test]
    fn test_prompt_definition_deserialize_without_arguments() {
        // Backward compatibility: JSON without arguments field should still deserialize
        let json = r#"{"name":"legacy","description":"A legacy prompt"}"#;
        let pd: PromptDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(pd.name, "legacy");
        assert!(pd.arguments.is_empty());
    }

    #[test]
    fn test_resource_definition_serialization() {
        let rd = ResourceDefinition {
            uri: "file:///scene.ma".to_string(),
            name: "scene".to_string(),
            description: "The scene file".to_string(),
            mime_type: "application/x-maya".to_string(),
            annotations: None,
        };
        let json = serde_json::to_string(&rd).unwrap();
        let deserialized: ResourceDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, rd);
        // annotations should be omitted when None
        assert!(!json.contains("annotations"));
    }

    #[test]
    fn test_resource_definition_with_annotations() {
        let rd = ResourceDefinition {
            uri: "file:///scene.ma".to_string(),
            name: "scene".to_string(),
            description: "The scene file".to_string(),
            mime_type: "application/x-maya".to_string(),
            annotations: Some(ResourceAnnotations {
                audience: vec!["user".to_string()],
                priority: Some(0.8),
            }),
        };
        let json = serde_json::to_string(&rd).unwrap();
        assert!(json.contains("\"annotations\""));
        assert!(json.contains("\"audience\""));
        assert!(json.contains("\"priority\""));

        let deserialized: ResourceDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, rd);
    }

    #[test]
    fn test_resource_annotations_default() {
        let ann = ResourceAnnotations::default();
        assert!(ann.audience.is_empty());
        assert!(ann.priority.is_none());
    }

    #[test]
    fn test_resource_annotations_roundtrip() {
        let ann = ResourceAnnotations {
            audience: vec!["user".to_string(), "assistant".to_string()],
            priority: Some(0.5),
        };
        let json = serde_json::to_string(&ann).unwrap();
        let deserialized: ResourceAnnotations = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ann);
    }

    #[test]
    fn test_resource_definition_deserialize_without_annotations() {
        // Backward compatibility: JSON without annotations field should still deserialize
        let json = r#"{"uri":"file:///test","name":"test","description":"A test","mimeType":"text/plain"}"#;
        let rd: ResourceDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(rd.name, "test");
        assert!(rd.annotations.is_none());
    }

    #[test]
    fn test_resource_template_with_annotations() {
        let rtd = ResourceTemplateDefinition {
            uri_template: "file:///{path}".to_string(),
            name: "template".to_string(),
            description: "A template".to_string(),
            mime_type: "text/plain".to_string(),
            annotations: Some(ResourceAnnotations {
                audience: vec!["assistant".to_string()],
                priority: Some(0.3),
            }),
        };
        let json = serde_json::to_string(&rtd).unwrap();
        let deserialized: ResourceTemplateDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, rtd);
    }

    #[test]
    fn test_resource_template_definition_default() {
        let rtd = ResourceTemplateDefinition::default();
        assert!(rtd.uri_template.is_empty());
        assert!(rtd.name.is_empty());
        assert!(rtd.annotations.is_none());
    }

    #[test]
    fn test_prompt_argument_serialization() {
        let arg = PromptArgument {
            name: "code".to_string(),
            description: "Code to review".to_string(),
            required: true,
        };
        let json = serde_json::to_string(&arg).unwrap();
        let deserialized: PromptArgument = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, arg);
    }
}
