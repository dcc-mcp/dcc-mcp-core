//! `gateway://docs/agent-workflows` — static, **platform-agnostic** guidance for
//! agents using the DCC-MCP **gateway**: MCP tools vs `resources/*` / `prompts/*`,
//! `describe` (schema, affinity, timeouts), `gateway://instances`, and
//! reading host help URIs from `resources/list`.
//!
//! Embeds `agent_workflows.md` in this directory; no dependency on any single
//! DCC SDK or vendor repository.

use serde_json::{Value, json};

/// Root URI for the embedded workflow guide.
pub const ROOT_URI: &str = "gateway://docs/agent-workflows";

/// Resource pointer emitted in `resources/list`.
pub fn pointer() -> Value {
    json!({
        "uri":         ROOT_URI,
        "name":        "DCC-MCP Gateway — agent workflow guide",
        "description": "How to use the MCP gateway well: tools/list exposes search→describe/load_skill→call; search hits carry executable next_step, has_schema=false may skip describe, unloaded hits carry target_tool_slug/available_groups, and correlated load_skill may inline compact_schema; REST GET /v1/context (instances), GET /v1/dcc/{dcc}/instances/{id}/describe, POST /v1/dcc/{dcc}/instances/{id}/call; refresh instances after DCC restart; resources/list+read; prompts; call/call_batch (≤25). Connector wrappers (e.g. get_sessions) map to gateway://instances or GET /v1/instances — not extra MCP tools. DCC names in chat → dcc_type / gateway://instances.",
        "mimeType":    "application/json"
    })
}

/// Recognise `gateway://docs/agent-workflows` (optional query string ignored).
pub fn parse(uri: &str) -> bool {
    let path = uri.split('?').next().unwrap_or(uri);
    path == ROOT_URI
}

/// JSON envelope so existing `resources/read` wiring can pretty-print `text`.
pub async fn build_payload() -> Result<Value, String> {
    Ok(json!({
        "uri":     ROOT_URI,
        "format":  "markdown",
        "document": include_str!("agent_workflows.md"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_root_and_ignores_query() {
        assert!(parse(ROOT_URI));
        assert!(parse(&format!("{ROOT_URI}?fresh=1")));
        assert!(!parse("gateway://docs/other"));
        assert!(!parse("gateway://instances"));
    }
}
