//! HTTP reverse-proxy helper for the gateway.

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// Forward `body` to `target_url` via POST, forwarding relevant headers.
/// Returns the upstream response (or a `502 Bad Gateway` on error).
pub async fn proxy_request(
    client: &reqwest::Client,
    target_url: &str,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let mut req = client.post(target_url).body(body.to_vec());

    for (key, val) in &headers {
        let name = key.as_str().to_lowercase();
        if matches!(
            name.as_str(),
            "content-type" | "accept" | "mcp-session-id" | "authorization"
        ) && let Ok(v) = val.to_str()
        {
            req = req.header(key.as_str(), v);
        }
    }

    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let resp_headers = resp.headers().clone();
            let bytes = resp.bytes().await.unwrap_or_default();
            let mut response = Response::new(axum::body::Body::from(bytes));
            *response.status_mut() = status;
            for (k, v) in &resp_headers {
                let n = k.as_str().to_lowercase();
                if n == "content-type" || n.starts_with("mcp-") {
                    response.headers_mut().insert(k, v.clone());
                }
            }
            response
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("Upstream unreachable: {e}")})),
        )
            .into_response(),
    }
}
