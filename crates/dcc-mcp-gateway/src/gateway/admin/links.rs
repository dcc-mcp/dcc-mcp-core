//! Link helpers shared by admin/debug JSON projections.

use axum::http::{HeaderMap, Uri};
use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) struct AdminLinkBuilder {
    origin: String,
    admin_base: String,
}

impl AdminLinkBuilder {
    pub(super) fn from_request(headers: &HeaderMap, uri: &Uri) -> Self {
        let proto = header_value(headers, "x-forwarded-proto").unwrap_or_else(|| "http".into());
        let host = header_value(headers, "x-forwarded-host")
            .or_else(|| header_value(headers, "host"))
            .unwrap_or_else(|| "127.0.0.1:9765".into());
        let admin_base = admin_base_path(uri.path());
        Self {
            origin: format!("{proto}://{host}"),
            admin_base,
        }
    }

    pub(super) fn request_links(&self, request_id: &str) -> Value {
        let encoded = encode_url_component(request_id);
        json!({
            "admin_trace_url": format!(
                "{}{}?panel=traces&trace={}",
                self.origin, self.admin_base, encoded
            ),
            "trace_api_url": format!(
                "{}{}/api/traces/{}",
                self.origin, self.admin_base, encoded
            ),
            "debug_bundle_url": format!(
                "{}{}/api/debug-bundle/{}",
                self.origin, self.admin_base, encoded
            ),
            "agent_trace_packet_url": format!(
                "{}/v1/debug/agent-traces/{}",
                self.origin, encoded
            ),
            "issue_report_url": format!(
                "{}{}/api/issue-report/{}",
                self.origin, self.admin_base, encoded
            ),
            "openapi_inspector_url": self.panel_url("openapi"),
            "openapi_spec_url": format!("{}/v1/openapi.json", self.origin),
            "openapi_docs_url": format!("{}/docs", self.origin),
            "stats_url": self.panel_url("stats"),
        })
    }

    pub(super) fn workflow_links(&self) -> Value {
        json!({
            "admin_workflows_url": self.panel_url("workflows"),
            "admin_traces_url": self.panel_url("traces"),
            "openapi_inspector_url": self.panel_url("openapi"),
            "openapi_spec_url": format!("{}/v1/openapi.json", self.origin),
            "openapi_docs_url": format!("{}/docs", self.origin),
            "stats_url": self.panel_url("stats"),
        })
    }

    pub(super) fn panel_url(&self, panel: &str) -> String {
        format!("{}{}?panel={panel}", self.origin, self.admin_base)
    }

    pub(super) fn api_url(&self, path: &str) -> String {
        let suffix = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!("{}{}/api{suffix}", self.origin, self.admin_base)
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn admin_base_path(path: &str) -> String {
    if path.starts_with("/v1/debug/") {
        return "/admin".to_string();
    }
    let base = path
        .find("/api")
        .map(|idx| &path[..idx])
        .unwrap_or(path)
        .trim_end_matches('/');
    if base.is_empty() {
        "/admin".to_string()
    } else {
        base.to_string()
    }
}

fn encode_url_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
