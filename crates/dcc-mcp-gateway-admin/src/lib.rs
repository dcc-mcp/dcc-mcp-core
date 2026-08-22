//! Domain and embedded frontend boundary for the DCC-MCP gateway admin dashboard.
//!
//! This crate owns admin-facing trace and caller-context contracts independently
//! of gateway routing state. It also owns the Vite/npm build script and generated
//! dashboard payload; the Node.js toolchain only runs when `embed` is enabled.

#![forbid(unsafe_code)]

/// Admin trace and caller-attribution value types.
pub mod domain;
mod trace_log;

pub use domain::agent_context::{
    AgentContext, AgentContextTrust, INTERNAL_AUTH_SUBJECT_HEADER, INTERNAL_FORWARDED_FOR_HEADER,
    INTERNAL_SOURCE_IP_HEADER, TRUST_AUTH, TRUST_HEADER, TRUST_SELF_REPORTED, TRUST_SERVER_DERIVED,
    TRUST_TRUSTED_PROXY,
};
pub use domain::trace::{
    DispatchTrace, LlmUsage, MAX_AGENT_CONTEXT_LIST_ITEMS, MAX_AGENT_CONTEXT_METADATA_BYTES,
    MAX_AGENT_CONTEXT_STRING_BYTES, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TOKEN_ESTIMATOR,
    TokenTelemetry, TraceContext, TraceContextHeader, TracePayload, TraceSpan, estimate_tokens,
    parse_traceparent,
};
pub use trace_log::TraceLog;

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "embed")]
pub const ADMIN_HTML: &str = include_str!("generated/index.html");

/// Minimal fallback for direct builds that do not request embedded assets.
#[cfg(not(feature = "embed"))]
pub const ADMIN_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>DCC-MCP Gateway Admin</title></head><body><h1>DCC-MCP Gateway Admin</h1><p>The embedded admin UI is not available in this build.</p></body></html>"#;

#[cfg(test)]
mod tests {
    use super::ADMIN_HTML;

    #[test]
    fn admin_html_is_a_complete_document() {
        assert!(ADMIN_HTML.starts_with("<!doctype html>"));
        assert!(ADMIN_HTML.contains("DCC-MCP"));
        assert!(ADMIN_HTML.trim_end().ends_with("</html>"));
    }
}
