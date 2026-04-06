//! Python bindings for wire protocol message codec functions.
//!
//! Exposes `encode_request`, `encode_response`, `encode_notify`, and
//! `decode_envelope` as Python-callable functions so that DCC-side Python
//! servers (e.g. dcc-mcp-rpyc) can build and parse framed messages without
//! depending on the full Rust transport stack.
//!
//! ## Wire format
//!
//! Every encoded message is a `bytes` object in the form:
//!
//! ```text
//! [4-byte big-endian length][MessagePack payload]
//! ```
//!
//! The length prefix is included in the returned `bytes` so callers can write
//! the buffer directly to a socket.
//!
//! ## DCC-side usage (Python)
//!
//! ```python,ignore
//! from dcc_mcp_core import encode_request, encode_response, encode_notify, decode_envelope
//!
//! # Server receives raw bytes from the socket, decodes them:
//! envelope = decode_envelope(raw_bytes_without_length_prefix)
//! if envelope["type"] == "request":
//!     req_id = envelope["id"]
//!     method = envelope["method"]
//!     params = envelope["params"]   # bytes
//!
//!     # Build and send a response:
//!     response_bytes = encode_response(req_id, success=True, payload=b"result")
//!     socket.sendall(response_bytes)
//!
//! # Client side: build a request frame
//! frame = encode_request("execute_python", b'cmds.sphere()')
//! socket.sendall(frame)
//!
//! # One-way notification
//! frame = encode_notify("scene_changed", b"")
//! socket.sendall(frame)
//! ```

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;
#[cfg(feature = "python-bindings")]
use pyo3::types::PyDict;

#[cfg(feature = "python-bindings")]
use uuid::Uuid;

#[cfg(feature = "python-bindings")]
use crate::message::{
    MessageEnvelope, Notification, Request, Response, decode_message, encode_message,
};

// ── encode helpers ─────────────────────────────────────────────────────────

/// Encode a ``Request`` message into a length-prefixed frame.
///
/// Returns ``bytes`` in the format ``[4-byte BE length][MessagePack payload]``
/// ready to write directly to a socket.
///
/// Args:
///     method:  Method name (e.g. ``"execute_python"``).
///     params:  Serialised parameters as bytes. Defaults to empty bytes.
///
/// Returns:
///     ``bytes`` — the framed message.
///
/// Raises:
///     RuntimeError: If serialisation fails.
///
/// Example:
///
/// ```python,ignore
/// from dcc_mcp_core import encode_request
///
/// frame = encode_request("execute_python", b'cmds.sphere()')
/// socket.sendall(frame)
/// ```
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "encode_request", signature = (method, params = None))]
pub fn py_encode_request(method: &str, params: Option<Vec<u8>>) -> PyResult<Vec<u8>> {
    let req = Request {
        id: Uuid::new_v4(),
        method: method.to_string(),
        params: params.unwrap_or_default(),
    };
    let envelope = MessageEnvelope::Request(req);
    encode_message(&envelope).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

/// Encode a ``Response`` message into a length-prefixed frame.
///
/// Args:
///     request_id: UUID string of the correlated request.
///     success:    Whether the request succeeded.
///     payload:    Serialised result bytes. Defaults to empty bytes.
///     error:      Optional error message string (use when ``success=False``).
///
/// Returns:
///     ``bytes`` — the framed message.
///
/// Raises:
///     RuntimeError: If serialisation fails.
///     ValueError:   If ``request_id`` is not a valid UUID string.
///
/// Example:
///
/// ```python,ignore
/// from dcc_mcp_core import encode_response
///
/// frame = encode_response(req_id, success=True, payload=b"result")
/// socket.sendall(frame)
/// ```
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "encode_response", signature = (request_id, success, payload = None, error = None))]
pub fn py_encode_response(
    request_id: &str,
    success: bool,
    payload: Option<Vec<u8>>,
    error: Option<String>,
) -> PyResult<Vec<u8>> {
    let id = Uuid::parse_str(request_id)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid UUID: {e}")))?;
    let resp = Response {
        id,
        success,
        payload: payload.unwrap_or_default(),
        error,
    };
    let envelope = MessageEnvelope::Response(resp);
    encode_message(&envelope).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

/// Encode a ``Notify`` (one-way event) message into a length-prefixed frame.
///
/// Args:
///     topic:  Event topic string (e.g. ``"scene_changed"``).
///     data:   Serialised event data bytes. Defaults to empty bytes.
///
/// Returns:
///     ``bytes`` — the framed message.
///
/// Raises:
///     RuntimeError: If serialisation fails.
///
/// Example:
///
/// ```python,ignore
/// from dcc_mcp_core import encode_notify
///
/// frame = encode_notify("render_complete", b"")
/// socket.sendall(frame)
/// ```
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "encode_notify", signature = (topic, data = None))]
pub fn py_encode_notify(topic: &str, data: Option<Vec<u8>>) -> PyResult<Vec<u8>> {
    let notif = Notification {
        id: None,
        topic: topic.to_string(),
        data: data.unwrap_or_default(),
    };
    let envelope = MessageEnvelope::Notify(notif);
    encode_message(&envelope).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}

