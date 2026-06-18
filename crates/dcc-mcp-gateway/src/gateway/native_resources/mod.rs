//! Gateway-native MCP resources (issues #813 phases 1+2 / #818 phase 0).
//!
//! These resources expose read-only registry / diagnostics / catalog views
//! that previously had dedicated MCP tools. Promoting them to resources
//! keeps the gateway tool surface lean — clients that need the view fetch
//! it on demand via `resources/read` instead of paying for tool definitions
//! in every `tools/list`.
//!
//! ## Module shape
//!
//! Each resource family lives in its own sub-module (SRP):
//!
//! - [`instances`] — `gateway://instances[/{id}]` (DCC registry)
//! - [`diagnostics`] — `gateway://diagnostics/{process,audit,metrics}`
//! - [`catalog`] — `gateway://catalog[/{name}]` (public package index)
//! - [`agent_workflows`] — `gateway://docs/agent-workflows` (MCP + resources + prompts + describe_tool hints + efficiency)
//!
//! [`util`] holds shared parsing helpers (`parse_query`, `parse_bool`,
//! `split_uri`).
//!
//! Adding a new resource family means: create a new sub-module exporting
//! `ROOT_URI`, `pointer()`, `Query`, `parse()`, `build_payload()`, then
//! plug it into [`pointers_for_list`] and [`Request::parse`] below
//! (~10 lines per family — OCP-aligned: existing resources do not change).
//!
//! ## What this layer does NOT do
//!
//! - It does not subscribe / push: `resources/subscribe` for `gateway://*`
//!   URIs falls through to the existing no-op handler. The
//!   `notifications/resources/list_changed` debouncer in
//!   [`super::tasks`] watches backend resources only; wiring it to
//!   registry mutations is a separate change (#766 follow-up).
//! - It does not enumerate every per-instance / per-catalog-entry URI in
//!   `resources/list` — only the root pointers. Callers can still
//!   `resources/read` any single-entry URI they have learned from the
//!   list payload. Listing one entry per item would reproduce the very
//!   fan-out the epic exists to remove.

pub mod agent_workflows;
pub mod catalog;
pub mod diagnostics;
pub mod instances;
pub mod util;

use serde_json::Value;

use super::state::GatewayState;

/// Re-export every gateway-native pointer in one batch — used by
/// `aggregator::resources::aggregate_resources_list` to inject the
/// gateway-native tier before backend resources are merged in.
///
/// Order is stable: instances first, then diagnostics, then catalog, then docs.
pub fn pointers_for_list() -> Vec<Value> {
    let mut out = Vec::with_capacity(7);
    out.push(instances::pointer());
    out.extend(diagnostics::pointers());
    out.push(catalog::pointer());
    out.push(agent_workflows::pointer());
    out
}

/// Parsed dispatch target across every gateway-native resource family.
///
/// Each variant wraps the family's own `Query` enum so the per-family
/// modules stay independently testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Instances(instances::Query),
    Diagnostics(diagnostics::Query),
    Catalog(catalog::Query),
    AgentWorkflows,
}

impl Request {
    /// Dispatch a `resources/read` URI to the right family.
    ///
    /// Returns `None` if the URI does not target any gateway-native
    /// resource — callers fall through to the next handler (backend
    /// resource decoding, then the `dcc://` admin fallback, etc.).
    pub fn parse(uri: &str) -> Option<Self> {
        if let Some(q) = instances::parse(uri) {
            return Some(Self::Instances(q));
        }
        if let Some(q) = diagnostics::parse(uri) {
            return Some(Self::Diagnostics(q));
        }
        if let Some(q) = catalog::parse(uri) {
            return Some(Self::Catalog(q));
        }
        if agent_workflows::parse(uri) {
            return Some(Self::AgentWorkflows);
        }
        None
    }

    /// Render the JSON payload for this request.
    ///
    /// `local_tool_count` is the value reported as
    /// `metrics.gateway_local_tools` by `gateway://diagnostics/metrics`;
    /// it is supplied by the caller (rather than computed here) so this
    /// module does not have to depend on the higher-level `tools` module
    /// (DIP — high-level diagnostics builder depends on a scalar, not on
    /// the tool registry).
    pub async fn build_payload(
        &self,
        gs: &GatewayState,
        local_tool_count: usize,
    ) -> Result<Value, String> {
        match self {
            Self::Instances(q) => instances::build_payload(gs, q).await,
            Self::Diagnostics(q) => diagnostics::build_payload(gs, q, local_tool_count).await,
            Self::Catalog(q) => catalog::build_payload(q).await,
            Self::AgentWorkflows => agent_workflows::build_payload().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dispatches_to_instances() {
        assert!(matches!(
            Request::parse("gateway://instances"),
            Some(Request::Instances(_))
        ));
    }

    #[test]
    fn parse_dispatches_to_diagnostics() {
        for uri in [
            "gateway://diagnostics/process",
            "gateway://diagnostics/audit",
            "gateway://diagnostics/metrics",
            "gateway://diagnostics/search",
        ] {
            assert!(
                matches!(Request::parse(uri), Some(Request::Diagnostics(_))),
                "uri: {uri}"
            );
        }
    }

    #[test]
    fn parse_dispatches_to_catalog() {
        assert!(matches!(
            Request::parse("gateway://catalog"),
            Some(Request::Catalog(_))
        ));
        assert!(matches!(
            Request::parse("gateway://catalog/foo"),
            Some(Request::Catalog(_))
        ));
    }

    #[test]
    fn parse_returns_none_for_unknown_uris() {
        assert!(Request::parse("dcc://maya/abc").is_none());
        assert!(Request::parse("resources://gateway/events").is_none());
        assert!(Request::parse("gateway://unknown").is_none());
    }

    #[test]
    fn pointers_for_list_covers_all_families() {
        let ps = pointers_for_list();
        let uris: Vec<&str> = ps.iter().filter_map(|p| p["uri"].as_str()).collect();
        assert!(uris.contains(&instances::ROOT_URI));
        assert!(uris.contains(&diagnostics::PROCESS_URI));
        assert!(uris.contains(&diagnostics::AUDIT_URI));
        assert!(uris.contains(&diagnostics::METRICS_URI));
        assert!(uris.contains(&diagnostics::SEARCH_URI));
        assert!(uris.contains(&catalog::ROOT_URI));
        assert!(uris.contains(&agent_workflows::ROOT_URI));
        assert_eq!(ps.len(), 7);
    }

    #[test]
    fn parse_dispatches_to_agent_workflows() {
        assert!(matches!(
            Request::parse(agent_workflows::ROOT_URI),
            Some(Request::AgentWorkflows)
        ));
    }
}
