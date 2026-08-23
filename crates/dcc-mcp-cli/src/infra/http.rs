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
    #[error("transport desync: response is missing X-Request-ID; expected {expected:?}")]
    MissingRequestId { expected: String },
    #[error(
        "transport desync: response X-Request-ID mismatch; expected {expected:?}, got {actual:?}"
    )]
    RequestIdMismatch { expected: String, actual: String },
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

    pub async fn get_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<Value, HttpError> {
        let mut request = self
            .client
            .get(url)
            .header(header::ACCEPT, "application/json");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        Self::json_response(request.send().await?).await
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

    pub async fn post_json_correlated(
        &self,
        url: &str,
        body: &Value,
        request_id: &str,
    ) -> Result<Value, HttpError> {
        let response = self
            .client
            .post(url)
            .header(header::ACCEPT, "application/json")
            .header("X-Request-ID", request_id)
            .json(body)
            .send()
            .await?;
        Self::json_response_correlated(response, request_id).await
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
        if status.is_success() {
            return response.json::<Value>().await.map_err(Into::into);
        }

        let body = response.text().await.unwrap_or_default();
        Err(HttpError::Status { status, body })
    }

    async fn json_response_correlated(
        response: reqwest::Response,
        expected_request_id: &str,
    ) -> Result<Value, HttpError> {
        let actual_request_id = response
            .headers()
            .get("X-Request-ID")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| HttpError::MissingRequestId {
                expected: expected_request_id.to_string(),
            })?;
        if actual_request_id != expected_request_id {
            return Err(HttpError::RequestIdMismatch {
                expected: expected_request_id.to_string(),
                actual: actual_request_id,
            });
        }
        if !response.status().is_success() {
            return Self::json_response(response).await;
        }
        response.json::<Value>().await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::Router;
    use axum::extract::Json;
    use axum::http::{HeaderMap, StatusCode, header};
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
        async fn correlated_echo(headers: HeaderMap) -> (HeaderMap, Json<Value>) {
            let mut response_headers = HeaderMap::new();
            if let Some(request_id) = headers.get("x-request-id") {
                response_headers.insert("x-request-id", request_id.clone());
            }
            (response_headers, Json(json!({"ok": true})))
        }

        async fn stale_correlation() -> (HeaderMap, Json<Value>) {
            let mut response_headers = HeaderMap::new();
            response_headers.insert("x-request-id", "previous-call".parse().unwrap());
            (response_headers, Json(json!({"ok": true})))
        }

        async fn correlated_error(headers: HeaderMap) -> (StatusCode, HeaderMap, Json<Value>) {
            let mut response_headers = HeaderMap::new();
            if let Some(request_id) = headers.get("x-request-id") {
                response_headers.insert("x-request-id", request_id.clone());
            }
            (
                StatusCode::BAD_REQUEST,
                response_headers,
                Json(json!({"error": "invalid request"})),
            )
        }

        let app = Router::new()
            .route("/accept", get(accept_echo).post(accept_echo))
            .route("/correlated", axum::routing::post(correlated_echo))
            .route(
                "/correlated-missing",
                axum::routing::post(|| async { Json(json!({"ok": true})) }),
            )
            .route("/correlated-stale", axum::routing::post(stale_correlation))
            .route("/correlated-error", axum::routing::post(correlated_error));
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

    #[tokio::test]
    async fn post_json_correlated_accepts_an_exact_echo() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();
        let url = fixture.url.replace("/accept", "/correlated");

        let response = gateway
            .post_json_correlated(&url, &json!({}), "current-call")
            .await
            .unwrap();

        assert_eq!(response["ok"], true);
    }

    #[tokio::test]
    async fn post_json_correlated_rejects_a_missing_echo() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();
        let url = fixture.url.replace("/accept", "/correlated-missing");

        let error = gateway
            .post_json_correlated(&url, &json!({}), "current-call")
            .await
            .unwrap_err();

        assert!(matches!(error, HttpError::MissingRequestId { .. }));
    }

    #[tokio::test]
    async fn post_json_correlated_rejects_a_stale_echo() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();
        let url = fixture.url.replace("/accept", "/correlated-stale");

        let error = gateway
            .post_json_correlated(&url, &json!({}), "current-call")
            .await
            .unwrap_err();

        assert!(matches!(error, HttpError::RequestIdMismatch { .. }));
        assert!(error.to_string().contains("transport desync"));
    }

    #[tokio::test]
    async fn post_json_correlated_validates_an_error_before_returning_its_status() {
        let fixture = spawn_accept_fixture().await;
        let gateway = HttpGateway::default();
        let url = fixture.url.replace("/accept", "/correlated-error");

        let error = gateway
            .post_json_correlated(&url, &json!({}), "current-call")
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            HttpError::Status {
                status: StatusCode::BAD_REQUEST,
                ..
            }
        ));
    }
}