// ── decode helper ──────────────────────────────────────────────────────────

/// Decode a MessagePack payload (WITHOUT length prefix) into a message dict.
///
/// This is the inverse of ``encode_request`` / ``encode_response`` / ``encode_notify``,
/// but the caller must strip the 4-byte length prefix before passing the data.
///
/// The returned dict always has a ``"type"`` key. Additional fields depend on
/// the variant:
///
/// - ``"request"``:  ``"id"`` (str), ``"method"`` (str), ``"params"`` (bytes)
/// - ``"response"``: ``"id"`` (str), ``"success"`` (bool), ``"payload"`` (bytes), ``"error"`` (str or None)
/// - ``"notify"``:   ``"id"`` (str or None), ``"topic"`` (str), ``"data"`` (bytes)
/// - ``"ping"``:     ``"id"`` (str), ``"timestamp_ms"`` (int)
/// - ``"pong"``:     ``"id"`` (str), ``"timestamp_ms"`` (int)
/// - ``"shutdown"``: ``"reason"`` (str or None)
///
/// Args:
///     data: Raw MessagePack bytes (length prefix already stripped).
///
/// Returns:
///     ``dict`` with ``"type"`` and variant-specific fields.
///
/// Raises:
///     RuntimeError: If ``data`` cannot be decoded as a valid ``MessageEnvelope``.
///
/// Example:
///
/// ```python,ignore
/// from dcc_mcp_core import decode_envelope
///
/// # Assume `raw` is what was received from the socket (after reading 4-byte length)
/// msg = decode_envelope(raw)
/// if msg["type"] == "request":
///     print(msg["method"], msg["params"])
/// ```
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "decode_envelope")]
pub fn py_decode_envelope(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let envelope: MessageEnvelope = decode_message(data)
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

    envelope_to_py_dict(py, envelope)
}

// ── internal helpers ───────────────────────────────────────────────────────

/// Convert a [`MessageEnvelope`] to a Python ``dict``.
#[cfg(feature = "python-bindings")]
fn envelope_to_py_dict(py: Python<'_>, envelope: MessageEnvelope) -> PyResult<Py<PyAny>> {
    let d = PyDict::new(py);
    match envelope {
        MessageEnvelope::Request(req) => {
            d.set_item("type", "request")?;
            d.set_item("id", req.id.to_string())?;
            d.set_item("method", req.method)?;
            d.set_item("params", req.params.as_slice())?;
        }
        MessageEnvelope::Response(resp) => {
            d.set_item("type", "response")?;
            d.set_item("id", resp.id.to_string())?;
            d.set_item("success", resp.success)?;
            d.set_item("payload", resp.payload.as_slice())?;
            match resp.error {
                Some(e) => d.set_item("error", e)?,
                None => d.set_item("error", py.None())?,
            }
        }
        MessageEnvelope::Notify(notif) => {
            d.set_item("type", "notify")?;
            match notif.id {
                Some(id) => d.set_item("id", id.to_string())?,
                None => d.set_item("id", py.None())?,
            }
            d.set_item("topic", notif.topic)?;
            d.set_item("data", notif.data.as_slice())?;
        }
        MessageEnvelope::Ping(ping) => {
            d.set_item("type", "ping")?;
            d.set_item("id", ping.id.to_string())?;
            d.set_item("timestamp_ms", ping.timestamp_ms)?;
        }
        MessageEnvelope::Pong(pong) => {
            d.set_item("type", "pong")?;
            d.set_item("id", pong.id.to_string())?;
            d.set_item("timestamp_ms", pong.timestamp_ms)?;
        }
        MessageEnvelope::Shutdown(msg) => {
            d.set_item("type", "shutdown")?;
            match msg.reason {
                Some(r) => d.set_item("reason", r)?,
                None => d.set_item("reason", py.None())?,
            }
        }
    }
    Ok(d.unbind().into_any())
}

