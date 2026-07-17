//! Shared response negotiation for stable admin/debug endpoints.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::gateway::response_codec::{ResponseFormat, negotiated_response_with_default};

#[derive(Debug, Default, Deserialize)]
pub struct DebugListQuery {
    limit: Option<String>,
    range: Option<String>,
    dcc_type: Option<String>,
    skill: Option<String>,
    tool: Option<String>,
    status: Option<String>,
    instance_id: Option<String>,
    session_id: Option<String>,
    response_format: Option<String>,
    compact: Option<bool>,
}

impl DebugListQuery {
    pub(crate) fn limit(&self, default: usize, max: usize) -> usize {
        self.limit
            .as_deref()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(default)
            .clamp(1, max)
    }

    pub(crate) fn range(&self) -> &str {
        self.range.as_deref().unwrap_or("all")
    }

    pub(crate) fn stats_filter(&self) -> Result<crate::gateway::admin::stats::StatsFilter, String> {
        use crate::gateway::admin::stats::{StatsFilter, StatsStatus};

        Ok(StatsFilter {
            dcc_type: non_empty(self.dcc_type.as_deref()),
            skill: non_empty(self.skill.as_deref()),
            tool: non_empty(self.tool.as_deref()),
            status: non_empty(self.status.as_deref())
                .as_deref()
                .map(StatsStatus::from_query)
                .transpose()?,
            instance_id: non_empty(self.instance_id.as_deref()),
            session_id: non_empty(self.session_id.as_deref()),
        })
    }

    fn response_format_body(&self) -> Value {
        let mut body = serde_json::Map::new();
        if let Some(format) = self.response_format.as_deref() {
            body.insert("response_format".to_string(), json!(format));
        }
        if let Some(compact) = self.compact {
            body.insert("compact".to_string(), json!(compact));
        }
        Value::Object(body)
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn debug_response(
    headers: &HeaderMap,
    params: &DebugListQuery,
    status: StatusCode,
    legacy_json: Value,
    compact_json: Option<Value>,
) -> Response {
    let request_body = params.response_format_body();
    negotiated_response_with_default(
        headers,
        &request_body,
        status,
        legacy_json,
        compact_json,
        ResponseFormat::Json,
    )
}
