//! PyO3 bindings for resource-related MCP types.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::types::{
    DEFAULT_MIME_TYPE, ResourceAnnotations, ResourceDefinition, ResourceTemplateDefinition,
};

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
