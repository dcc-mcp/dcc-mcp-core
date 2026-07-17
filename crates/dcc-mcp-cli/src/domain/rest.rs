use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatsRequest {
    pub range: String,
    pub dcc_type: Option<String>,
    pub skill: Option<String>,
    pub tool: Option<String>,
    pub status: Option<String>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
}

impl StatsRequest {
    #[must_use]
    pub fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = vec![("range", self.range.clone())];
        for (key, value) in [
            ("dcc_type", self.dcc_type.as_ref()),
            ("skill", self.skill.as_ref()),
            ("tool", self.tool.as_ref()),
            ("status", self.status.as_ref()),
            ("instance_id", self.instance_id.as_ref()),
            ("session_id", self.session_id.as_ref()),
        ] {
            if let Some(value) = value {
                pairs.push((key, value.clone()));
            }
        }
        pairs
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DescribeRequest {
    pub tool_slug: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSkillRequest {
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallRequest {
    pub tool_slug: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectCallRequest {
    pub dcc_type: String,
    pub instance_id: String,
    pub backend_tool: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitReadyRequest {
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
    pub required: Vec<String>,
    pub timeout: Duration,
    pub interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadSkillsRequest {
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StopInstanceRequest {
    pub dcc_type: String,
    pub instance_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
}

impl Endpoint {
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self { base_url }
    }

    #[must_use]
    pub fn path(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    #[must_use]
    pub fn mcp_url(&self) -> String {
        self.path("/mcp")
    }

    #[must_use]
    pub fn from_mcp_url(url: impl Into<String>) -> Self {
        let url = url.into();
        let trimmed = url.trim_end_matches('/');
        let base_url = trimmed.strip_suffix("/mcp").unwrap_or(trimmed).to_string();
        Self { base_url }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_normalizes_trailing_slashes() {
        let endpoint = Endpoint::new("http://127.0.0.1:9765/");
        assert_eq!(
            endpoint.path("/v1/instances"),
            "http://127.0.0.1:9765/v1/instances"
        );
    }

    #[test]
    fn endpoint_accepts_mcp_url_for_base() {
        let endpoint = Endpoint::from_mcp_url("http://127.0.0.1:9765/mcp");
        assert_eq!(endpoint.base_url, "http://127.0.0.1:9765");
        assert_eq!(endpoint.mcp_url(), "http://127.0.0.1:9765/mcp");
    }

    #[test]
    fn search_request_omits_empty_filters() {
        let body = serde_json::to_value(SearchRequest {
            query: Some("sphere".into()),
            dcc_type: None,
            instance_id: None,
            limit: Some(10),
        })
        .unwrap();
        assert_eq!(body["query"], "sphere");
        assert_eq!(body["limit"], 10);
        assert!(body.get("dcc_type").is_none());
    }

    #[test]
    fn stats_request_emits_stable_query_pairs() {
        let request = StatsRequest {
            range: "7d".into(),
            dcc_type: Some("houdini".into()),
            skill: Some("houdini-render".into()),
            tool: Some("render_rop".into()),
            status: Some("failure".into()),
            instance_id: Some("instance-a".into()),
            session_id: Some("solar-session".into()),
        };

        assert_eq!(
            request.query_pairs(),
            vec![
                ("range", "7d".into()),
                ("dcc_type", "houdini".into()),
                ("skill", "houdini-render".into()),
                ("tool", "render_rop".into()),
                ("status", "failure".into()),
                ("instance_id", "instance-a".into()),
                ("session_id", "solar-session".into()),
            ]
        );
    }
}
