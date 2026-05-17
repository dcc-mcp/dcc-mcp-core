use std::time::Duration;

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
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl HttpGateway {
    pub async fn get_json(&self, url: &str) -> Result<Value, HttpError> {
        let response = self.client.get(url).send().await?;
        Self::json_response(response).await
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, HttpError> {
        let response = self.client.post(url).json(body).send().await?;
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
