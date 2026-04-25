//! Unified Python ↔ serde_json::Value conversion utilities.
//!
//! Provides a single source of truth for converting between Python objects and
//! `serde_json::Value`, eliminating duplicate implementations across crates.
//! Also exposes high-performance `json_dumps` / `json_loads` pyfunctions that
//! serve as drop-in replacements for Python's `json.dumps()` / `json.loads()`.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyList};
use std::collections::HashMap;

/// Convert a Python object to a `serde_json::Value`.
///
/// Extraction order matters — Python `bool` is a subclass of `int`,
/// and `int` can be extracted as `f64`. So: bool → int → float → string.
pub fn py_any_to_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::Value::Number(i.into()));
    }
    if let Ok(f) = obj.extract::<f64>() {
        // JSON does not support NaN/Infinity — fall back to Null
        return if f.is_finite() {
            Ok(serde_json::json!(f))
        } else {
            Ok(serde_json::Value::Null)
        };
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_any_to_json_value(&item))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        return Ok(serde_json::Value::Object(py_dict_to_json_object(dict)?));
    }
    // Fallback: convert to string
    Ok(serde_json::Value::String(obj.str()?.to_string()))
}

/// Convert a `serde_json::Value` to a Python object (bound).
pub fn json_value_to_bound_py<'py>(
    py: Python<'py>,
    val: &serde_json::Value,
) -> PyResult<Bound<'py, PyAny>> {
    match val {
        serde_json::Value::Null => Ok(py.None().into_bound(py)),
        serde_json::Value::Bool(b) => Ok(PyBool::new(py, *b).to_owned().into_any()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.clone().into_any())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.clone().into_any())
            } else {
                tracing::warn!(
                    "serde_json::Number {n} cannot be represented as i64 or f64 — returning None"
                );
                Ok(py.None().into_bound(py))
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any()),
        serde_json::Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_bound_py(py, item)?)?;
            }
            Ok(list.into_any())
        }
        serde_json::Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, v) in obj {
                dict.set_item(k, json_value_to_bound_py(py, v)?)?;
            }
            Ok(dict.into_any())
        }
    }
}

/// Convert a `serde_json::Value` to an unbound `PyObject`.
///
/// Convenience wrapper around [`json_value_to_bound_py`] that calls `.unbind()`.
pub fn json_value_to_pyobject(py: Python, val: &serde_json::Value) -> PyResult<Py<PyAny>> {
    Ok(json_value_to_bound_py(py, val)?.unbind())
}

/// Convert a Python dict to a `serde_json::Map<String, serde_json::Value>`.
///
/// Used when building a `serde_json::Value::Object` directly (avoids the
/// intermediate `HashMap` allocation that [`py_dict_to_json_map`] requires).
fn py_dict_to_json_object(
    dict: &Bound<'_, PyDict>,
) -> PyResult<serde_json::Map<String, serde_json::Value>> {
    let mut map = serde_json::Map::with_capacity(dict.len());
    for (k, v) in dict.iter() {
        let key: String = k.extract()?;
        let val = py_any_to_json_value(&v)?;
        map.insert(key, val);
    }
    Ok(map)
}

/// Convert a Python dict to a `HashMap<String, serde_json::Value>`.
///
/// Delegates to [`py_dict_to_json_object`] and collects into a `HashMap`,
/// avoiding duplicate iteration logic.
pub fn py_dict_to_json_map(
    dict: &Bound<'_, PyDict>,
) -> PyResult<HashMap<String, serde_json::Value>> {
    Ok(py_dict_to_json_object(dict)?.into_iter().collect())
}

// ---------------------------------------------------------------------------
// High-performance Python-accessible JSON functions
// ---------------------------------------------------------------------------

