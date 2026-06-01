use std::time::Duration;

use reqwest::header;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("server returned HTTP {status}: {body}")]
    Status {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error(
        "decode failed (HTTP {status}, content-type: {content_type}): {message}\nBody preview: {body_preview}"
    )]
    Decode {
        status: reqwest::StatusCode,
        content_type: String,
        body_preview: String,
        message: String,
    },
}

#[derive(Clone)]
pub struct HttpGateway {
    client: reqwest::Client,
}

impl Default for HttpGateway {
    fn default() -> Self {
        Self::with_timeout(Duration::from_secs(30))
    }
}

impl HttpGateway {
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn get_json(&self, url: &str) -> Result<Value, HttpError> {
        let response = self
            .client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .send()
            .await?;
        Self::json_response(response).await
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, HttpError> {
        let response = self
            .client
            .post(url)
            .header(header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await?;
        Self::json_response(response).await
    }

    pub async fn post_json_with_headers(
        &self,
        url: &str,
        body: &Value,
        headers: &[(&str, &str)],
    ) -> Result<Value, HttpError> {
        let mut request = self.client.post(url).json(body);
        let has_accept = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("accept"));
        if !has_accept {
            request = request.header(header::ACCEPT, "application/json");
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request.send().await?;
        Self::json_response(response).await
    }

    async fn json_response(response: reqwest::Response) -> Result<Value, HttpError> {
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();

        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(HttpError::Status { status, body });
        }

        // Content-Type-aware decoding: gateway defaults to TOON.
        if content_type.starts_with("application/toon") {
            return toon_format::decode_default(&body).map_err(|err| HttpError::Decode {
                status,
                content_type,
                body_preview: body_preview(&body),
                message: format!("TOON decode failed: {err}"),
            });
        }

        // Try JSON first.
        match serde_json::from_str::<Value>(&body) {
            Ok(value) => Ok(value),
            Err(json_err) => {
                // Fallback: try TOON even without TOON content-type.
                // This is defense-in-depth for gateway versions that might
                // return TOON with an incorrect Content-Type header.
                match toon_format::decode_default(&body) {
                    Ok(value) => Ok(value),
                    Err(toon_err) => Err(HttpError::Decode {
                        status,
                        content_type,
                        body_preview: body_preview(&body),
                        message: format!("JSON error: {json_err}; TOON error: {toon_err}"),
                    }),
                }
            }
        }
    }
}

/// Return the first 200 characters of the body for diagnostic output.
fn body_preview(body: &str) -> String {
    body.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::extract::Json;
    use axum::http::{HeaderMap, header};
    use axum::routing::get;
    use serde_json::json;
    use tokio::sync::oneshot;

    struct AcceptFixture {
        url: String,
        shutdown: Option<oneshot::Sender<()>>,
    }

    impl Drop for AcceptFixture {
        fn drop(&mut self) {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }

    async fn accept_echo(headers: HeaderMap) -> Json<Value> {
        let accept = headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        Json(json!({ "accept": accept }))
    }

    async fn spawn_accept_fixture() -> AcceptFixture {
        let app = Router::new().route("/accept", get(accept_echo).post(accept_echo));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        AcceptFixture {
            url: format!("http://{addr}/accept"),
            shutdown: Some(shutdown_tx),
        }
    }

    #[tokio::test]
    async fn get_json_requests_json_response() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();

        let response = gateway.get_json(&fixture.url).await.unwrap();

        assert_eq!(response["accept"], "application/json");
    }

    #[tokio::test]
    async fn post_json_requests_json_response() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();

        let response = gateway.post_json(&fixture.url, &json!({})).await.unwrap();

        assert_eq!(response["accept"], "application/json");
    }

    #[tokio::test]
    async fn post_json_with_headers_defaults_to_json_accept() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();

        let response = gateway
            .post_json_with_headers(&fixture.url, &json!({}), &[("X-Test", "yes")])
            .await
            .unwrap();

