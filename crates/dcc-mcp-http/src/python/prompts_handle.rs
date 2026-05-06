//! PyO3 binding for [`crate::prompts::PromptRegistry`] (issue #792).
//!
//! Exposes a minimal Python API to register and clear prompts on the
//! `PromptRegistry` owned by the running `McpHttpServer`. Mirrors the
//! `ResourceHandle` pattern so Python embedders can publish prompts
//! without touching Rust code.
//!
//! # Surface
//!
//! Obtained via [`crate::python::PyMcpHttpServer::prompts`]:
//!
//! ```python
//! server = McpHttpServer(registry, McpHttpConfig(port=8765))
//! handle = server.prompts()
//!
//! handle.register_prompt(
//!     name="bake_animation",
//!     description="Bake animation across frame range",
//!     template="Please bake animation from {{start}} to {{end}}",
//!     arguments=[
//!         {"name": "start", "required": True},
//!         {"name": "end",   "required": True},
//!     ],
//! )
//!
//! # prompts/list now includes "bake_animation"
//! # prompts/get with args {"start":"1","end":"100"} renders template
//!
//! handle.unregister_prompt("bake_animation")
//! handle.clear()
//! ```

use std::collections::HashSet;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::prompts::{PromptArgumentSpec, PromptEntry, PromptRegistry, PromptSource};

/// Python-facing handle to the server's [`PromptRegistry`].
///
/// Obtained via [`crate::python::PyMcpHttpServer::prompts`]. The
/// underlying registry is shared with the running server, so the next
/// `prompts/list` and `prompts/get` calls reflect registered prompts.
#[pyclass(name = "PromptHandle", skip_from_py_object)]
pub struct PyPromptHandle {
    pub(crate) inner: PromptRegistry,
}

impl PyPromptHandle {
    pub(crate) fn new(registry: PromptRegistry) -> Self {
        Self { inner: registry }
    }
}

#[pymethods]
impl PyPromptHandle {
    /// Register (or overwrite) a prompt.
    ///
    /// Args:
    ///     name: Prompt name shown in `prompts/list`.
    ///     template: String with ``{{arg_name}}`` placeholders.
    ///     description: Optional human-readable description.
    ///     arguments: Optional list of ``{"name": str, "description": str,
    ///         "required": bool}`` dicts. Defaults to ``[]``.
    ///
    /// Raises:
    ///     ValueError: If `name` or `template` is empty, or if
    ///         `arguments` is not a ``list`` of ``dict``.
    #[pyo3(signature = (name, template, description=None, arguments=None))]
    fn register_prompt(
        &self,
        py: Python<'_>,
        name: String,
        template: String,
        description: Option<String>,
        arguments: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "register_prompt: 'name' must not be empty",
            ));
        }
        if template.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "register_prompt: 'template' must not be empty",
            ));
        }

        let mut argspec: Vec<PromptArgumentSpec> = Vec::new();
        let mut seen_args: HashSet<String> = HashSet::new();
        if let Some(obj) = arguments {
            let list = obj.bind(py).cast::<PyList>().map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "register_prompt: 'arguments' must be a list: {e}"
                ))
            })?;
            for item in list.iter() {
                let dict = item.cast::<PyDict>().map_err(|e| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "register_prompt: each argument must be a dict: {e}"
                    ))
                })?;
                let arg_name: String = dict
                    .get_item("name")?
                    .ok_or_else(|| {
                        pyo3::exceptions::PyValueError::new_err(
                            "register_prompt: argument dict must contain 'name'",
                        )
                    })?
                    .extract()?;
                let arg_name = arg_name.trim().to_string();
                if arg_name.is_empty() {
                    return Err(pyo3::exceptions::PyValueError::new_err(
                        "register_prompt: argument 'name' must not be empty",
                    ));
                }
                if !seen_args.insert(arg_name.clone()) {
                    return Err(pyo3::exceptions::PyValueError::new_err(format!(
                        "register_prompt: duplicate argument name: {arg_name}"
                    )));
                }
                let description: Option<String> = dict
                    .get_item("description")?
                    .map(|v| v.extract())
                    .transpose()?;
                let required: bool = dict
                    .get_item("required")?
                    .map(|v| v.extract())
                    .transpose()?
                    .unwrap_or(false);
                argspec.push(PromptArgumentSpec {
                    name: arg_name,
                    description,
                    required,
                });
            }
        }

        let entry = PromptEntry {
            name: name.clone(),
            description,
            arguments: argspec,
            template,
            source: PromptSource::Explicit,
            skill: "manual".to_string(),
        };

        self.inner.register_prompt("manual", entry);
        Ok(())
    }

    /// Remove a previously registered prompt.
    ///
    /// Args:
    ///     name: Prompt name passed to `register_prompt`.
    ///
    /// No-op if the prompt was not found.
    fn unregister_prompt(&self, name: &str) {
        self.inner.unregister_prompt("manual", name);
    }

    /// Remove every manually registered prompt on this server.
    fn clear(&self) {
        self.inner.clear_manual_for_skill("manual");
    }
}

// ── Module registration ───────────────────────────────────────────────────────

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPromptHandle>()?;
    Ok(())
}
