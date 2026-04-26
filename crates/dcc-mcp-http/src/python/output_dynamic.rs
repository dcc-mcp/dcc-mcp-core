//! PyO3 bindings for dynamic tool registration (issue #462) and
//! DCC output capture (issue #461).

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::dynamic_tools::ToolSpec;
use crate::output::OutputCapture;

// ── PyToolSpec ────────────────────────────────────────────────────────────────

/// A session-scoped tool definition for dynamic registration (issue #462).
///
/// Example::
///
///     from dcc_mcp_core import ToolSpec
///
///     spec = ToolSpec(
///         name="rename_spheres",
///         description="Rename all pSphere* transforms to hero_sphere*",
///         code="import pymel.core as pm\\nfor n in pm.ls('pSphere*'): n.rename('hero_sphere')",
///     )
///
/// Then register it::
///
///     # via MCP tools/call register_tool:
///     result = dispatcher.dispatch("register_tool", json.dumps({"tool_spec": spec.to_dict()}))
#[pyclass(name = "ToolSpec", skip_from_py_object)]
#[derive(Clone)]
pub struct PyToolSpec {
    pub inner: ToolSpec,
}

#[pymethods]
impl PyToolSpec {
    #[new]
    #[pyo3(signature = (
        name,
        description,
        code,
        language = "python",
        parameters = None,
        dcc = None,
        timeout_sec = 30,
        read_only_hint = true,
        destructive_hint = false,
        ttl_secs = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        description: String,
        code: String,
        language: &str,
        parameters: Option<String>,
        dcc: Option<String>,
        timeout_sec: u64,
        read_only_hint: bool,
        destructive_hint: bool,
        ttl_secs: Option<u64>,
    ) -> PyResult<Self> {
        let params_value = if let Some(s) = parameters {
            serde_json::from_str(&s).ok()
        } else {
            None
        };

        Ok(Self {
            inner: ToolSpec {
                name,
                description,
                code,
                language: language.to_string(),
                parameters: params_value,
                dcc,
                timeout_sec,
                read_only_hint,
                destructive_hint,
                ttl_secs,
            },
        })
    }

    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }

    #[getter]
    fn code(&self) -> &str {
        &self.inner.code
    }

    #[getter]
    fn language(&self) -> &str {
        &self.inner.language
    }

    #[getter]
    fn timeout_sec(&self) -> u64 {
        self.inner.timeout_sec
    }

    #[getter]
    fn read_only_hint(&self) -> bool {
        self.inner.read_only_hint
    }

    #[getter]
    fn destructive_hint(&self) -> bool {
        self.inner.destructive_hint
    }

    /// Serialize the ToolSpec to a JSON string suitable for `register_tool`.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
    }

    /// Return the ToolSpec as a Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let json = serde_json::to_value(&self.inner)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        json_value_to_pydict(py, &json)
    }

    fn __repr__(&self) -> String {
        format!(
            "ToolSpec(name={:?}, language={:?})",
            self.inner.name, self.inner.language
        )
    }
}

/// Convert a serde_json::Value object into a Python dict.
fn json_value_to_pydict<'py>(
    py: Python<'py>,
    v: &serde_json::Value,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    if let Some(obj) = v.as_object() {
        for (k, val) in obj {
            let py_val = json_value_to_py(py, val)?;
            d.set_item(k, py_val)?;
        }
    }
    Ok(d)
}

fn json_value_to_py<'py>(py: Python<'py>, v: &serde_json::Value) -> PyResult<Bound<'py, PyAny>> {
    match v {
        serde_json::Value::Null => Ok(py.None().into_bound(py)),
        serde_json::Value::Bool(b) => {
            let bound: &pyo3::Bound<'_, pyo3::types::PyBool> = &pyo3::types::PyBool::new(py, *b);
            Ok(bound.clone().into_any())
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any())
            } else {
                Ok(py.None().into_bound(py))
            }
        }
        serde_json::Value::String(s) => Ok(s.clone().into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(obj) => {
            let d = PyDict::new(py);
            for (k, v) in obj {
                d.set_item(k, json_value_to_py(py, v)?)?;
            }
            Ok(d.into_any())
        }
    }
}

// ── PyOutputCapture ───────────────────────────────────────────────────────────

/// Python wrapper for DCC output capture (issue #461).
///
/// Captures DCC stdout/stderr/script-editor output into a ring buffer and
/// exposes it as an MCP ``output://`` resource.
///
/// Usage::
///
///     from dcc_mcp_core import OutputCapture
///
///     capture = OutputCapture(instance_id="maya-001")
///
///     # Push lines manually (or redirect sys.stdout via Python):
///     capture.push("stdout", "INFO: scene loaded")
///     capture.push("stderr", "WARNING: deprecated node")
///
///     # Read buffered entries since epoch ns (0 = all):
///     entries = capture.drain(since_ns=0)
///     # [{"timestamp_ns": ..., "stream": "stdout", "text": "INFO: scene loaded"}, ...]
///
///     # The MCP resource URI (use with resources/read):
///     print(capture.resource_uri)  # "output://instance/maya-001"
///
/// Register with an MCP server::
///
///     from dcc_mcp_core import McpHttpServer, McpHttpConfig
///     registry = ...
///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
///     server.resources().register_output_buffer(capture._buffer)
///     handle = server.start()
#[pyclass(name = "OutputCapture")]
pub struct PyOutputCapture {
    pub inner: OutputCapture,
}

#[pymethods]
impl PyOutputCapture {
    #[new]
    #[pyo3(signature = (instance_id, maxlen = 1000))]
    fn new(instance_id: String, maxlen: usize) -> Self {
        Self {
            inner: OutputCapture::with_capacity(instance_id, maxlen),
        }
    }

    /// Push a captured text line.
    ///
    /// :param stream: ``"stdout"``, ``"stderr"``, or ``"script_editor"``.
    /// :param text: The text to buffer.
    fn push(&self, stream: &str, text: String) {
        self.inner.push(stream, text);
    }

    /// Return all buffered entries since `since_ns` nanoseconds (0 = all).
    ///
    /// :returns: List of dicts with keys ``timestamp_ns``, ``instance_id``,
    ///           ``stream``, and ``text``.
    fn drain<'py>(&self, py: Python<'py>, since_ns: u128) -> Bound<'py, PyList> {
        let entries = self.inner.drain(since_ns);
        let list = PyList::empty(py);
        for e in entries {
            let d = PyDict::new(py);
            let _ = d.set_item("timestamp_ns", e.timestamp_ns as u64);
            let _ = d.set_item("instance_id", &e.instance_id);
            let _ = d.set_item("stream", e.stream.as_str());
            let _ = d.set_item("text", &e.text);
            let _ = list.append(d);
        }
        list
    }

    /// The MCP resource URI: ``output://instance/{instance_id}``.
    #[getter]
    fn resource_uri(&self) -> String {
        self.inner.resource_uri()
    }

    /// The DCC instance ID.
    #[getter]
    fn instance_id(&self) -> &str {
        self.inner.buffer.instance_id()
    }

    fn __repr__(&self) -> String {
        format!(
            "OutputCapture(instance_id={:?})",
            self.inner.buffer.instance_id()
        )
    }
}

// ── Module registration ────────────────────────────────────────────────────────

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyToolSpec>()?;
    m.add_class::<PyOutputCapture>()?;
    Ok(())
}
