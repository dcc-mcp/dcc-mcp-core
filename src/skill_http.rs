//! Rust-backed synchronous HTTP helper for skill scripts.

use std::io::Read;
use std::time::{Duration, Instant};

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use reqwest::{Method, Url};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MAX_BYTES: usize = 1024 * 1024;

#[pyfunction(name = "skill_http_request")]
#[pyo3(signature = (
    method,
    url,
    headers=None,
    query=None,
    json_body=None,
    body=None,
    timeout_ms=DEFAULT_TIMEOUT_MS,
    max_bytes=DEFAULT_MAX_BYTES,
))]
pub fn py_skill_http_request(
    py: Python<'_>,
    method: &str,
    url: &str,
    headers: Option<Vec<(String, String)>>,
    query: Option<Vec<(String, String)>>,
    json_body: Option<String>,
    body: Option<Vec<u8>>,
    timeout_ms: u64,
    max_bytes: usize,
) -> PyResult<Py<PyDict>> {
    if timeout_ms == 0 {
        return error_payload(
            py,
            "invalid-timeout",
            "timeout_ms must be greater than zero",
            url,
        );
    }
    if json_body.is_some() && body.is_some() {
        return error_payload(
            py,
            "invalid-body",
            "json_body and body are mutually exclusive",
            url,
        );
    }

    let method = match Method::from_bytes(method.trim().as_bytes()) {
        Ok(method) => method,
        Err(err) => {
            return error_payload(
                py,
                "invalid-method",
                &format!("invalid HTTP method: {err}"),
                url,
            );
        }
    };

    let mut request_url = match Url::parse(url) {
        Ok(url) => url,
        Err(err) => {
            return error_payload(py, "invalid-url", &format!("invalid URL: {err}"), url);
        }
    };
    if let Some(query) = query.as_ref() {
        let mut pairs = request_url.query_pairs_mut();
        for (name, value) in query {
            pairs.append_pair(name, value);
        }
    }

    let mut parsed_headers = Vec::new();
    let mut has_content_type = false;
    if let Some(headers) = headers.as_ref() {
        for (name, value) in headers {
            let header_name = match HeaderName::from_bytes(name.as_bytes()) {
                Ok(header_name) => header_name,
                Err(err) => {
                    return error_payload(
                        py,
                        "invalid-header",
                        &format!("invalid HTTP header name {name:?}: {err}"),
                        url,
                    );
                }
            };
            if header_name == CONTENT_TYPE {
                has_content_type = true;
            }
            let header_value = match HeaderValue::from_str(value) {
                Ok(header_value) => header_value,
                Err(err) => {
                    return error_payload(
                        py,
                        "invalid-header",
                        &format!("invalid HTTP header value for {name:?}: {err}"),
                        url,
                    );
                }
            };
            parsed_headers.push((header_name, header_value));
        }
    }

    let original_url = url.to_string();
    let outcome = py.detach(move || {
        send_request(
            method,
            request_url,
            parsed_headers,
            has_content_type,
            json_body,
            body,
            timeout_ms,
            max_bytes,
            original_url,
        )
    });

    match outcome {
        RawHttpOutcome::Response(response) => response_payload(py, response),
        RawHttpOutcome::Error(err) => error_payload(py, err.kind, &err.message, &err.url),
    }
}

struct RawHttpResponse {
    status: u16,
    final_url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    truncated: bool,
    elapsed_ms: u64,
}

struct RawHttpError {
    kind: &'static str,
    message: String,
    url: String,
}

enum RawHttpOutcome {
    Response(RawHttpResponse),
    Error(RawHttpError),
}

fn send_request(
    method: Method,
    request_url: Url,
    headers: Vec<(HeaderName, HeaderValue)>,
    has_content_type: bool,
    json_body: Option<String>,
    body: Option<Vec<u8>>,
    timeout_ms: u64,
    max_bytes: usize,
    original_url: String,
) -> RawHttpOutcome {
    let client = match Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(err) => return raw_error("client-build", err.to_string(), original_url),
    };

    let mut request = client.request(method, request_url);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    if let Some(json_body) = json_body {
        if !has_content_type {
            request = request.header(CONTENT_TYPE, "application/json");
        }
        request = request.body(json_body);
    } else if let Some(body) = body {
        request = request.body(body);
    }

    let start = Instant::now();
    let response = match request.send() {
        Ok(response) => response,
        Err(err) => {
            return raw_error(classify_reqwest_error(&err), err.to_string(), original_url);
        }
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                value.to_str().unwrap_or("<non-utf8>").to_string(),
            )
        })
        .collect::<Vec<_>>();

    let limit = max_bytes.saturating_add(1) as u64;
    let mut reader = response.take(limit);
    let mut body = Vec::new();
    if let Err(err) = reader.read_to_end(&mut body) {
        return raw_error("body-read", err.to_string(), final_url);
    }
    let truncated = body.len() > max_bytes;
    if truncated {
        body.truncate(max_bytes);
    }

    RawHttpOutcome::Response(RawHttpResponse {
        status,
        final_url,
        headers,
        body,
        truncated,
        elapsed_ms,
    })
}

fn raw_error(kind: &'static str, message: String, url: String) -> RawHttpOutcome {
    RawHttpOutcome::Error(RawHttpError { kind, message, url })
}

fn response_payload(py: Python<'_>, response: RawHttpResponse) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    result.set_item("ok", true)?;
    result.set_item("status", response.status)?;
    result.set_item("url", response.final_url)?;
    result.set_item("headers", response.headers)?;
    result.set_item("body", PyBytes::new(py, &response.body))?;
    result.set_item("truncated", response.truncated)?;
    result.set_item("elapsed_ms", response.elapsed_ms)?;
    Ok(result.unbind())
}

fn classify_reqwest_error(err: &reqwest::Error) -> &'static str {
    if err.is_connect() {
        "connect"
    } else if err.is_timeout() {
        "timeout"
    } else if err.is_redirect() {
        "redirect"
    } else if err.is_decode() {
        "decode"
    } else if err.is_builder() {
        "request-build"
    } else if err.is_body() {
        "body"
    } else {
        "request"
    }
}

fn error_payload(py: Python<'_>, kind: &str, message: &str, url: &str) -> PyResult<Py<PyDict>> {
    let result = PyDict::new(py);
    result.set_item("ok", false)?;
    result.set_item("error_kind", kind)?;
    result.set_item("message", message)?;
    result.set_item("url", url)?;
    Ok(result.unbind())
}
