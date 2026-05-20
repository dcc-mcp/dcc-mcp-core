//! PyO3 bindings for adapter session event buffers.

use pyo3::prelude::*;
use serde_json::Value;

use dcc_mcp_http::session_events::{
    DEFAULT_SESSION_EVENT_CAPACITY, DEFAULT_SESSION_EVENT_MAX_MESSAGE_BYTES, SessionEventBuffer,
};
use dcc_mcp_http_types::session_events::{
    SessionEvent, SessionEventLevel, SessionEventReadOptions,
};

/// Python wrapper for a bounded adapter session/job event buffer.
///
/// Adapters append runtime events and register the buffer through
/// ``server.resources().register_session_event_buffer(buffer)`` so clients can
/// read ``events://session/{instance_id}`` by cursor.
#[pyclass(name = "SessionEventBuffer")]
pub struct PySessionEventBuffer {
    pub inner: SessionEventBuffer,
}

#[pymethods]
impl PySessionEventBuffer {
    #[new]
    #[pyo3(signature = (instance_id, maxlen=DEFAULT_SESSION_EVENT_CAPACITY, max_message_bytes=DEFAULT_SESSION_EVENT_MAX_MESSAGE_BYTES))]
    fn new(instance_id: String, maxlen: usize, max_message_bytes: usize) -> Self {
        Self {
            inner: SessionEventBuffer::with_limits(instance_id, maxlen, max_message_bytes),
        }
    }

    /// Append one event and return the stored event as a dict.
    #[pyo3(signature = (source, stream, message, level="info", session_id=None, tool_call_id=None, job_id=None, correlation_id=None, metadata=None))]
    fn append(
        &self,
        py: Python<'_>,
        source: String,
        stream: String,
        message: String,
        level: &str,
        session_id: Option<String>,
        tool_call_id: Option<String>,
        job_id: Option<String>,
        correlation_id: Option<String>,
        metadata: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let level = SessionEventLevel::parse(level).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!("unknown session event level: {level}"))
        })?;
        let metadata = match metadata {
            Some(value) => dcc_mcp_pybridge::py_json::py_any_to_json_value(value.bind(py))
                .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
            None => Value::Null,
        };
        let mut event = SessionEvent::new(source, stream, level, message);
        event.session_id = session_id;
        event.tool_call_id = tool_call_id;
        event.job_id = job_id;
        event.correlation_id = correlation_id;
        event.metadata = metadata;
        let stored = self.inner.append(event);
        let value = serde_json::to_value(stored)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, &value)
    }

    /// Read a cursor page as a dict.
    #[pyo3(signature = (cursor=0, limit=100, drain=false))]
    fn read(&self, py: Python<'_>, cursor: u64, limit: usize, drain: bool) -> PyResult<Py<PyAny>> {
        let page = self.inner.read(SessionEventReadOptions {
            cursor,
            limit,
            drain,
        });
        let value = serde_json::to_value(page)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
        dcc_mcp_pybridge::py_json::json_value_to_pyobject(py, &value)
    }

    /// The MCP resource URI: ``events://session/{instance_id}``.
    #[getter]
    fn resource_uri(&self) -> String {
        self.inner.resource_uri()
    }

    /// The DCC instance ID.
    #[getter]
    fn instance_id(&self) -> &str {
        self.inner.instance_id()
    }

    fn __repr__(&self) -> String {
        format!(
            "SessionEventBuffer(instance_id={:?})",
            self.inner.instance_id()
        )
    }
}

/// Register Python classes from this module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySessionEventBuffer>()?;
    Ok(())
}
