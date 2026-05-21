use std::fmt;

#[derive(Debug)]
pub(crate) enum BackendCallError {
    Booting {
        mcp_url: String,
    },
    Unreachable {
        mcp_url: String,
    },
    Transport {
        mcp_url: String,
        reason: String,
    },
    Http {
        mcp_url: String,
        status: String,
        body: String,
    },
    ReadBody {
        mcp_url: String,
        reason: String,
    },
    InvalidJson {
        mcp_url: String,
        reason: String,
    },
    Backend {
        mcp_url: String,
        code: i64,
        message: String,
    },
    EmptyResult {
        mcp_url: String,
    },
}

impl fmt::Display for BackendCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Booting { mcp_url } => write!(
                f,
                "{mcp_url}: backend not ready (GET /v1/readyz reports not ready — host DCC still initialising)"
            ),
            Self::Unreachable { mcp_url } => write!(
                f,
                "{mcp_url}: not a DCC MCP HTTP endpoint (GET /v1/readyz and legacy /health or /healthz probes failed)"
            ),
            Self::Transport { mcp_url, reason } => {
                write!(f, "{mcp_url}: transport error: {reason}")
            }
            Self::Http {
                mcp_url,
                status,
                body,
            } => write!(f, "{mcp_url}: HTTP {status}: {body}"),
            Self::ReadBody { mcp_url, reason } => write!(f, "{mcp_url}: read body: {reason}"),
            Self::InvalidJson { mcp_url, reason } => {
                write!(f, "{mcp_url}: invalid JSON-RPC response: {reason}")
            }
            Self::Backend {
                mcp_url,
                code,
                message,
            } => write!(f, "{mcp_url}: backend error {code}: {message}"),
            Self::EmptyResult { mcp_url } => write!(f, "{mcp_url}: empty JSON-RPC result"),
        }
    }
}

fn http_status_prometheus_kind(status_line: &str) -> &'static str {
    let code = status_line
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u16>().ok());
    match code {
        Some(c) if (500..600).contains(&c) => "http_5xx",
        Some(c) if (400..500).contains(&c) => "http_4xx",
        Some(_) => "http_other",
        None => "http_other",
    }
}

impl BackendCallError {
    /// Coarse label for `dcc_mcp_gateway_backend_errors_total{kind=…}`.
    #[must_use]
    pub(crate) fn prometheus_error_kind(&self) -> &'static str {
        match self {
            Self::Booting { .. } => "booting",
            Self::Unreachable { .. } => "unreachable",
            Self::Transport { reason, .. } => {
                if reason.contains("circuit breaker") {
                    "circuit_open"
                } else {
                    "transport"
                }
            }
            Self::Http { status, .. } => http_status_prometheus_kind(status),
            Self::ReadBody { .. } => "read_body",
            Self::InvalidJson { .. } => "invalid_json",
            Self::Backend { .. } => "jsonrpc_backend",
            Self::EmptyResult { .. } => "empty_result",
        }
    }
}

/// Classify `rest_get` / `rest_post` `Err(String)` for Prometheus (same vocabulary as JSON-RPC).
#[must_use]
pub(crate) fn rest_error_prometheus_kind(err: &str) -> &'static str {
    if err.contains("circuit breaker open") || err.contains("circuit breaker") {
        return "circuit_open";
    }
    if err.contains("transport error") {
        return "transport";
    }
    if let Some(idx) = err.find(": HTTP ") {
        let tail = &err[idx + 7..];
        if let Some(st) = tail.split_whitespace().next() {
            return http_status_prometheus_kind(st);
        }
    }
    if err.contains("invalid JSON response") {
        return "invalid_json";
    }
    if err.contains("read body") {
        return "read_body";
    }
    "other"
}
