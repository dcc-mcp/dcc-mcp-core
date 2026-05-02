//! Shared dynamic-capability service used by both the REST API
//! (#654) and the MCP wrapper tools (#655).
//!
//! Keeping REST and MCP on top of a single service guarantees parity
//! without duplication — the only difference between a
//! `POST /v1/call` and a `call_tool` MCP invocation is the transport
//! adapter. That is the same invariant the tracking issue #657 calls
//! out as the "success criterion":
//!
//! > REST and MCP wrapper paths share the same routing/call
//! > implementation.
//!
//! The service is deliberately **async-free** for search/describe —
//! those operations never need to await because the capability index
//! is an in-process `parking_lot::RwLock`. The call path does await
//! on the backend HTTP forward but otherwise is a thin wrapper around
//! [`super::backend_client::forward_tools_call`].

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use dcc_mcp_jsonrpc::McpTool;

use super::backend_client::{forward_tools_call, try_fetch_tools};
use super::capability::{
    CapabilityIndex, CapabilityRecord, RefreshReason, SearchHit, SearchQuery, parse_slug,
    refresh_instance, remove_instance, search,
};
use super::state::GatewayState;

/// Shape of a structured error emitted by the call / describe paths.
///
/// Lives on the wire as JSON so REST and MCP callers see identical
/// error payloads — the `kind` discriminator lets agents dispatch on
/// failure class without parsing the free-form `message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceError {
    /// Short kebab-case discriminator (`unknown-slug`,
    /// `instance-offline`, `ambiguous`, `backend-error`).
    pub kind: String,
    /// Human-readable message. Safe to display to end users.
    pub message: String,
    /// Candidate slugs for `ambiguous` errors, empty otherwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CapabilityRecord>,
}

impl ServiceError {
    /// Convenience constructor used by the handlers.
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    /// Attach disambiguation candidates (for `kind = "ambiguous"`).
    pub fn with_candidates(mut self, candidates: Vec<CapabilityRecord>) -> Self {
        self.candidates = candidates;
        self
    }
}

/// Run a capability search against the index. Pure and synchronous —
/// every callable path reuses this function.
pub fn search_service(index: &CapabilityIndex, query: &SearchQuery) -> Vec<SearchHit> {
    let snap = index.snapshot();
    search(&snap, query)
}

/// Resolve `slug` to its record. Returns a structured error when the
/// slug is malformed, unknown, or matches more than one row (the
/// ambiguous case can happen if callers pass a record that has since
/// been evicted but an older one with the same backend tool remains).
pub fn describe_service(
    index: &CapabilityIndex,
    slug: &str,
) -> Result<CapabilityRecord, ServiceError> {
    if parse_slug(slug).is_none() {
        return Err(ServiceError::new(
            "unknown-slug",
            format!("slug {slug:?} is not in the <dcc>.<id8>.<tool> form"),
        ));
    }
    let snap = index.snapshot();
    let matches: Vec<&CapabilityRecord> = snap
        .records
        .iter()
        .filter(|r| r.tool_slug == slug)
        .collect();
    match matches.as_slice() {
        [] => Err(ServiceError::new(
            "unknown-slug",
            format!("no capability registered with slug {slug:?}"),
        )),
        [one] => Ok((*one).clone()),
        many => {
            let candidates: Vec<CapabilityRecord> = many.iter().map(|r| (*r).clone()).collect();
            Err(ServiceError::new(
                "ambiguous",
                format!(
                    "slug {slug:?} matches {} capability records — pick an instance by UUID",
                    candidates.len(),
                ),
            )
            .with_candidates(candidates))
        }
    }
}

