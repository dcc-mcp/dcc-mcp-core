/// Build the lightweight HTTP health URL that identifies a real MCP backend.
pub(crate) fn health_url_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}/health"))
        .unwrap_or_else(|| format!("{}/health", mcp_url.trim_end_matches('/')))
}

/// Build the legacy sidecar health URL.
///
/// Early sidecar listeners exposed `/healthz` rather than `/health` or
/// `/v1/readyz`. Keep probing it as a final fallback so a new gateway can
/// supervise already-running sidecars during mixed-version rollouts.
pub(crate) fn healthz_url_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}/healthz"))
        .unwrap_or_else(|| format!("{}/healthz", mcp_url.trim_end_matches('/')))
}

/// Build the three-state readiness URL exposed by `dcc-mcp-skill-rest`
/// (issue #660 — `GET /v1/readyz`).
///
/// Mirrors [`health_url_from_mcp_url`]: strip the trailing `/mcp` segment
/// from the JSON-RPC endpoint and append the REST path.
pub(crate) fn readyz_url_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}/v1/readyz"))
        .unwrap_or_else(|| format!("{}/v1/readyz", mcp_url.trim_end_matches('/')))
}

/// Derive the per-DCC REST base path from the MCP endpoint URL.
///
/// `http://host:port/mcp` → `http://host:port`
///
/// This is the root onto which `/v1/{search,call,prompts,resources,...}`
/// are appended.  Used by all REST-based backend calls (#818 phase 2).
pub(crate) fn rest_base_from_mcp_url(mcp_url: &str) -> String {
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(str::to_owned)
        .unwrap_or_else(|| mcp_url.trim_end_matches('/').to_owned())
}
