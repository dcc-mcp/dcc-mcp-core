//! PyO3 binding for [`crate::resources::ResourceRegistry`] (issue #730).
//!
//! Exposes the mutating surface of the Rust-side `ResourceRegistry` to
//! Python adapters embedding `dcc-mcp-core` via PyO3. Without this
//! binding, embedders (`dcc-mcp-maya`, Blender, Houdini, ...) could not
//! push scene snapshots, register custom producers, wire an
//! `OutputBuffer`, or emit `resources/updated` for URIs they own — even
//! though the Rust API fully supports all of this.
//!
//! # Surface
//!
//! Obtained via [`crate::python::PyMcpHttpServer::resources`]:
//!
//! ```python
//! server = McpHttpServer(registry, McpHttpConfig(port=8765))
//! handle = server.resources()
//!
//! handle.set_scene({"nodes": [...]})              # scene://current
//! handle.notify_updated("scene://current")        # kick SSE subscribers
//! handle.register_output_buffer(capture)          # output://instance/...
//! handle.register_producer(                       # any-scheme producer
//!     "docs://",
//!     lambda uri: {"mimeType": "text/markdown", "text": "..."},
//! )
//! ```

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use crate::protocol::McpResource;
use crate::resources::{
    ProducerContent, ResourceError, ResourceProducer, ResourceRegistry, ResourceResult,
};

// ── PythonProducer ────────────────────────────────────────────────────────────

/// [`ResourceProducer`] backed by a Python callable.
///
/// The callable is invoked from a Tokio worker thread; thread-affinity
/// (e.g. running the body on the DCC main thread) is the Python caller's
/// responsibility — this struct is only the bridge.
///
/// Expected return shape from the callable:
///
/// - `{"mimeType": str, "text": str}` → [`ProducerContent::Text`]
/// - `{"mimeType": str, "blob": bytes}` → [`ProducerContent::Blob`]
///
/// `mimeType` defaults to `"application/octet-stream"` for blobs and
/// `"text/plain"` for text if the key is missing.
pub(crate) struct PythonProducer {
    scheme: String,
    uri_prefix: String,
    callable: Py<PyAny>,
}

impl PythonProducer {
    /// Build a producer for `scheme_or_uri`.
    ///
    /// `"docs"`, `"docs:"`, `"docs://"`, or `"docs://anything"` all yield
    /// the scheme `"docs"`. The original string is kept as the list URI
    /// prefix so embedders can pin a producer to a specific URI.
    fn new(scheme_or_uri: &str, callable: Py<PyAny>) -> Self {
        let scheme = extract_scheme(scheme_or_uri);
        let uri_prefix = if scheme_or_uri.contains(':') {
            scheme_or_uri.to_string()
        } else {
            format!("{}://", scheme)
        };
        Self {
            scheme,
            uri_prefix,
            callable,
        }
    }
}

fn extract_scheme(s: &str) -> String {
    match s.find(':') {
        Some(idx) => s[..idx].to_string(),
        None => s.to_string(),
    }
}

impl ResourceProducer for PythonProducer {
    fn scheme(&self) -> &str {
        &self.scheme
    }

    fn list(&self) -> Vec<McpResource> {
        // The callable is the source of truth for reads, but `resources/list`
        // must be non-blocking and deterministic — surface a single entry
        // matching the registered prefix so agents can discover the scheme
        // without having to speculatively invoke the callable.
        vec![McpResource {
            uri: self.uri_prefix.clone(),
            name: format!("Python-provided resource ({})", self.scheme),
            description: Some(format!(
                "Resources served by a Python-registered producer for scheme `{}://`.",
                self.scheme
            )),
            mime_type: None,
        }]
    }

    fn read(&self, uri: &str) -> ResourceResult<ProducerContent> {
        Python::attach(|py| {
            let result = self
                .callable
                .call1(py, (uri,))
                .map_err(|e| ResourceError::Read(format!("python producer error: {e}")))?;
            let bound = result.bind(py);
            decode_producer_return(bound, uri)
        })
    }
}