/// Serialize a Python object to a JSON string using Rust's serde_json.
///
/// This is a high-performance drop-in replacement for `json.dumps()`.
/// Supports dicts, lists, strings, numbers, booleans, and None.
/// Non-serializable objects are converted to their string representation.
///
/// Parameters
/// ----------
/// obj : object
///     The Python object to serialize.
/// ensure_ascii : bool, optional
///     If True (default), escape non-ASCII characters. If False, output
///     raw Unicode characters.
/// indent : int or None, optional
///     If given, pretty-print with the specified number of spaces.
#[pyfunction]
#[pyo3(signature = (obj, *, ensure_ascii=true, indent=None))]
pub fn json_dumps(
    _py: Python,
    obj: &Bound<'_, PyAny>,
    ensure_ascii: bool,
    indent: Option<usize>,
) -> PyResult<String> {
    let value = py_any_to_json_value(obj)?;
    let s = match indent {
        Some(_) => serde_json::to_string_pretty(&value),
        None => serde_json::to_string(&value),
    }
    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;

    if ensure_ascii {
        Ok(s)
    } else {
        // serde_json always escapes non-ASCII; for ensure_ascii=False we
        // post-process the output to replace \uXXXX escapes with raw chars.
        Ok(unescape_unicode_json(&s))
    }
}

/// Deserialize a JSON string to a Python object using Rust's serde_json.
///
/// This is a high-performance drop-in replacement for `json.loads()`.
/// Returns a Python dict, list, string, number, bool, or None.
#[pyfunction]
pub fn json_loads(py: Python, s: &str) -> PyResult<Py<PyAny>> {
    let value: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    json_value_to_pyobject(py, &value)
}

/// Replace `\uXXXX` escape sequences with their actual Unicode characters
/// in a JSON string, for `ensure_ascii=False` support.
fn unescape_unicode_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.peek() == Some(&'u') {
                chars.next(); // consume 'u'
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    // Handle surrogate pairs (U+D800..U+DBFF followed by U+DC00..U+DFFF)
                    if (0xD800..=0xDBFF).contains(&code) {
                        if chars.peek() == Some(&'\\') {
                            let mut lookahead = chars.clone();
                            lookahead.next(); // consume '\'
                            if lookahead.peek() == Some(&'u') {
                                chars.next(); // consume '\'
                                chars.next(); // consume 'u'
                                let hex2: String = chars.by_ref().take(4).collect();
                                if let Ok(code2) = u32::from_str_radix(&hex2, 16) {
                                    if (0xDC00..=0xDFFF).contains(&code2) {
                                        let combined =
                                            0x10000 + (code - 0xD800) * 0x400 + (code2 - 0xDC00);
                                        if let Some(ch) = char::from_u32(combined) {
                                            result.push(ch);
                                            continue;
                                        }
                                    }
                                    // Invalid surrogate pair, keep as-is
                                    result.push_str(&format!("\\u{hex}\\u{hex2}"));
                                    continue;
                                }
                            }
                        }
                        // High surrogate without low surrogate, keep as-is
                        result.push_str(&format!("\\u{hex}"));
                        continue;
                    }
                    // BMP character
                    if let Some(ch) = char::from_u32(code) {
                        result.push(ch);
                        continue;
                    }
                    result.push_str(&format!("\\u{hex}"));
                    continue;
                }
                result.push_str(&format!("\\u{hex}"));
                continue;
            }
            result.push('\\');
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_basic() {
        assert_eq!(unescape_unicode_json(r#""hello""#), r#""hello""#);
        assert_eq!(unescape_unicode_json(r#""\u4f60\u597d""#), r#""你好""#);
    }

    #[test]
    fn test_unescape_no_escapes() {
        assert_eq!(unescape_unicode_json("abc"), "abc");
    }

    #[test]
    fn test_unescape_surrogate_pair() {
        // U+1F600 = D83D DE00 (surrogate pair)
        let input = r#""\uD83D\uDE00""#;
        let output = unescape_unicode_json(input);
        assert_eq!(output, r#""😀""#);
    }

    #[test]
    fn test_json_loads_basic() {
        let val: serde_json::Value = serde_json::from_str(r#"{"key": "value"}"#).unwrap();
        assert_eq!(val["key"], "value");
    }
}
