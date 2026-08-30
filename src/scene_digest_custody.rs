//! Native transaction custody for in-process Python scene observations.
//!
//! Arbitrary host scripts can inspect and mutate Python frames in their
//! interpreter. The provider object selected by the adapter is therefore
//! pinned by Rust for the whole before/script/after transaction. The extension
//! never brands caller-supplied bytes as trusted evidence: it captures both
//! wires itself and Python rehydrates them through the public
//! `SceneDigestSnapshot` contract after the transaction completes.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

fn capture_wire(
    py: Python<'_>,
    provider: &Py<PyAny>,
    capture_callback: &Bound<'_, PyAny>,
) -> PyResult<Py<PyBytes>> {
    let rendered = capture_callback.call1((provider.clone_ref(py),))?;
    let wire = rendered.extract::<Vec<u8>>()?;
    Ok(PyBytes::new(py, &wire).unbind())
}

fn exception_value(py: Python<'_>, error: PyErr) -> Py<PyAny> {
    error.value(py).clone().into_any().unbind()
}

/// Capture before and after through the same pinned provider around one script.
///
/// Callback failures, including Python `BaseException` subclasses, are held as
/// values until after-state capture has completed. The Python caller then maps
/// them into the stable `SceneDigestExecutionError` contract.
#[pyfunction(name = "_run_with_scene_digest_transaction")]
pub(crate) fn py_run_with_scene_digest_transaction(
    py: Python<'_>,
    provider: Py<PyAny>,
    capture_callback: &Bound<'_, PyAny>,
    script_callback: &Bound<'_, PyAny>,
    code: &str,
    filename: &str,
) -> PyResult<(
    bool,
    Py<PyAny>,
    Py<PyBytes>,
    Option<Py<PyBytes>>,
    Option<Py<PyAny>>,
)> {
    if !provider.bind(py).is_callable() {
        return Err(PyTypeError::new_err(
            "state digest provider must be callable",
        ));
    }
    let before = capture_wire(py, &provider, capture_callback)?;
    let (script_succeeded, value_or_error) = match script_callback.call1((code, filename)) {
        Ok(value) => (true, value.unbind()),
        Err(error) => (false, exception_value(py, error)),
    };
    let (after, readback_error) = match capture_wire(py, &provider, capture_callback) {
        Ok(wire) => (Some(wire), None),
        Err(error) => (None, Some(exception_value(py, error))),
    };

    Ok((
        script_succeeded,
        value_or_error,
        before,
        after,
        readback_error,
    ))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(py_run_with_scene_digest_transaction, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::exceptions::PyKeyboardInterrupt;
    use pyo3::ffi::c_str;
    use pyo3::types::PyDict;

    use super::*;

    static PYTHON_INIT: Once = Once::new();

    #[test]
    fn transaction_captures_after_before_returning_base_exception() {
        PYTHON_INIT.call_once(Python::initialize);
        Python::attach(|py| -> PyResult<()> {
            let locals = PyDict::new(py);
            py.run(
                c_str!(
                    r#"
state = {"objects": 0, "reads": 0}
def provider():
    state["reads"] += 1
    return state["objects"]
def capture(provider_handle):
    return str(provider_handle()).encode("ascii")
def script(code, filename):
    state["objects"] += 1
    raise KeyboardInterrupt("operator stop")
"#
                ),
                Some(&locals),
                Some(&locals),
            )?;
            let provider = locals.get_item("provider")?.expect("provider").unbind();
            let capture = locals.get_item("capture")?.expect("capture");
            let script = locals.get_item("script")?.expect("script");

            let (succeeded, error, before, after, readback_error) =
                py_run_with_scene_digest_transaction(
                    py, provider, &capture, &script, "ignored", "<test>",
                )?;

            assert!(!succeeded);
            assert!(error.bind(py).is_instance_of::<PyKeyboardInterrupt>());
            assert_eq!(before.bind(py).as_bytes(), b"0");
            assert_eq!(after.expect("after").bind(py).as_bytes(), b"1");
            assert!(readback_error.is_none());
            let state = locals.get_item("state")?.expect("state");
            assert_eq!(state.get_item("reads")?.extract::<usize>()?, 2);
            Ok(())
        })
        .expect("native scene digest transaction");
    }
}
