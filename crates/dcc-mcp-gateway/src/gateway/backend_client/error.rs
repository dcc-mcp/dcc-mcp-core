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
                "{mcp_url}: not a DCC MCP HTTP endpoint (GET /v1/readyz and /health both failed)"
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
