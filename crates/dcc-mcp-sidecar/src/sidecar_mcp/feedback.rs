//! Gateway feedback forwarding for the sidecar dispatch surface.

use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{Map, Value, json};

pub(super) const FEEDBACK_TOOL_NAME: &str = "dcc_feedback__report";
const GATEWAY_EVENTS_URI: &str = "resources://gateway/events";
const FEEDBACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Gateway client that binds reports to one sidecar-owned DCC instance.
#[derive(Clone)]
pub(crate) struct SidecarFeedbackForwarder {
    dcc_type: String,
    instance_id: String,
    endpoint: Option<String>,
    client: Option<reqwest::Client>,
}

impl SidecarFeedbackForwarder {
    pub(crate) fn new(
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
        endpoint: Option<String>,
    ) -> Self {
        Self {
            dcc_type: dcc_type.into(),
            instance_id: instance_id.into(),
            endpoint,
            client: reqwest::Client::builder()
                .timeout(FEEDBACK_TIMEOUT)
                .build()
                .ok(),
        }
    }

    pub(crate) fn for_gateway(
        dcc_type: impl Into<String>,
        instance_id: impl Into<String>,
        gateway_host: &str,
        gateway_port: u16,
    ) -> Self {
        Self::new(
            dcc_type,
            instance_id,
            gateway_feedback_endpoint(gateway_host, gateway_port),
        )
    }

    pub(crate) async fn forward(&self, arguments: Value) -> Value {
        let Some(arguments) = arguments.as_object() else {
            return feedback_error(
                "Feedback params must be an object.",
                "invalid_input",
                Map::new(),
            );
        };
        let (Some(endpoint), Some(client)) = (self.endpoint.as_deref(), self.client.as_ref())
        else {
            return unavailable_error();
        };

        let mut report = Map::new();
        for name in [
            "tool_name",
            "intent",
            "attempt",
            "blocker",
            "severity",
            "request_id",
            "job_id",
        ] {
            if let Some(value) = arguments.get(name)
                && !value.is_null()
            {
                report.insert(name.to_string(), value.clone());
            }
        }
        report.insert("dcc_type".to_string(), Value::String(self.dcc_type.clone()));
        report.insert(
            "instance_id".to_string(),
            Value::String(self.instance_id.clone()),
        );

        let transport_request_id = uuid::Uuid::new_v4().to_string();
        let response = match client
            .post(endpoint)
            .header(reqwest::header::ACCEPT, "application/json")
            .header("X-Request-ID", &transport_request_id)
            .json(&Value::Object(report))
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(error = %error, "sidecar gateway feedback forwarding failed");
                return unavailable_error();
            }
        };

        let echoed_request_id = response
            .headers()
            .get("X-Request-ID")
            .and_then(|value| value.to_str().ok());
        if echoed_request_id != Some(transport_request_id.as_str()) {
            return feedback_error(
                "Gateway feedback response did not match the request.",
                "transport_desync",
                Map::new(),
            );
        }

        let status = response.status();
        let payload = response.json::<Value>().await.ok();
        if !status.is_success() {
            let mut context = Map::new();
            context.insert(
                "status_code".to_string(),
                Value::Number(status.as_u16().into()),
            );
            return feedback_error(
                gateway_error_message(payload.as_ref()),
                "gateway_feedback_rejected",
                context,
            );
        }
        if status != StatusCode::CREATED {
            return invalid_receipt(status);
        }

        let Some(payload) = payload.as_ref().and_then(Value::as_object) else {
            return invalid_receipt(status);
        };
        let feedback_id = payload.get("feedback_id").and_then(Value::as_str);
        let recorded_at = payload.get("recorded_at").and_then(Value::as_str);
        let event_resource_uri = payload.get("event_resource_uri").and_then(Value::as_str);
        let feedback_id_is_valid =
            feedback_id.is_some_and(|value| uuid::Uuid::parse_str(value).is_ok());
        if payload.get("ok").and_then(Value::as_bool) != Some(true)
            || payload.get("success").and_then(Value::as_bool) != Some(true)
            || !feedback_id_is_valid
            || recorded_at.is_none_or(str::is_empty)
            || event_resource_uri != Some(GATEWAY_EVENTS_URI)
        {
            return invalid_receipt(status);
        }

        json!({
            "success": true,
            "message": "Feedback recorded at the gateway.",
            "context": {
                "feedback_id": feedback_id,
                "recorded_at": recorded_at,
                "event_resource_uri": event_resource_uri,
            }
        })
    }
}

fn gateway_feedback_endpoint(host: &str, port: u16) -> Option<String> {
    let host = host.trim();
    if port == 0
        || host.is_empty()
        || ["://", "/", "?", "#", "@"]
            .iter()
            .any(|marker| host.contains(marker))
    {
        return None;
    }
    let host = match host {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        value if value.contains(':') && !value.starts_with('[') => format!("[{value}]"),
        value => value.to_string(),
    };
    let endpoint = format!("http://{host}:{port}/v1/feedback");
    reqwest::Url::parse(&endpoint).ok().map(|_| endpoint)
}

fn gateway_error_message(payload: Option<&Value>) -> &str {
    payload
        .and_then(|value| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
        })
        .unwrap_or("Gateway rejected the feedback report.")
}

fn unavailable_error() -> Value {
    feedback_error(
        "Gateway feedback endpoint is unavailable.",
        "gateway_feedback_unavailable",
        Map::new(),
    )
}

fn invalid_receipt(status: StatusCode) -> Value {
    let mut context = Map::new();
    context.insert(
        "status_code".to_string(),
        Value::Number(status.as_u16().into()),
    );
    feedback_error(
        "Gateway returned an invalid feedback receipt.",
        "gateway_feedback_invalid_receipt",
        context,
    )
}

fn feedback_error(message: &str, error: &str, context: Map<String, Value>) -> Value {
    let mut result = Map::from_iter([
        ("success".to_string(), Value::Bool(false)),
        ("message".to_string(), Value::String(message.to_string())),
        ("error".to_string(), Value::String(error.to_string())),
    ]);
    if !context.is_empty() {
        result.insert("context".to_string(), Value::Object(context));
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_endpoint_normalizes_wildcard_and_ipv6_hosts() {
        assert_eq!(
            gateway_feedback_endpoint("0.0.0.0", 9765).as_deref(),
            Some("http://127.0.0.1:9765/v1/feedback")
        );
        assert_eq!(
            gateway_feedback_endpoint("::1", 9765).as_deref(),
            Some("http://[::1]:9765/v1/feedback")
        );
        assert_eq!(gateway_feedback_endpoint("127.0.0.1", 0), None);
        assert_eq!(gateway_feedback_endpoint("http://gateway", 9765), None);
    }
}
