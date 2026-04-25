//! MCP prompt type definitions.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde::{Deserialize, Serialize};

/// MCP Prompt argument.
///
/// Describes a single named argument that a prompt accepts, including
/// whether it is required or optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[cfg_attr(
    feature = "python-bindings",
    pyclass(name = "PromptArgument", eq, get_all, set_all, from_py_object)
)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
        assert!(!json.contains("arguments"));
    }

    #[test]
    fn test_prompt_definition_deserialize_without_arguments() {
        let json = r#"{"name":"legacy","description":"A legacy prompt"}"#;
        let pd: PromptDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(pd.name, "legacy");
        assert!(pd.arguments.is_empty());
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
