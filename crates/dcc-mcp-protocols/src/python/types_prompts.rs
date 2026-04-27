//! PyO3 bindings for prompt-related MCP types.

use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::gen_stub_pymethods;

use crate::types::{PromptArgument, PromptDefinition};

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