        assert_eq!(response["accept"], "application/json");
    }

    #[tokio::test]
    async fn post_json_with_headers_preserves_explicit_accept() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();

        let response = gateway
            .post_json_with_headers(
                &fixture.url,
                &json!({}),
                &[("Accept", "application/json, text/event-stream")],
            )
            .await
            .unwrap();

        assert_eq!(response["accept"], "application/json, text/event-stream");
    }

    // ── Content-Type-aware decode tests ──────────────────────────────

    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::post;

    struct DecodeFixture {
        url: String,
        shutdown: Option<oneshot::Sender<()>>,
    }

    impl Drop for DecodeFixture {
        fn drop(&mut self) {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }

    async fn spawn_decode_fixture(
        status: StatusCode,
        content_type: &'static str,
        body_str: String,
    ) -> DecodeFixture {
        let app = Router::new().route(
            "/decode",
            post(move || async move {
                (status, [(header::CONTENT_TYPE, content_type)], body_str).into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        DecodeFixture {
            url: format!("http://{addr}/decode"),
            shutdown: Some(shutdown_tx),
        }
    }

    #[tokio::test]
    async fn decode_json_response() {
        let payload = json!({"ok": true, "total": 3});
        let body = serde_json::to_string(&payload).unwrap();
        let fixture = spawn_decode_fixture(StatusCode::OK, "application/json", body).await;
        let gateway = HttpGateway::default();

        let value = gateway.post_json(&fixture.url, &json!({})).await.unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["total"], 3);
    }

    #[tokio::test]
    async fn decode_toon_response() {
        let payload = json!({"ok": true, "total": 3});
        let body = toon_format::encode_default(&payload).unwrap();
        let fixture =
            spawn_decode_fixture(StatusCode::OK, "application/toon; charset=utf-8", body).await;
        let gateway = HttpGateway::default();

        let value = gateway.post_json(&fixture.url, &json!({})).await.unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["total"], 3);
    }

    #[tokio::test]
    async fn decode_toon_fallback_when_json_fails() {
        // Gateway returns TOON content but with incorrect content-type.
        let payload = json!({"result": "success", "count": 7});
        let body = toon_format::encode_default(&payload).unwrap();
        let fixture = spawn_decode_fixture(StatusCode::OK, "text/plain", body).await;
        let gateway = HttpGateway::default();

        let value = gateway.post_json(&fixture.url, &json!({})).await.unwrap();

        assert_eq!(value["result"], "success");
        assert_eq!(value["count"], 7);
    }

    #[tokio::test]
    async fn decode_nested_toon_response() {
        let payload = json!({
            "total": 2,
            "hits": [
                {"tool_slug": "maya.create_sphere", "score": 91},
                {"tool_slug": "photoshop.select_layer", "score": 87},
            ],
        });
        let body = toon_format::encode_default(&payload).unwrap();
        let fixture =
            spawn_decode_fixture(StatusCode::OK, "application/toon; charset=utf-8", body).await;
        let gateway = HttpGateway::default();

        let value = gateway.post_json(&fixture.url, &json!({})).await.unwrap();

        assert_eq!(value["total"], 2);
        assert_eq!(value["hits"][0]["tool_slug"], "maya.create_sphere");
        assert_eq!(value["hits"][1]["score"], 87);
    }

    #[tokio::test]
    async fn decode_error_includes_content_type_and_body_preview() {
        let body = "this is not valid toon {{{".to_string();
        let fixture =
            spawn_decode_fixture(StatusCode::OK, "application/toon; charset=utf-8", body).await;
        let gateway = HttpGateway::default();

        let err = gateway
            .post_json(&fixture.url, &json!({}))
            .await
            .unwrap_err();

        let err_str = err.to_string();
        assert!(
            err_str.contains("decode failed"),
            "expected decode failed: {err_str}"
        );
        assert!(
            err_str.contains("application/toon"),
            "expected content-type in error: {err_str}"
        );
        assert!(
            err_str.contains("Body preview:"),
            "expected body preview: {err_str}"
        );
        assert!(
            err_str.contains("this is not valid toon"),
            "expected body preview content: {err_str}"
        );
    }
}
