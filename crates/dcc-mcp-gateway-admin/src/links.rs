//! Link helpers shared by admin/debug JSON projections.

use axum::http::{HeaderMap, Uri};
use serde_json::{Value, json};

/// Builds stable absolute URLs for admin and agent-debug projections.
#[derive(Clone)]
pub struct AdminLinkBuilder {
    origin: String,
    admin_base: String,
}

impl AdminLinkBuilder {
    /// Derive the externally visible origin and mounted admin base from a request.
    #[must_use]
    pub fn from_request(headers: &HeaderMap, uri: &Uri) -> Self {
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

    /// Build links associated with one request or trace identifier.
    #[must_use]
    pub fn request_links(&self, request_id: &str) -> Value {
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

    /// Build the shared navigation links for workflow projections.
    #[must_use]
    pub fn workflow_links(&self) -> Value {
        json!({
            "admin_workflows_url": self.panel_url("workflows"),
            "admin_traces_url": self.panel_url("traces"),
            "openapi_inspector_url": self.panel_url("openapi"),
            "openapi_spec_url": format!("{}/v1/openapi.json", self.origin),
            "openapi_docs_url": format!("{}/docs", self.origin),
            "stats_url": self.panel_url("stats"),
        })
    }

    /// Build an absolute dashboard URL for a named panel.
    #[must_use]
    pub fn panel_url(&self, panel: &str) -> String {
        format!("{}{}?panel={panel}", self.origin, self.admin_base)
    }

    /// Build an absolute URL below the mounted admin API root.
    #[must_use]
    pub fn api_url(&self, path: &str) -> String {
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

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn debug_request_uses_default_admin_mount_and_encodes_identifier() {
        let links = AdminLinkBuilder::from_request(
            &HeaderMap::new(),
            &Uri::from_static("/v1/debug/traces/request"),
        );

        let request_links = links.request_links("maya request/1");
        assert_eq!(
            request_links["trace_api_url"],
            "http://127.0.0.1:9765/admin/api/traces/maya%20request%2F1"
        );
        assert_eq!(
            request_links["agent_trace_packet_url"],
            "http://127.0.0.1:9765/v1/debug/agent-traces/maya%20request%2F1"
        );
    }

    #[test]
    fn forwarded_origin_and_nested_admin_mount_are_preserved() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("gateway.example.test"),
        );
        let links = AdminLinkBuilder::from_request(
            &headers,
            &Uri::from_static("/studio/admin/api/traces/request"),
        );

        assert_eq!(
            links.panel_url("workflows"),
            "https://gateway.example.test/studio/admin?panel=workflows"
        );
        assert_eq!(
            links.api_url("traffic/export"),
            "https://gateway.example.test/studio/admin/api/traffic/export"
        );
    }
}
