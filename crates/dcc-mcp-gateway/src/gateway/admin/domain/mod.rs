//! Compatibility paths for domain types owned by `dcc-mcp-gateway-admin`.

pub mod agent_context {
    pub use dcc_mcp_gateway_admin::domain::agent_context::*;
}

pub mod trace {
    pub use dcc_mcp_gateway_admin::domain::trace::*;
}

pub use trace::*;
