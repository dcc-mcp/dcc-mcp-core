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
        if status.is_success() {
            return response.json::<Value>().await.map_err(Into::into);
        }

        let body = response.text().await.unwrap_or_default();
        Err(HttpError::Status { status, body })
    }
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
}
