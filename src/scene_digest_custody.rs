//! Native custody for before-state evidence during in-process Python execution.
//!
//! Arbitrary host scripts can inspect every Python frame in their interpreter.
//! The before-state snapshot therefore cannot remain in a Python local or
//! closure while the script runs.  This module parses and owns the serialized
//! snapshot on the Rust stack, invokes the script callback, and only then
//! materializes an immutable Python-facing evidence object.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Host-owned scene evidence released only after the script callback returns.
#[pyclass(frozen, module = "dcc_mcp_core._core", name = "_SceneDigestEvidence")]
pub(crate) struct PySceneDigestEvidence {
    snapshot: serde_json::Value,
    fingerprint: String,
    payload: serde_json::Value,
    truncated: bool,
    schema_version: String,
    integrity: String,
}

impl PySceneDigestEvidence {
    fn from_wire(wire: &[u8]) -> PyResult<Self> {
        let snapshot: serde_json::Value = serde_json::from_slice(wire)
            .map_err(|_| PyValueError::new_err("serialized scene digest evidence is malformed"))?;
        let object = snapshot.as_object().ok_or_else(|| {
            PyValueError::new_err("serialized scene digest evidence must be an object")
        })?;
        let string_field = |name: &str| -> PyResult<String> {
            object
                .get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    PyValueError::new_err(format!(
                        "serialized scene digest evidence is missing {name}"
                    ))
                })
        };
        let payload = object
            .get("payload")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| {
                PyValueError::new_err("serialized scene digest evidence is missing payload")
            })?;
        let truncated = object
            .get("truncated")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| {
                PyValueError::new_err("serialized scene digest evidence is missing truncated")
            })?;

        Ok(Self {
            fingerprint: string_field("fingerprint")?,
            schema_version: string_field("schema_version")?,
            integrity: string_field("integrity")?,
            payload,
            truncated,
            snapshot,
        })
    }

    fn value_to_pyobject(py: Python<'_>, value: &serde_json::Value) -> PyResult<Py<PyAny>> {
        dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, value)
    }
}

#[pymethods]
impl PySceneDigestEvidence {
    #[getter]
    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[getter]
    fn payload(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Self::value_to_pyobject(py, &self.payload)
    }

    #[getter]
    fn truncated(&self) -> bool {
        self.truncated
    }

    #[getter]
    fn schema_version(&self) -> &str {
        &self.schema_version
    }

    #[getter]
    fn integrity(&self) -> &str {
        &self.integrity
    }

    /// Return a fresh JSON-safe copy of the trusted snapshot.
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Self::value_to_pyobject(py, &self.snapshot)
    }

    /// Native custody is immutable; construction already validated the wire shape.
    fn validate(&self) {}
}

/// Hold one before-state snapshot outside Python while invoking a script callback.
#[pyfunction(name = "_run_with_scene_digest_custody")]
pub(crate) fn py_run_with_scene_digest_custody(
    py: Python<'_>,
    before_wire: &[u8],
    callback: &Bound<'_, PyAny>,
    code: &str,
    filename: &str,
) -> PyResult<(Py<PyAny>, Py<PySceneDigestEvidence>)> {
    // Parse and move every trusted byte into Rust before entering arbitrary
    // Python.  No Python frame or closure owns the evidence during callback.
    let evidence = PySceneDigestEvidence::from_wire(before_wire)?;
    let callback_result = callback.call1((code, filename))?.unbind();
    Ok((callback_result, Py::new(py, evidence)?))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySceneDigestEvidence>()?;
    m.add_function(wrap_pyfunction!(py_run_with_scene_digest_custody, m)?)?;
    Ok(())
}
