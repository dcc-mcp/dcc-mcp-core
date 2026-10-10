//! Gateway capability policy bindings (ADR-037 Cut 1).
//!
//! The policy value types themselves are operator-written configuration and
//! now live in `dcc-mcp-http-types`. This module keeps the historical
//! `dcc_mcp_gateway_core::policy::*` paths working by re-exporting them, and
//! supplies the only gateway-domain-specific piece: the [`PolicySubject`]
//! implementation for [`CapabilityRecord`].

pub use dcc_mcp_http_types::policy::{
    GatewayPolicy, GatewayPolicyDenial, GatewayPolicyDenyReason, GatewayPolicyOperation,
    PolicySubject,
};

use crate::capability::CapabilityRecord;

impl PolicySubject for CapabilityRecord {
    fn policy_dcc_type(&self) -> &str {
        &self.dcc_type
    }

    fn policy_skill_name(&self) -> Option<&str> {
        self.skill_name.as_deref()
    }

    fn policy_tool_slug(&self) -> &str {
        &self.tool_slug
    }

    fn policy_read_only_hint(&self) -> Option<bool> {
        self.annotations.as_ref().and_then(|a| a.read_only_hint)
    }
}