/// Resolve `slug` and return the exact backend tool definition for that
/// capability. This is the schema-bearing describe path shared by REST and MCP.
pub async fn describe_tool_full(
    gs: &GatewayState,
    slug: &str,
) -> Result<(CapabilityRecord, McpTool), ServiceError> {
    let record = describe_service(&gs.capability_index, slug)?;
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);
    let Some(entry) = all.iter().find(|e| e.instance_id == record.instance_id) else {
        return Err(ServiceError::new(
            "instance-offline",
            format!(
                "instance {} ({}) is no longer live; refresh and retry",
                record.instance_id, record.dcc_type,
            ),
        ));
    };
    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
    drop(reg);

    let tools = try_fetch_tools(&gs.http_client, &url, gs.backend_timeout)
        .await
        .map_err(|e| {
            ServiceError::new(
                "schema-unavailable",
                format!("backend tools/list failed: {e}"),
            )
        })?;
    let Some(tool) = tools
        .into_iter()
        .find(|tool| tool.name == record.callable_id)
    else {
        return Err(ServiceError::new(
            "schema-unavailable",
            format!(
                "backend tool {:?} is no longer available on instance {}",
                record.callable_id, record.instance_id,
            ),
        ));
    };
    Ok((record, tool))
}

/// Call a backend action by slug. Returns the raw backend
/// `tools/call` envelope on success so REST and MCP wrappers can
/// forward it verbatim.
pub async fn call_service(
    gs: &GatewayState,
    slug: &str,
    arguments: Value,
    meta: Option<Value>,
) -> Result<Value, ServiceError> {
    let record = describe_service(&gs.capability_index, slug)?;
    // Resolve the backend endpoint using the live registry — the
    // capability record's `instance_id` is authoritative even if the
    // backend's port changed since indexing, because we always
    // look it up fresh here.
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);
    let Some(entry) = all.iter().find(|e| e.instance_id == record.instance_id) else {
        return Err(ServiceError::new(
            "instance-offline",
            format!(
                "instance {} ({}) is no longer live; refresh and retry",
                record.instance_id, record.dcc_type,
            ),
        ));
    };
    let url = format!("http://{}:{}/mcp", entry.host, entry.port);

    match forward_tools_call(
        &gs.http_client,
        &url,
        &record.callable_id,
        Some(arguments),
        meta,
        None,
        gs.backend_timeout,
    )
    .await
    {
        Ok(result) => Ok(result),
        Err(e) => Err(ServiceError::new(
            "backend-error",
            format!("backend call failed: {e}"),
        )),
    }
}

/// Helper — materialise a `SearchQuery` from the REST / MCP JSON
/// payload shape (`{query, dcc_type, tags, scene_hint, limit}`).
pub fn parse_search_payload(payload: &Value) -> SearchQuery {
    serde_json::from_value(payload.clone()).unwrap_or_default()
}

/// Refresh the capability index for every currently-live backend.
///
/// Called on-demand by the REST / MCP dynamic-capability entry
/// points so the first agent query after startup (or after a skill
/// load/unload) always sees fresh data without waiting for the
/// periodic watcher. Each backend's slice is short-circuited on an
/// unchanged fingerprint, so the extra `tools/list` round-trips are
/// free in the steady state.
///
/// Evicts records owned by instances that have disappeared from the
/// live registry — this is how `instance-offline` errors stay rare
/// after a backend crashes.
pub async fn refresh_all_live_backends(gs: &GatewayState, reason: RefreshReason) {
    let reg = gs.registry.read().await;
    let instances: Vec<_> = gs
        .live_instances(&reg)
        .into_iter()
        .filter(|e| {
            !matches!(
                e.status,
                dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
            )
        })
        .collect();
    drop(reg);

    // Remove records owned by instances that left between refreshes.
    let current: std::collections::HashSet<uuid::Uuid> =
        instances.iter().map(|e| e.instance_id).collect();
    let snap = gs.capability_index.snapshot();
    for iid in snap.fingerprints.keys() {
        if !current.contains(iid) {
            remove_instance(&gs.capability_index, *iid);
        }
    }

    // Refresh every live instance in parallel. Errors are logged and
    // swallowed — a single flaky backend must not break the others.
    let refreshes = instances.iter().map(|entry| {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        async move {
            refresh_instance(
                &gs.capability_index,
                &gs.http_client,
                &url,
                entry.instance_id,
                &entry.dcc_type,
                gs.backend_timeout,
                reason,
            )
            .await
        }
    });
    futures::future::join_all(refreshes).await;
}