/// Turn the Python callable's return value into a [`ProducerContent`].
///
/// Extracted so the pure-decoding logic can be unit-tested without
/// going through the `ResourceProducer` trait.
fn decode_producer_return(bound: &Bound<'_, PyAny>, uri: &str) -> ResourceResult<ProducerContent> {
    let dict = bound
        .cast::<PyDict>()
        .map_err(|e| ResourceError::Read(format!("producer return must be a dict: {e}")))?;

    // Prefer blob over text so binary producers that also set `text=""`
    // as a placeholder don't accidentally become text content.
    if let Some(blob_any) = dict
        .get_item("blob")
        .map_err(|e| ResourceError::Read(format!("dict lookup: {e}")))?
    {
        let bytes: Vec<u8> = blob_any
            .extract()
            .map_err(|e| ResourceError::Read(format!("producer 'blob' must be bytes-like: {e}")))?;
        let mime_type = dict
            .get_item("mimeType")
            .map_err(|e| ResourceError::Read(format!("dict lookup: {e}")))?
            .map(|v| v.extract::<String>())
            .transpose()
            .map_err(|e| ResourceError::Read(format!("producer 'mimeType' must be str: {e}")))?
            .unwrap_or_else(|| "application/octet-stream".to_string());
        return Ok(ProducerContent::Blob {
            uri: uri.to_string(),
            mime_type,
            bytes,
        });
    }

    let text_any = dict
        .get_item("text")
        .map_err(|e| ResourceError::Read(format!("dict lookup: {e}")))?
        .ok_or_else(|| {
            ResourceError::Read("producer return must contain either 'text' or 'blob'".to_string())
        })?;
    let text: String = text_any
        .extract()
        .map_err(|e| ResourceError::Read(format!("producer 'text' must be str: {e}")))?;
    let mime_type = dict
        .get_item("mimeType")
        .map_err(|e| ResourceError::Read(format!("dict lookup: {e}")))?
        .map(|v| v.extract::<String>())
        .transpose()
        .map_err(|e| ResourceError::Read(format!("producer 'mimeType' must be str: {e}")))?
        .unwrap_or_else(|| "text/plain".to_string());
    Ok(ProducerContent::Text {
        uri: uri.to_string(),
        mime_type,
        text,
    })
}

// ── PyResourceHandle ──────────────────────────────────────────────────────────

/// Python-facing handle to the server's [`ResourceRegistry`].
///
/// Obtained via [`crate::python::PyMcpHttpServer::resources`]. The
/// underlying registry is shared with the running server, so mutations
/// take effect immediately — `resources/list` and `resources/read`
/// reflect new producers, scene snapshots, and output buffers without
/// requiring a restart.
#[pyclass(name = "ResourceHandle", skip_from_py_object)]
pub struct PyResourceHandle {
    pub(crate) inner: ResourceRegistry,
}

impl PyResourceHandle {
    pub(crate) fn new(registry: ResourceRegistry) -> Self {
        Self { inner: registry }
    }
}

