//! Internal helper functions for Python binding conversions.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

/// Parse a UUID string, returning a `PyValueError` on failure.
#[cfg(feature = "python-bindings")]
pub(super) fn parse_uuid(s: &str) -> PyResult<uuid::Uuid> {
    uuid::Uuid::parse_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {}", e)))
}

/// Convert a [`crate::session::Session`] into a Python dict.
#[cfg(feature = "python-bindings")]
pub(super) fn session_to_py(py: Python, session: &crate::session::Session) -> Py<PyAny> {
    let dict = PyDict::new(py);
    let _ = dict.set_item("id", session.id.to_string());
    let _ = dict.set_item("dcc_type", &session.dcc_type);
    let _ = dict.set_item("instance_id", session.instance_id.to_string());
    let _ = dict.set_item("host", &session.host);
    let _ = dict.set_item("port", session.port);
    let _ = dict.set_item("transport_address", session.address.to_connection_string());
    let _ = dict.set_item("state", session.state.to_string());
    let _ = dict.set_item("request_count", session.metrics.request_count);
    let _ = dict.set_item("error_count", session.metrics.error_count);
    let _ = dict.set_item("avg_latency_ms", session.metrics.avg_latency_ms());
    let _ = dict.set_item("error_rate", session.metrics.error_rate());
    let _ = dict.set_item("reconnect_attempts", session.reconnect_attempts);
    dict.unbind().into_any()
}