// ── register ───────────────────────────────────────────────────────────────

/// Register all message codec functions into the given module.
#[cfg(feature = "python-bindings")]
pub fn register_functions(m: &Bound<'_, pyo3::types::PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(py_encode_request, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(py_encode_response, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(py_encode_notify, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(py_decode_envelope, m)?)?;
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // These tests verify the internal helpers only (no Python interpreter needed).
    // The full encode/decode roundtrip is covered by message.rs tests.
    // The Python-facing API is covered by test_transport.py (TestMessageCodec).

    use crate::message::{MessageEnvelope, encode_message};
    use uuid::Uuid;

    mod encode_decode {
        use super::*;

        #[test]
        fn test_encode_message_request_has_length_prefix() {
            let req = crate::message::Request {
                id: Uuid::new_v4(),
                method: "test".to_string(),
                params: b"params".to_vec(),
            };
            let envelope = MessageEnvelope::Request(req);
            let encoded = encode_message(&envelope).unwrap();

            // Must have at least 4 bytes for the length prefix.
            assert!(encoded.len() >= 4);

            // Length prefix must match the payload size.
            let len = u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
            assert_eq!(len, encoded.len() - 4);
        }

        #[test]
        fn test_decode_request_envelope_roundtrip() {
            let req = crate::message::Request {
                id: Uuid::new_v4(),
                method: "execute_python".to_string(),
                params: b"cmds.sphere()".to_vec(),
            };
            let envelope = MessageEnvelope::Request(req.clone());
            let encoded = encode_message(&envelope).unwrap();

            // Strip the 4-byte length prefix before decoding.
            let decoded: MessageEnvelope = crate::message::decode_message(&encoded[4..]).unwrap();

            match decoded {
                MessageEnvelope::Request(decoded_req) => {
                    assert_eq!(decoded_req.id, req.id);
                    assert_eq!(decoded_req.method, req.method);
                    assert_eq!(decoded_req.params, req.params);
                }
                _ => panic!("expected Request envelope"),
            }
        }

        #[test]
        fn test_decode_response_envelope_roundtrip() {
            let id = Uuid::new_v4();
            let resp = crate::message::Response {
                id,
                success: true,
                payload: b"result".to_vec(),
                error: None,
            };
            let envelope = MessageEnvelope::Response(resp);
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = crate::message::decode_message(&encoded[4..]).unwrap();

            match decoded {
                MessageEnvelope::Response(r) => {
                    assert_eq!(r.id, id);
                    assert!(r.success);
                    assert_eq!(r.payload, b"result");
                    assert!(r.error.is_none());
                }
                _ => panic!("expected Response envelope"),
            }
        }

        #[test]
        fn test_decode_notify_envelope_roundtrip() {
            let notif = crate::message::Notification {
                id: None,
                topic: "scene_changed".to_string(),
                data: b"event_data".to_vec(),
            };
            let envelope = MessageEnvelope::Notify(notif);
            let encoded = encode_message(&envelope).unwrap();
            let decoded: MessageEnvelope = crate::message::decode_message(&encoded[4..]).unwrap();

            match decoded {
                MessageEnvelope::Notify(n) => {
                    assert!(n.id.is_none());
                    assert_eq!(n.topic, "scene_changed");
                    assert_eq!(n.data, b"event_data");
                }
                _ => panic!("expected Notify envelope"),
            }
        }

        #[test]
        fn test_decode_invalid_bytes_errors() {
            let bad_data = b"not valid msgpack";
            let result: Result<MessageEnvelope, _> = crate::message::decode_message(bad_data);
            assert!(result.is_err());
        }
    }
}