#[pymethods]
impl PyResourceHandle {
    /// Publish a new scene snapshot for ``scene://current``.
    ///
    /// Fires ``notifications/resources/updated`` for subscribed clients.
    ///
    /// Args:
    ///     value: A ``dict``/``list``/scalar that is JSON-serialisable,
    ///         or a pre-serialised JSON ``str``. Strings are parsed to
    ///         preserve structured content — use
    ///         ``handle.set_scene({"raw": the_string})`` to publish a
    ///         literal string payload.
    ///
    /// Raises:
    ///     ValueError: If ``value`` cannot be converted to JSON.
    fn set_scene(&self, py: Python<'_>, value: Py<PyAny>) -> PyResult<()> {
        let bound = value.bind(py);
        let json: Value = if let Ok(s) = bound.extract::<String>() {
            // Accept pre-serialised JSON for callers that already
            // hold a string — common when the adapter is forwarding
            // bytes from a scene-export tool.
            serde_json::from_str(&s).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "set_scene: string payload must be valid JSON: {e}"
                ))
            })?
        } else {
            dcc_mcp_pybridge::py_json::py_any_to_json_value(bound).map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "set_scene: value is not JSON-serialisable: {e}"
                ))
            })?
        };
        self.inner.set_scene(json);
        Ok(())
    }

    /// Emit ``notifications/resources/updated`` for ``uri``.
    ///
    /// Use this when the adapter has already mutated some state that
    /// backs a custom producer and just needs to kick SSE subscribers.
    fn notify_updated(&self, uri: &str) {
        self.inner.notify_updated(uri);
    }

    /// Register an :class:`OutputCapture`'s underlying buffer as an
    /// ``output://instance/{instance_id}`` resource (issue #461).
    ///
    /// After this call, ``resources/list`` advertises the buffer and
    /// ``resources/read output://instance/{instance_id}`` returns the
    /// buffered lines.
    ///
    /// Args:
    ///     capture: An :class:`OutputCapture` instance. The capture
    ///         continues to own its buffer — the registry keeps an
    ///         ``Arc``-cloned reference so push/drain on the Python
    ///         side remain visible to MCP clients.
    fn register_output_buffer(&self, capture: PyRef<'_, super::PyOutputCapture>) {
        self.inner
            .register_output_buffer(capture.inner.buffer.clone());
    }

    /// Register a Python callable as a producer for ``scheme_or_uri``.
    ///
    /// The callable is invoked on a Tokio worker thread when an MCP
    /// client calls ``resources/read`` for a URI whose scheme matches.
    /// Thread-affinity (e.g. running the body on the DCC main thread)
    /// is the caller's responsibility — this API is only the bridge.
    ///
    /// Args:
    ///     scheme_or_uri: Either a bare scheme (``"maya-cmds"``) or a
    ///         full URI prefix (``"maya-cmds://"``). The value is used
    ///         both to dispatch reads by scheme and as the ``uri``
    ///         field in ``resources/list``.
    ///     callable: A Python callable of signature ``(uri: str) -> dict``.
    ///         The returned dict must contain either
    ///         ``{"mimeType": str, "text": str}`` or
    ///         ``{"mimeType": str, "blob": bytes}``. ``mimeType`` is
    ///         optional and defaults to ``"text/plain"`` or
    ///         ``"application/octet-stream"``.
    ///
    /// Raises:
    ///     TypeError: If ``callable`` is not callable.
    ///     ValueError: If ``scheme_or_uri`` is empty.
    fn register_producer(
        &self,
        py: Python<'_>,
        scheme_or_uri: &str,
        callable: Py<PyAny>,
    ) -> PyResult<()> {
        if scheme_or_uri.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "scheme_or_uri must not be empty",
            ));
        }
        if !callable.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "callable must be callable",
            ));
        }
        let producer = Arc::new(PythonProducer::new(scheme_or_uri, callable));
        self.inner.add_producer(producer);
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!("ResourceHandle(enabled={})", self.inner.is_enabled())
    }
}