/// Convert a [`ServiceError`] into the gateway's existing
/// `to_text_result` envelope shape so MCP wrappers return the same
/// error format as every other gateway meta-tool.
pub fn service_error_to_json(err: &ServiceError) -> Value {
    json!({
        "error": {
            "kind": err.kind,
            "message": err.message,
            "candidates": err.candidates,
        }
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::gateway::capability::{CapabilityRecord, InstanceFingerprint, tool_slug};
    use uuid::Uuid;

    fn push(index: &CapabilityIndex, dcc: &str, iid: Uuid, backend_tool: &str) {
        let rec = CapabilityRecord::new(
            tool_slug(dcc, &iid, backend_tool),
            backend_tool.to_string(),
            backend_tool.to_string(),
            None,
            "",
            Vec::new(),
            dcc.to_string(),
            iid,
            false,
        );
        index.upsert_instance(iid, vec![rec], InstanceFingerprint(1));
    }

    #[test]
    fn describe_returns_record_for_known_slug() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(0xabcd);
        push(&idx, "maya", iid, "create_sphere");
        let slug = tool_slug("maya", &iid, "create_sphere");
        let rec = describe_service(&idx, &slug).expect("slug should resolve");
        assert_eq!(rec.backend_tool, "create_sphere");
        assert_eq!(rec.dcc_type, "maya");
    }

    #[test]
    fn describe_rejects_malformed_slug() {
        let idx = CapabilityIndex::new();
        let err = describe_service(&idx, "not-a-slug").unwrap_err();
        assert_eq!(err.kind, "unknown-slug");
        // The malformed-slug error points at the expected shape so
        // the agent can fix its input instead of retrying blind.
        assert!(err.message.contains("<dcc>.<id8>.<tool>"));
    }

    #[test]
    fn describe_returns_unknown_for_live_but_unindexed_slug() {
        let idx = CapabilityIndex::new();
        // Shape is valid but nothing is indexed.
        let err = describe_service(&idx, "maya.abcdef01.create_sphere").unwrap_err();
        assert_eq!(err.kind, "unknown-slug");
    }

    #[test]
    fn search_service_uses_the_same_ranking_as_the_raw_helper() {
        // The REST / MCP surfaces MUST route through `search_service`;
        // this test pins that route by calling both paths and
        // checking the outputs are byte-identical.
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        push(&idx, "maya", iid, "create_sphere");
        push(&idx, "maya", iid, "open_scene");

        let q = SearchQuery {
            query: "sphere".into(),
            ..Default::default()
        };
        let via_service = search_service(&idx, &q);
        let via_raw = {
            let snap = idx.snapshot();
            search(&snap, &q)
        };
        let service_slugs: Vec<&str> = via_service
            .iter()
            .map(|h| h.record.tool_slug.as_str())
            .collect();
        let raw_slugs: Vec<&str> = via_raw
            .iter()
            .map(|h| h.record.tool_slug.as_str())
            .collect();
        assert_eq!(service_slugs, raw_slugs);
    }

    #[test]
    fn service_error_to_json_preserves_shape() {
        // The REST + MCP wrappers both serialise ServiceError through
        // this helper; the wire shape must stay stable so clients can
        // branch on `error.kind` without fuzzy matching.
        let err = ServiceError::new("unknown-slug", "x").with_candidates(Vec::new());
        let j = service_error_to_json(&err);
        assert_eq!(j["error"]["kind"], "unknown-slug");
        assert_eq!(j["error"]["message"], "x");
        assert_eq!(j["error"]["candidates"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn parse_search_payload_defaults_when_fields_missing() {
        // Both REST and MCP wrappers pass the caller's JSON straight
        // in; missing fields must default rather than fail so empty
        // queries (`{}`) become a catalogue browse.
        let q = parse_search_payload(&json!({}));
        assert!(q.query.is_empty());
        assert!(q.dcc_type.is_none());
        assert!(q.tags.is_empty());
        assert!(q.scene_hint.is_none());
        assert!(q.limit.is_none());
    }
}
