//! MCP resource type definitions.

/// Default MIME type assigned to MCP resources when the producer does not
/// declare one. Per the MCP spec resources are inherently text-shaped.
pub const DEFAULT_MIME_TYPE: &str = "text/plain";
#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde::{Deserialize, Serialize};

/// Annotations for MCP Resource behavior hints.
///
/// Per MCP spec (2025-11-25), resources MAY include annotations that describe
/// their audience (user/assistant) and priority level.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
