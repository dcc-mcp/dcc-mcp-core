//! Structured error envelopes for skill-management dispatch (#4210).
//!
//! Backends occasionally report `isError: true` with an empty
//! `content[0].text`, which leaves the caller with `ok=false` and nothing to
//! act on. The gateway knows more than the backend does at that point — the
//! target registry row, its status, and its last readiness probe — so it
//! substitutes an envelope with the same shape the `call` path already
//! returns.

use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
use serde_json::{Value, json};

use crate::gateway::GatewayState;
use crate::gateway::backend_client::readyz_url_from_mcp_url;
use crate::gateway::http_registration::entry_mcp_url;

/// Build the error envelope for a backend failure that returned no diagnostic
/// text.
pub(crate) fn structured_backend_error_text(
    gs: &GatewayState,
    tool: &str,
    entry: &ServiceEntry,
    forward_args: &Value,
) -> String {
    let requested = forward_args
        .get("skill_name")
        .or_else(|| forward_args.get("skill"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mcp_url = entry_mcp_url(entry);
    let diagnostics = gs
        .instance_diagnostics
        .get(&entry.instance_id)
        .map(|diag| {
            crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::to_json_value(&diag)
        })
        .unwrap_or(Value::Null);

    let mut message = format!(
        "backend returned an empty error payload for {tool}{} — the instance answered, so this is a backend-side failure, not a transport failure",
        if requested.is_empty() {
            String::new()
        } else {
            format!(" (skill_name={requested})")
        }
    );
    let ready = matches!(entry.status, ServiceStatus::Available);
    if !ready {
        message.push_str(&format!(
            "; target instance is {} so capability indexing skips it",
            entry.status
        ));
    }

    // The readiness URL the health loop actually probes: `map_mcp_url` strips
    // the `/mcp` segment, so `GET {mcp_url} /v1/readyz` is not a request an
    // operator can copy.
    let readyz_url = readyz_url_from_mcp_url(&mcp_url);
    let mut recommended_next_action = vec![
        format!("GET {readyz_url} from the gateway host and confirm it answers 200"),
        format!(
            "GET the gateway /v1/readyz and read instances[].probe_failure for instance {}",
            entry.instance_id
        ),
    ];
    if !ready {
        recommended_next_action.push(
            "Fix readiness first: a non-Available instance exposes no capabilities regardless of the backend"
                .to_string(),
        );
    }

    serde_json::to_string_pretty(&json!({
        "kind": "backend-error-empty-payload",
        "tool": tool,
        "message": message,
        "request": {
            "skill_name": requested,
            "instance_id": entry.instance_id.to_string(),
            "dcc_type": entry.dcc_type,
        },
        "instance": {
            "instance_id": entry.instance_id.to_string(),
            "dcc_type": entry.dcc_type,
            "status": entry.status.to_string(),
            "mcp_url": mcp_url,
        },
        "diagnostics": diagnostics,
        "candidates": [],
        "recommended_next_action": recommended_next_action,
    }))
    .unwrap_or_else(|_| message.clone())
}
