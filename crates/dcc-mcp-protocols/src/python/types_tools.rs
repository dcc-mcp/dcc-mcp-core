//! PyO3 bindings for tool-related MCP types.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::types::{ToolAnnotations, ToolDefinition};

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