// ── Module registration ───────────────────────────────────────────────────────

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyResourceHandle>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::{PyBytes, PyDict};

    #[test]
    fn extract_scheme_handles_uri_and_bare_scheme() {
        assert_eq!(extract_scheme("docs"), "docs");
        assert_eq!(extract_scheme("docs:"), "docs");
        assert_eq!(extract_scheme("docs://"), "docs");
        assert_eq!(extract_scheme("docs://foo/bar"), "docs");
    }

    #[test]
    fn python_producer_new_uri_prefix() {
        Python::attach(|py| {
            let cb = py
                .eval(
                    c"lambda uri: {'mimeType': 'text/plain', 'text': uri}",
                    None,
                    None,
                )
                .unwrap()
                .unbind();
            let prod = PythonProducer::new("docs://custom/foo", cb);
            assert_eq!(prod.scheme(), "docs");
            let listed = prod.list();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].uri, "docs://custom/foo");
        });
    }

    #[test]
    fn python_producer_new_bare_scheme_prefix() {
        Python::attach(|py| {
            let cb = py
                .eval(c"lambda uri: {'text': 'x'}", None, None)
                .unwrap()
                .unbind();
            let prod = PythonProducer::new("maya-cmds", cb);
            assert_eq!(prod.scheme(), "maya-cmds");
            assert_eq!(prod.list()[0].uri, "maya-cmds://");
        });
    }

    #[test]
    fn decode_text_uses_default_mime() {
        Python::attach(|py| {
            let d = PyDict::new(py);
            d.set_item("text", "hello").unwrap();
            let got = decode_producer_return(d.as_any(), "foo://x").unwrap();
            match got {
                ProducerContent::Text {
                    uri,
                    mime_type,
                    text,
                } => {
                    assert_eq!(uri, "foo://x");
                    assert_eq!(mime_type, "text/plain");
                    assert_eq!(text, "hello");
                }
                _ => panic!("expected text"),
            }
        });
    }

    #[test]
    fn decode_blob_prefers_blob_over_text() {
        Python::attach(|py| {
            let d = PyDict::new(py);
            d.set_item("mimeType", "image/png").unwrap();
            d.set_item("blob", PyBytes::new(py, b"\x89PNG\x0d\x0a"))
                .unwrap();
            d.set_item("text", "").unwrap();
            let got = decode_producer_return(d.as_any(), "img://1").unwrap();
            match got {
                ProducerContent::Blob {
                    uri,
                    mime_type,
                    bytes,
                } => {
                    assert_eq!(uri, "img://1");
                    assert_eq!(mime_type, "image/png");
                    assert_eq!(bytes, b"\x89PNG\x0d\x0a");
                }
                _ => panic!("expected blob"),
            }
        });
    }

    #[test]
    fn decode_missing_text_and_blob_errors() {
        Python::attach(|py| {
            let d = PyDict::new(py);
            d.set_item("mimeType", "text/plain").unwrap();
            let result = decode_producer_return(d.as_any(), "foo://x");
            assert!(
                result.is_err(),
                "expected error, got {:?}",
                result.ok().map(|_| "content")
            );
            if let Err(e) = result {
                assert!(matches!(e, ResourceError::Read(_)));
            }
        });
    }

    // ── End-to-end: PyResourceHandle wired to a live ResourceRegistry ─────────

    fn enabled_registry() -> ResourceRegistry {
        ResourceRegistry::new(true, false)
    }

    #[test]
    fn set_scene_roundtrips_through_scene_current() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry.clone());

            let scene = PyDict::new(py);
            scene
                .set_item("nodes", pyo3::types::PyList::empty(py))
                .unwrap();
            scene.set_item("frame", 42i64).unwrap();
            handle.set_scene(py, scene.into_any().unbind()).unwrap();

            let read = registry.read("scene://current").expect("scene read");
            let blob = &read.contents[0];
            let text = blob.text.as_deref().expect("scene is text");
            // The SceneProducer round-trips via serde_json; key presence and
            // integer value are the stable invariants here.
            assert!(text.contains("\"nodes\""));
            assert!(text.contains("\"frame\""));
            assert!(text.contains("42"));
        });
    }

    #[test]
    fn set_scene_accepts_pre_serialised_json_string() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry.clone());

            let payload = "{\"count\":7}";
            handle
                .set_scene(py, payload.into_pyobject(py).unwrap().into_any().unbind())
                .unwrap();

            let read = registry.read("scene://current").unwrap();
            let text = read.contents[0].text.as_deref().unwrap();
            assert!(text.contains("\"count\""));
            assert!(text.contains("7"));
        });
    }

    #[test]
    fn register_producer_lists_and_invokes_callable() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry.clone());

            let cb = py
                .eval(
                    c"lambda uri: {'mimeType': 'text/plain', 'text': 'echo:' + uri}",
                    None,
                    None,
                )
                .unwrap()
                .unbind();
            handle.register_producer(py, "test-scheme://", cb).unwrap();

            let listed = registry.list();
            assert!(
                listed.iter().any(|r| r.uri == "test-scheme://"),
                "test-scheme producer should surface in resources/list"
            );

            let read = registry.read("test-scheme://hello").unwrap();
            let text = read.contents[0].text.as_deref().unwrap();
            assert_eq!(text, "echo:test-scheme://hello");
        });
    }

    #[test]
    fn register_producer_rejects_empty_scheme() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry);
            let cb = py
                .eval(c"lambda uri: {'text': ''}", None, None)
                .unwrap()
                .unbind();
            let err = handle.register_producer(py, "", cb).unwrap_err();
            assert!(err.is_instance_of::<pyo3::exceptions::PyValueError>(py));
        });
    }

    #[test]
    fn register_producer_rejects_non_callable() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry);
            let not_callable: Py<PyAny> = 42i64.into_pyobject(py).unwrap().into_any().unbind();
            let err = handle
                .register_producer(py, "x://", not_callable)
                .unwrap_err();
            assert!(err.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        });
    }

    #[test]
    fn register_output_buffer_surfaces_instance_uri() {
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry.clone());

            // Build a PyOutputCapture directly — its `new` is pub(crate),
            // which is visible from within this crate's test module.
            let inner = crate::output::OutputCapture::with_capacity("unit-test-inst", 1000);
            inner.push("stdout", "hello");
            let capture = super::super::output_dynamic::PyOutputCapture { inner };
            // PyO3 method receives a `PyRef<PyOutputCapture>` — mirror that
            // by registering the class and round-tripping through a Py<_>.
            let py_capture: Py<super::super::output_dynamic::PyOutputCapture> =
                Py::new(py, capture).unwrap();
            handle.register_output_buffer(py_capture.borrow(py));

            let listed = registry.list();
            assert!(
                listed
                    .iter()
                    .any(|r| r.uri == "output://instance/unit-test-inst"),
                "output:// producer should advertise instance URI"
            );
            let read = registry.read("output://instance/unit-test-inst").unwrap();
            let text = read.contents[0].text.as_deref().unwrap();
            assert!(text.contains("hello"));
        });
    }

    #[test]
    fn notify_updated_is_noop_without_subscribers() {
        let registry = enabled_registry();
        let handle = PyResourceHandle::new(registry);
        // Sanity: must not panic even though no SSE session is listening.
        handle.notify_updated("scene://current");
    }

    #[test]
    fn builtin_producers_still_work_after_custom_registration() {
        // Regression for issue #730 — adding a Python producer must not
        // shadow or break the built-in `scene://`, `capture://`,
        // `audit://`, `artefact://` producers that `ResourceRegistry::new`
        // wires up.
        Python::attach(|py| {
            let registry = enabled_registry();
            let handle = PyResourceHandle::new(registry.clone());
            let cb = py
                .eval(c"lambda uri: {'text': 'x'}", None, None)
                .unwrap()
                .unbind();
            handle.register_producer(py, "custom://", cb).unwrap();

            let listed = registry.list();
            let uris: Vec<_> = listed.iter().map(|r| r.uri.as_str()).collect();
            assert!(uris.iter().any(|u| u.starts_with("scene://")));
            assert!(uris.iter().any(|u| u.starts_with("capture://")));
            assert!(uris.iter().any(|u| u.starts_with("audit://")));
            assert!(uris.iter().any(|u| u.starts_with("custom://")));
        });
    }
}
