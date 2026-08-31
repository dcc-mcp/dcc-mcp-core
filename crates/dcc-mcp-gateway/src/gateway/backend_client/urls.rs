/// Build the lightweight HTTP health URL that identifies a real MCP backend.
pub(crate) fn health_url_from_mcp_url(mcp_url: &str) -> String {
    map_mcp_url(mcp_url, "/health")
}

fn map_mcp_url(mcp_url: &str, suffix: &str) -> String {
    if let Ok(mut url) = reqwest::Url::parse(mcp_url) {
        let path = url.path().trim_end_matches('/');
        let base = path.strip_suffix("/mcp").unwrap_or(path);
        let path = if base.is_empty() {
            suffix.to_string()
        } else {
            format!("{base}{suffix}")
        };
        url.set_path(&path);
        url.set_query(None);
        return url.to_string();
    }
    mcp_url
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(|base| format!("{base}{suffix}"))
        .unwrap_or_else(|| format!("{}{suffix}", mcp_url.trim_end_matches('/')))
}

/// Build the legacy sidecar health URL.
///
/// Early sidecar listeners exposed `/healthz` rather than `/health` or
/// `/v1/readyz`. Keep probing it as a final fallback so a new gateway can
/// supervise already-running sidecars during mixed-version rollouts.
pub(crate) fn healthz_url_from_mcp_url(mcp_url: &str) -> String {
    map_mcp_url(mcp_url, "/healthz")
}

/// Build the readiness URL exposed by `dcc-mcp-skill-rest`
/// (issue #660 — `GET /v1/readyz`).
///
/// Mirrors [`health_url_from_mcp_url`]: strip the trailing `/mcp` segment
/// from the JSON-RPC endpoint and append the REST path.
pub(crate) fn readyz_url_from_mcp_url(mcp_url: &str) -> String {
    map_mcp_url(mcp_url, "/v1/readyz")
}

/// Derive the per-DCC REST base path from the MCP endpoint URL.
///
/// `http://host:port/mcp` → `http://host:port`
///
/// This is the root onto which `/v1/{search,call,prompts,resources,...}`
/// are appended.  Used by all REST-based backend calls (#818 phase 2).
pub(crate) fn rest_base_from_mcp_url(mcp_url: &str) -> String {
    if let Ok(mut url) = reqwest::Url::parse(mcp_url) {
        let path = url.path().trim_end_matches('/').to_owned();
        let base = path.strip_suffix("/mcp").unwrap_or(&path).to_owned();
        url.set_path(&base);
        url.set_query(None);
        return url.to_string().trim_end_matches('/').to_owned();
    }
    mcp_url
        .split_once('?')
        .map(|(base, _)| base)
        .unwrap_or(mcp_url)
        .trim_end_matches('/')
        .strip_suffix("/mcp")
        .map(str::to_owned)
        .unwrap_or_else(|| {
            mcp_url
                .split_once('?')
                .map(|(base, _)| base)
                .unwrap_or(mcp_url)
                .trim_end_matches('/')
                .to_owned()
        })
}
