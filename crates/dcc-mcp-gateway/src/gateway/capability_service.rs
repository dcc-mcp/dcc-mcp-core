//! Shared dynamic-capability service used by both the REST API
//! (#654) and the MCP wrapper tools (#655).
//!
//! Keeping REST and MCP on top of a single service guarantees parity
//! without duplication — the only difference between a
//! `POST /v1/call` and a hidden compatibility MCP invocation is the transport
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
use std::sync::Arc;
use uuid::Uuid;

use dcc_mcp_gateway_core::policy::{GatewayPolicy, GatewayPolicyDenial, GatewayPolicyOperation};
use dcc_mcp_jsonrpc::McpTool;
use dcc_mcp_transport::discovery::{file_registry::FileRegistry, types::ServiceEntry};

use crate::gateway::admin::trace::TraceContext;
use crate::gateway::http_registration::{
    HttpInstanceDeregisterRequest, HttpInstanceRegistry, entry_mcp_url,
};

use super::admin::trace::AgentContext;
use super::backend_client::{ForwardToolsCallRequest, forward_tools_call, try_describe_tool};
use super::capability::{
    CapabilityIndex, CapabilityRecord, RANKER_VERSION, RefreshReason, SearchHit, SearchQuery,
    parse_slug, refresh_instance, remove_instance, search,
};
use super::request_meta::meta_with_agent_context;
use super::state::GatewayState;
use dcc_mcp_gateway_core::naming::instance_short;

/// Metadata attached to one gateway search response for follow-up correlation.
#[derive(Debug, Clone)]
pub struct SearchResponseContext {
    pub search_id: String,
    pub ranker_version: &'static str,
    pub index_generation: String,
}

impl SearchResponseContext {
    #[must_use]
    pub fn new(search_id: String, index_generation: String) -> Self {
        Self {
            search_id,
            ranker_version: RANKER_VERSION,
            index_generation,
        }
    }
}

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
    /// Why a prior instance is no longer routable (`deregistered`,
    /// `heartbeat-timeout`, `never-registered`) — issue #996.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_status: Option<String>,
    /// Last known instance UUID when `kind = instance-offline`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_instance_id: Option<String>,
    /// Backend identity + readiness/dispatcher diagnostics (#1076).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<Box<Value>>,
    /// Gateway policy denial details when `kind = "policy-denied"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<GatewayPolicyDenial>,
}

impl ServiceError {
    /// Convenience constructor used by the handlers.
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            candidates: Vec::new(),
            previous_status: None,
            previous_instance_id: None,
            backend: None,
            policy: None,
        }
    }

    /// Attach backend routing/diagnostics context (#1076).
    pub fn with_backend(mut self, backend: Value) -> Self {
        self.backend = Some(Box::new(backend));
        self
    }

    /// Attach gateway policy denial context.
    pub fn with_policy_denial(mut self, denial: GatewayPolicyDenial) -> Self {
        self.policy = Some(denial);
        self
    }

    /// Attach disambiguation candidates (for `kind = "ambiguous"`).
    pub fn with_candidates(mut self, candidates: Vec<CapabilityRecord>) -> Self {
        self.candidates = candidates;
        self
    }

    /// Attach instance lifecycle provenance (issue #996).
    pub fn with_instance_provenance(
        mut self,
        previous_status: impl Into<String>,
        previous_instance_id: Option<Uuid>,
    ) -> Self {
        self.previous_status = Some(previous_status.into());
        self.previous_instance_id = previous_instance_id.map(|id| id.to_string());
        self
    }
}

/// Run a capability search against the index. Pure and synchronous —
/// every callable path reuses this function.
pub fn search_service(index: &CapabilityIndex, query: &SearchQuery) -> Vec<SearchHit> {
    let snap = index.snapshot();
    search(&snap, query)
}

/// Search and materialise transport-neutral JSON rows. Unloaded
/// capability hits include a machine-executable `next_step` that both
/// MCP wrappers and REST clients can call directly.
pub fn search_service_rows(index: &CapabilityIndex, query: &SearchQuery) -> Vec<Value> {
    search_service(index, query)
        .into_iter()
        .map(search_hit_to_value)
        .collect()
}

/// Search and materialise rows after applying gateway policy surface filters.
///
/// Policy allowlists hide disallowed capabilities from search. Read-only mode
/// is intentionally not a search filter: agents may still discover actions,
/// but execution of non-read-only records is denied by `call_service`.
pub fn search_service_rows_for_policy(
    index: &CapabilityIndex,
    query: &SearchQuery,
    policy: &GatewayPolicy,
) -> Vec<Value> {
    search_service_hits_for_policy(index, query, policy)
        .into_iter()
        .map(search_hit_to_value)
        .collect()
}

pub fn search_hit_to_value(hit: SearchHit) -> Value {
    search_hit_to_value_with_context(hit, None)
}

pub fn search_service_hits_for_policy(
    index: &CapabilityIndex,
    query: &SearchQuery,
    policy: &GatewayPolicy,
) -> Vec<SearchHit> {
    search_service(index, query)
        .into_iter()
        .filter(|hit| {
            policy
                .enforce_record(GatewayPolicyOperation::Search, &hit.record)
                .is_ok()
        })
        .collect()
}

pub fn search_service_rows_for_policy_with_context(
    index: &CapabilityIndex,
    query: &SearchQuery,
    policy: &GatewayPolicy,
    context: &SearchResponseContext,
) -> Vec<Value> {
    search_service_hits_for_policy(index, query, policy)
        .into_iter()
        .map(|hit| search_hit_to_value_with_context(hit, Some(context)))
        .collect()
}

pub fn search_hit_to_value_with_context(
    hit: SearchHit,
    context: Option<&SearchResponseContext>,
) -> Value {
    let mut value = serde_json::to_value(&hit).unwrap_or(Value::Null);

    // Always expose the callable flag.
    let callable = hit.record.is_callable();
    value["callable"] = json!(callable);

    if !hit.record.loaded {
        value["load_state"] = json!("unloaded");
        if let Some(skill_name) = &hit.record.skill_name {
            let mut arguments = json!({
                "skill_name": skill_name,
                "dcc": &hit.record.dcc_type,
                "dcc_type": &hit.record.dcc_type,
                "instance_id": hit.record.instance_id.to_string(),
            });
            // If the tool belongs to a group that will need activation,
            // hint the tool_group so the agent can activate it after load.
            if let Some(ref tool_group) = hit.record.tool_group {
                arguments["tool_group"] = json!(tool_group);
            }
            attach_search_meta(&mut arguments, context);
            value["next_step"] = json!({
                "action": "load_skill",
                "arguments": arguments.clone(),
                "mcp": {
                    "tool": "load_skill",
                    "arguments": arguments.clone(),
                    "_meta": search_meta(context),
                },
                "rest": {
                    "method": "POST",
                    "path": "/v1/load_skill",
                    "body": arguments,
                },
            });
        }
    } else if hit.record.loaded {
        value["load_state"] = json!("loaded");
        if let Some(group_name) = hit.record.disabled_by_group() {
            // Tool is loaded but its progressive group is inactive.
            value["disabled_by_group"] = json!(group_name);
            // Provide an activate_tool_group next_step (MCP only —
            // activate_tool_group is a gateway meta-tool, not a
            // capability-indexed action, so there is no REST /v1/call
            // route for it).
            if let Some(skill_name) = &hit.record.skill_name {
                let mut arguments = json!({
                    "skill_name": skill_name,
                    "dcc": &hit.record.dcc_type,
                    "dcc_type": &hit.record.dcc_type,
                    "tool_group": group_name,
                    "instance_id": hit.record.instance_id.to_string(),
                });
                attach_search_meta(&mut arguments, context);
                value["next_step"] = json!({
                    "action": "activate_tool_group",
                    "arguments": arguments.clone(),
                    "mcp": {
                        "tool": "activate_tool_group",
                        "arguments": {
                            "skill_name": skill_name,
                            "group": group_name,
                        },
                        "_meta": search_meta(context),
                    },
                });
            }
        } else {
            // Tool is loaded and callable — standard describe next_step.
            let mut arguments = json!({
                "tool_slug": hit.record.tool_slug,
            });
            attach_search_meta(&mut arguments, context);
            value["next_step"] = json!({
                "action": "describe",
                "arguments": arguments.clone(),
                "mcp": {
                    "tool": "describe",
                    "arguments": arguments.clone(),
                    "_meta": search_meta(context),
                },
                "rest": {
                    "method": "POST",
                    "path": "/v1/describe",
                    "body": arguments,
                },
            });
        }
    }
    value
}

fn attach_search_meta(arguments: &mut Value, context: Option<&SearchResponseContext>) {
    let Some(meta) = search_meta(context) else {
        return;
    };
    if let Some(obj) = arguments.as_object_mut() {
        obj.insert("meta".to_string(), meta);
    }
}

fn search_meta(context: Option<&SearchResponseContext>) -> Option<Value> {
    context.map(|ctx| {
        json!({
            "search_id": ctx.search_id,
            "ranker_version": ctx.ranker_version,
            "index_generation": ctx.index_generation,
        })
    })
}

/// Compute a stable, compact fingerprint for the current capability index.
///
/// The value is intentionally opaque. Clients can compare it for equality to
/// detect that a cached `tool_slug` came from an older search view.
pub fn index_generation(index: &CapabilityIndex) -> String {
    index_snapshot_generation(&index.snapshot())
}

fn index_snapshot_generation(snapshot: &super::capability::IndexSnapshot) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    let mut fingerprints: Vec<_> = snapshot.fingerprints.iter().collect();
    fingerprints.sort_by_key(|(id, _)| id.to_string());
    for (id, fp) in fingerprints {
        id.hash(&mut hasher);
        fp.0.hash(&mut hasher);
    }
    for record in snapshot.records.iter() {
        record.tool_slug.hash(&mut hasher);
        record.loaded.hash(&mut hasher);
        record.has_schema.hash(&mut hasher);
        record.tool_group.hash(&mut hasher);
        for group in &record.available_groups {
            group.name.hash(&mut hasher);
            group.default_active.hash(&mut hasher);
            group.active.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

/// Resolve `slug` to its record. Returns a structured error when the
/// slug is malformed, unknown, or matches more than one row (the
/// ambiguous case can happen if callers pass a record that has since
/// been evicted but an older one with the same backend tool remains).
pub fn describe_service(
    index: &CapabilityIndex,
    slug: &str,
) -> Result<CapabilityRecord, ServiceError> {
    let Some((dcc, instance_hint, tool)) = parse_slug(slug) else {
        return Err(ServiceError::new(
            "unknown-slug",
            format!("slug {slug:?} is not in the <dcc>.<instance-id-prefix>.<tool> form"),
        ));
    };
    let snap = index.snapshot();
    let matches: Vec<&CapabilityRecord> = snap
        .records
        .iter()
        .filter(|r| record_matches_slug(r, dcc, instance_hint, tool))
        .collect();
    match matches.as_slice() {
        [] => Err(ServiceError::new(
            "instance-offline",
            format!("no capability registered with slug {slug:?}"),
        )
        .with_instance_provenance("never-registered", parse_instance_uuid(instance_hint))),
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

fn record_matches_slug(
    record: &CapabilityRecord,
    dcc: &str,
    instance_hint: &str,
    tool: &str,
) -> bool {
    if !record.dcc_type.eq_ignore_ascii_case(dcc) || record.callable_id != tool {
        return false;
    }
    if let Ok(uuid) = Uuid::parse_str(instance_hint) {
        return record.instance_id == uuid;
    }
    record
        .instance_id
        .simple()
        .to_string()
        .starts_with(&instance_hint.to_ascii_lowercase())
}

/// Resolve `slug` and return the exact backend tool definition for that
/// capability. This is the schema-bearing describe path shared by REST and MCP.
///
/// Uses `POST /v1/describe` on the backend to fetch the **full** `input_schema`
/// (including `properties`), rather than the schema-free `/v1/search` response.
/// This fixes issue #992 where schemas were stripped.
pub async fn describe_tool_full(
    gs: &GatewayState,
    slug: &str,
) -> Result<(CapabilityRecord, McpTool), ServiceError> {
    let record = describe_service(&gs.capability_index, slug)?;
    enforce_record_policy(&gs.policy, GatewayPolicyOperation::Describe, &record)?;
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);
    let Some(entry) = all.iter().find(|e| e.instance_id == record.instance_id) else {
        return Err(ServiceError::new(
            "instance-offline",
            format!(
                "instance {} ({}) is no longer live; refresh and retry",
                record.instance_id, record.dcc_type,
            ),
        )
        .with_instance_provenance("deregistered", Some(record.instance_id)));
    };
    let url = entry_mcp_url(entry);
    drop(reg);

    // Use /v1/describe to get the full input_schema (issue #992).
    // The backend's resolve_slug accepts bare action names as well as
    // full <dcc>.<skill>.<action> slugs, so we can pass callable_id directly.
    let tool = try_describe_tool(
        &gs.http_client,
        &url,
        &record.callable_id,
        gs.backend_timeout,
    )
    .await
    .map_err(|e| {
        ServiceError::new(
            "schema-unavailable",
            format!("backend /v1/describe failed: {e}"),
        )
    })?;
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
    trace_context: Option<&TraceContext>,
    agent_context: Option<&AgentContext>,
) -> Result<Value, ServiceError> {
    let record = describe_service(&gs.capability_index, slug)?;
    enforce_record_policy(&gs.policy, GatewayPolicyOperation::Call, &record)?;
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
        )
        .with_instance_provenance("deregistered", Some(record.instance_id)));
    };
    let url = entry_mcp_url(entry);
    let entry = entry.clone();
    drop(reg);

    match forward_tools_call(
        &gs.http_client,
        &url,
        ForwardToolsCallRequest {
            tool_name: &record.callable_id,
            arguments: Some(arguments),
            meta: meta_with_agent_context(meta, agent_context),
            request_id: None,
            trace_context,
            traffic_capture: Some(&gs.traffic_capture),
            timeout: gs.backend_timeout,
        },
    )
    .await
    {
        Ok(mut result) => {
            inject_call_instance_meta(&mut result, &entry);
            Ok(result)
        }
        Err(e) => {
            let backend_body = super::instance_diagnostics::parse_rest_error_json(&e);
            let kind = backend_failure_kind(&e, backend_body.as_ref());
            gs.instance_diagnostics.record_call_error(
                entry.instance_id,
                kind,
                e.chars().take(512).collect::<String>(),
            );
            let diag = gs.instance_diagnostics.get(&entry.instance_id);
            let backend_attachment = super::instance_diagnostics::backend_error_attachment(
                &entry,
                &gs.gateway_mcp_url(),
                diag.as_ref(),
                backend_body.as_ref(),
            );
            if kind == "host-died" {
                record_host_died(gs, &entry, &record, &e);
                evict_host_died_instance(
                    &gs.capability_index,
                    &gs.registry,
                    &gs.http_instance_registry,
                    &entry,
                )
                .await;
            }
            Err(ServiceError::new(
                kind,
                format!("backend call failed during {}: {e}", record.callable_id),
            )
            .with_backend(backend_attachment))
        }
    }
}

/// Helper — materialise a `SearchQuery` from the REST / MCP JSON
/// payload shape (`{query, dcc_type, instance_id, tags, scene_hint,
/// limit}`).
pub fn parse_search_payload(payload: &Value) -> SearchQuery {
    let raw_instance_id = payload
        .get("instance_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let mut sanitized = payload.clone();
    if let Some(obj) = sanitized.as_object_mut() {
        obj.remove("instance_id");
    }
    let mut query: SearchQuery = serde_json::from_value(sanitized).unwrap_or_default();
    if let Some(raw) = raw_instance_id
        && let Ok(uuid) = Uuid::parse_str(&raw)
    {
        query.instance_id = Some(uuid);
    }
    query
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
        let url = entry_mcp_url(entry);
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
    let mut error = json!({
        "kind": err.kind,
        "message": err.message,
        "candidates": err.candidates,
        "previous_status": err.previous_status,
        "previous_instance_id": err.previous_instance_id,
    });
    if let Some(backend) = &err.backend {
        error["backend"] = (**backend).clone();
    }
    if let Some(policy) = &err.policy {
        error["policy"] = serde_json::to_value(policy).unwrap_or(Value::Null);
    }
    json!({ "error": error })
}

pub(crate) fn policy_denied_error(denial: GatewayPolicyDenial) -> ServiceError {
    crate::gateway::metrics::record_gateway_governance_event("policy", denial.reason.as_str());
    ServiceError::new("policy-denied", denial.message.clone()).with_policy_denial(denial)
}

fn enforce_record_policy(
    policy: &GatewayPolicy,
    operation: GatewayPolicyOperation,
    record: &CapabilityRecord,
) -> Result<(), ServiceError> {
    policy
        .enforce_record(operation, record)
        .map_err(policy_denied_error)
}

fn inject_call_instance_meta(result: &mut Value, entry: &ServiceEntry) {
    let Some(obj) = result.as_object_mut() else {
        return;
    };

    let meta = obj.entry("_meta".to_string()).or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    let meta_obj = meta.as_object_mut().expect("_meta just normalised");
    let dcc = meta_obj
        .entry("dcc".to_string())
        .or_insert_with(|| json!({}));
    if !dcc.is_object() {
        *dcc = json!({});
    }
    let dcc_obj = dcc.as_object_mut().expect("_meta.dcc just normalised");
    dcc_obj.insert("dcc_type".to_string(), json!(entry.dcc_type));
    dcc_obj.insert(
        "instance_id".to_string(),
        json!(entry.instance_id.to_string()),
    );
    dcc_obj.insert(
        "instance_short".to_string(),
        json!(instance_short(&entry.instance_id)),
    );
    dcc_obj.insert("display_id".to_string(), json!(entry.display_id()));
}

fn backend_failure_kind(message: &str, backend_body: Option<&Value>) -> &'static str {
    if let Some(kind) = backend_body
        .and_then(|b| b.get("kind"))
        .and_then(Value::as_str)
    {
        match kind {
            "host-busy" => return "host-busy",
            "host-died" => return "host-died",
            "thread-affinity-violation" => return "thread-affinity-violation",
            _ => {}
        }
    }
    if is_host_busy_backend_error(message) {
        "host-busy"
    } else if is_host_died_backend_error(message) {
        "host-died"
    } else {
        "backend-error"
    }
}

fn is_host_busy_backend_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("host-busy")
        || lower.contains("queue-overloaded")
        || lower.contains("queue overloaded")
}

fn is_host_died_backend_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("host-died") || lower.contains("host_died")
}

fn record_host_died(
    gs: &GatewayState,
    entry: &ServiceEntry,
    record: &CapabilityRecord,
    backend_error: &str,
) {
    let mut reason = format!(
        "call={} display_id={} error={}",
        record.callable_id,
        entry.display_id(),
        backend_error
    );
    const MAX_REASON_CHARS: usize = 512;
    if reason.chars().count() > MAX_REASON_CHARS {
        reason = reason.chars().take(MAX_REASON_CHARS).collect();
        reason.push_str("...");
    }

    crate::gateway::event_log::record_event(
        &gs.event_log,
        #[cfg(feature = "prometheus")]
        &gs.gateway_metrics,
        crate::gateway::event_log::EventKind::HostDied,
        entry.dcc_type.clone(),
        instance_short(&entry.instance_id),
        Some(reason),
    );

    if gs.events_tx.receiver_count() > 0 {
        let notif = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/resources/updated",
            "params": {"uri": crate::gateway::handlers::resources::GATEWAY_EVENTS_URI}
        }))
        .unwrap_or_default();
        let _ = gs.events_tx.send(notif);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HostDiedEviction {
    capability_records_removed: bool,
    file_registry_row_removed: bool,
    http_registry_row_removed: bool,
}

async fn evict_host_died_instance(
    index: &Arc<CapabilityIndex>,
    registry: &Arc<tokio::sync::RwLock<FileRegistry>>,
    http_registry: &Arc<parking_lot::RwLock<HttpInstanceRegistry>>,
    entry: &ServiceEntry,
) -> HostDiedEviction {
    let capability_records_removed = remove_instance(index, entry.instance_id);
    let file_registry_row_removed = {
        let reg = registry.read().await;
        match reg.deregister(&entry.key()) {
            Ok(removed) => removed.is_some(),
            Err(err) => {
                tracing::warn!(
                    instance_id = %entry.instance_id,
                    dcc = %entry.dcc_type,
                    error = %err,
                    "gateway failed to deregister host-died instance"
                );
                false
            }
        }
    };
    let http_registry_row_removed = http_registry
        .write()
        .deregister(HttpInstanceDeregisterRequest {
            instance_id: entry.instance_id.to_string(),
        })
        .map(|removed| removed.is_some())
        .unwrap_or(false);
    tracing::info!(
        instance_id = %entry.instance_id,
        dcc = %entry.dcc_type,
        capability_records_removed,
        file_registry_row_removed,
        http_registry_row_removed,
        "gateway evicted host-died instance"
    );
    HostDiedEviction {
        capability_records_removed,
        file_registry_row_removed,
        http_registry_row_removed,
    }
}

fn parse_instance_uuid(instance_hint: &str) -> Option<Uuid> {
    Uuid::parse_str(instance_hint).ok()
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use crate::gateway::capability::{CapabilityRecord, InstanceFingerprint, tool_slug};
    use uuid::Uuid;

    fn push(index: &CapabilityIndex, dcc: &str, iid: Uuid, backend_tool: &str, loaded: bool) {
        let rec = CapabilityRecord::new(
            tool_slug(dcc, &iid, backend_tool),
            backend_tool.to_string(),
            backend_tool.to_string(),
            None,
            "",
            Vec::new(),
            dcc.to_string(),
            iid,
            false, // has_schema
            loaded,
            None,
        );
        index.upsert_instance(iid, vec![rec], InstanceFingerprint(1));
    }

    #[test]
    fn describe_returns_record_for_known_slug() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(0xabcd);
        push(&idx, "maya", iid, "create_sphere", true);
        let slug = tool_slug("maya", &iid, "create_sphere");
        let rec = describe_service(&idx, &slug).expect("slug should resolve");
        assert_eq!(rec.backend_tool, "create_sphere");
        assert_eq!(rec.dcc_type, "maya");
    }

    #[test]
    fn describe_accepts_full_uuid_slug() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        push(&idx, "maya", iid, "create_sphere", true);
        let slug = format!("maya.{iid}.create_sphere");

        let rec = describe_service(&idx, &slug).expect("full UUID slug should resolve");

        assert_eq!(rec.instance_id, iid);
        assert_eq!(rec.backend_tool, "create_sphere");
    }

    #[test]
    fn describe_rejects_malformed_slug() {
        let idx = CapabilityIndex::new();
        let err = describe_service(&idx, "not-a-slug").unwrap_err();
        assert_eq!(err.kind, "unknown-slug");
        // The malformed-slug error points at the expected shape so
        // the agent can fix its input instead of retrying blind.
        assert!(err.message.contains("<dcc>.<instance-id-prefix>.<tool>"));
    }

    #[test]
    fn describe_returns_unknown_for_live_but_unindexed_slug() {
        let idx = CapabilityIndex::new();
        // Shape is valid but nothing is indexed.
        let err = describe_service(&idx, "maya.abcdef01.create_sphere").unwrap_err();
        assert_eq!(err.kind, "instance-offline");
        assert_eq!(err.previous_status.as_deref(), Some("never-registered"));
    }

    #[test]
    fn search_service_uses_the_same_ranking_as_the_raw_helper() {
        // The REST / MCP surfaces MUST route through `search_service`;
        // this test pins that route by calling both paths and
        // checking the outputs are byte-identical.
        let idx = CapabilityIndex::new();
        let iid = Uuid::from_u128(1);
        push(&idx, "maya", iid, "create_sphere", true);
        push(&idx, "maya", iid, "open_scene", true);

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
    fn search_service_rows_adds_executable_load_skill_next_step() {
        let idx = CapabilityIndex::new();
        let iid = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let rec = CapabilityRecord::new(
            tool_slug("maya", &iid, "maya_primitives__create_sphere"),
            "maya_primitives__create_sphere".to_string(),
            "maya_primitives__create_sphere".to_string(),
            Some("maya-primitives".to_string()),
            "Create a sphere",
            Vec::new(),
            "maya".to_string(),
            iid,
            true,
            false,
            None,
        )
        .with_available_groups(vec![crate::gateway::capability::CapabilityGroupInfo {
            name: "core".to_string(),
            description: "Default modeling primitives".to_string(),
            tools: vec!["create_sphere".to_string()],
            default_active: true,
            active: Some(false),
        }]);
        idx.upsert_instance(iid, vec![rec], InstanceFingerprint(1));

        let rows = search_service_rows(
            &idx,
            &SearchQuery {
                query: "sphere".into(),
                loaded_only: Some(false),
                ..Default::default()
            },
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["load_state"], "unloaded");
        assert_eq!(rows[0]["available_groups"][0]["name"], "core");
        assert_eq!(rows[0]["next_step"]["action"], "load_skill");
        assert_eq!(
            rows[0]["next_step"]["arguments"]["skill_name"],
            "maya-primitives"
        );
        assert_eq!(rows[0]["next_step"]["mcp"]["tool"], "load_skill");
        assert_eq!(rows[0]["next_step"]["arguments"]["dcc"], "maya");
        assert_eq!(
            rows[0]["next_step"]["arguments"]["instance_id"],
            iid.to_string()
        );
        assert_eq!(rows[0]["next_step"]["rest"]["path"], "/v1/load_skill");
        assert_eq!(
            rows[0]["next_step"]["rest"]["body"]["instance_id"],
            iid.to_string()
        );
    }

    #[test]
    fn search_service_rows_carry_load_state_for_two_dcc_families() {
        let idx = CapabilityIndex::new();
        let maya = Uuid::from_u128(1);
        let photoshop = Uuid::from_u128(2);
        push(&idx, "maya", maya, "maya_workflow__cube", false);
        push(
            &idx,
            "photoshop",
            photoshop,
            "photoshop_workflow__select",
            true,
        );

        let rows = search_service_rows(
            &idx,
            &SearchQuery {
                query: "workflow".to_string(),
                loaded_only: Some(false),
                ..Default::default()
            },
        );

        let states: std::collections::HashMap<_, _> = rows
            .iter()
            .filter_map(|row| {
                Some((
                    row.get("dcc_type")?.as_str()?.to_string(),
                    row.get("load_state")?.as_str()?.to_string(),
                ))
            })
            .collect();
        assert_eq!(states.get("maya").map(String::as_str), Some("unloaded"));
        assert_eq!(states.get("photoshop").map(String::as_str), Some("loaded"));
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
    fn call_result_meta_includes_display_id() {
        let mut entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", 8765);
        entry.instance_id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        entry.version = Some("2026".to_string());
        let mut result = json!({"slug": "maya.test.tool", "output": {"ok": true}});

        inject_call_instance_meta(&mut result, &entry);

        assert_eq!(result["_meta"]["dcc"]["dcc_type"], "maya");
        assert_eq!(
            result["_meta"]["dcc"]["instance_id"],
            "abcdef01-2345-6789-abcd-ef0123456789"
        );
        assert_eq!(result["_meta"]["dcc"]["instance_short"], "abcdef01");
        assert_eq!(result["_meta"]["dcc"]["display_id"], "maya@2026-abcdef01");
    }

    #[test]
    fn forwarded_meta_includes_canonical_agent_context() {
        let merged = meta_with_agent_context(
            Some(json!({"search_id": "search-1"})),
            Some(&AgentContext {
                actor_id: Some("artist-1".to_string()),
                client_platform: Some("cursor".to_string()),
                source_ip: Some("192.0.2.44".to_string()),
                forwarded_for: vec!["198.51.100.7".to_string()],
                ..AgentContext::default()
            }),
        )
        .expect("merged meta");

        assert_eq!(merged["search_id"], "search-1");
        assert_eq!(merged["agent_context"]["actor_id"], "artist-1");
        assert_eq!(merged["agent_context"]["client_platform"], "cursor");
        assert_eq!(merged["agent_context"]["source_ip"], "192.0.2.44");
        assert_eq!(
            merged["agent_context"]["forwarded_for"],
            json!(["198.51.100.7"])
        );
    }

    #[test]
    fn backend_error_attachment_includes_readiness() {
        let mut entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", 8765);
        entry.display_name = Some("Maya-Rig".into());
        let diag = crate::gateway::instance_diagnostics::InstanceDiagnostics {
            readiness: Some(dcc_mcp_skill_rest::ReadinessReport {
                process: true,
                dcc: true,
                skill_catalog: true,
                dispatcher: false,
                host_execution_bridge: false,
                main_thread_executor: false,
            }),
            ..Default::default()
        };
        let backend = crate::gateway::instance_diagnostics::backend_error_attachment(
            &entry,
            "http://127.0.0.1:9765/mcp",
            Some(&diag),
            None,
        );
        assert_eq!(backend["dcc_type"], "maya");
        assert_eq!(backend["gateway_mcp_url"], "http://127.0.0.1:9765/mcp");
        assert!(
            !backend["diagnostics"]["readiness"]["dispatcher"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn host_died_records_consistent_last_error_kind() {
        let entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", 8765);
        let store = crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new();
        let err_msg = r#"http://127.0.0.1:8765: HTTP 502: {"kind":"host-died","message":"gone"}"#;
        let kind = if is_host_died_backend_error(err_msg) {
            "host-died"
        } else {
            "backend-error"
        };
        store.record_call_error(entry.instance_id, kind, err_msg);
        let diag = store.get(&entry.instance_id).unwrap();
        assert_eq!(diag.last_error.as_ref().unwrap().kind, "host-died");
    }

    #[test]
    fn host_died_classifier_matches_kebab_and_snake_case() {
        assert!(is_host_died_backend_error(
            r#"HTTP 502: {"error":{"kind":"host-died"}}"#
        ));
        assert!(is_host_died_backend_error("event=host_died"));
        assert!(!is_host_died_backend_error(
            "transport error: connection refused"
        ));
    }

    #[tokio::test]
    async fn host_died_eviction_drops_index_and_registry_row() {
        let registry_dir = tempfile::TempDir::new().expect("tempdir");
        let registry = Arc::new(tokio::sync::RwLock::new(
            FileRegistry::new(registry_dir.path()).expect("registry"),
        ));
        let http_registry = Arc::new(parking_lot::RwLock::new(HttpInstanceRegistry::default()));
        let mut entry =
            dcc_mcp_transport::discovery::types::ServiceEntry::new("maya", "127.0.0.1", 8765);
        entry.instance_id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let key = entry.key();
        registry
            .read()
            .await
            .register(entry.clone())
            .expect("register row");
        http_registry
            .write()
            .register(
                crate::gateway::http_registration::HttpInstanceRegistrationRequest {
                    instance_id: entry.instance_id.to_string(),
                    dcc_type: "maya".to_string(),
                    mcp_url: "http://127.0.0.1:8765/mcp".to_string(),
                    capabilities_fingerprint: None,
                    adapter_version: None,
                    scene: None,
                    ttl_secs: Some(30),
                },
                std::time::SystemTime::now(),
            )
            .expect("register http row");

        let index = Arc::new(CapabilityIndex::new());
        push(
            &index,
            "maya",
            entry.instance_id,
            "maya_scene__list_objects",
            true,
        );
        assert_eq!(
            search_service(
                &index,
                &SearchQuery {
                    query: "objects".into(),
                    ..Default::default()
                }
            )
            .len(),
            1
        );

        let eviction = evict_host_died_instance(&index, &registry, &http_registry, &entry).await;

        assert_eq!(
            eviction,
            HostDiedEviction {
                capability_records_removed: true,
                file_registry_row_removed: true,
                http_registry_row_removed: true,
            }
        );
        assert!(
            registry.read().await.get(&key).is_none(),
            "host-died instance must be removed from the shared registry"
        );
        assert!(
            http_registry
                .read()
                .live_entries(std::time::SystemTime::now())
                .is_empty(),
            "host-died instance must be removed from HTTP registrations"
        );
        assert!(
            search_service(
                &index,
                &SearchQuery {
                    query: "objects".into(),
                    ..Default::default()
                }
            )
            .is_empty(),
            "host-died instance must be removed from the gateway capability index"
        );
    }

    #[test]
    fn backend_failure_kind_preserves_host_busy_and_host_died() {
        assert_eq!(
            backend_failure_kind("HTTP 503", Some(&json!({"kind": "host-busy"}))),
            "host-busy"
        );
        assert_eq!(
            backend_failure_kind("HTTP 502", Some(&json!({"kind": "host-died"}))),
            "host-died"
        );
        assert_eq!(
            backend_failure_kind("queue overloaded (depth=16/16)", None),
            "host-busy"
        );
        assert_eq!(
            backend_failure_kind(
                "HTTP 409",
                Some(&json!({"kind": "thread-affinity-violation"}))
            ),
            "thread-affinity-violation"
        );
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

    #[test]
    fn parse_search_payload_preserves_filters_when_instance_prefix_needs_resolution() {
        let q = parse_search_payload(&json!({
            "query": "sphere",
            "dcc_type": "maya",
            "instance_id": "abc12345",
            "limit": 5,
        }));

        assert_eq!(q.query, "sphere");
        assert_eq!(q.dcc_type.as_deref(), Some("maya"));
        assert_eq!(q.limit, Some(5));
        assert!(q.instance_id.is_none());
    }

    #[test]
    fn parse_search_payload_accepts_full_instance_uuid() {
        let iid = Uuid::parse_str("abcdef01-2345-6789-abcd-ef0123456789").unwrap();
        let q = parse_search_payload(&json!({
            "instance_id": iid.to_string(),
        }));

        assert_eq!(q.instance_id, Some(iid));
    }

    // ── progressive group-aware search hit tests ──────────────────────

    fn make_group_record(
        tool_slug: &str,
        skill_name: Option<&str>,
        loaded: bool,
        tool_group: Option<&str>,
        available_groups: Vec<crate::gateway::capability::CapabilityGroupInfo>,
    ) -> CapabilityRecord {
        let iid = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        CapabilityRecord::new(
            tool_slug.to_string(),
            tool_slug
                .split('.')
                .next_back()
                .unwrap_or(tool_slug)
                .to_string(),
            tool_slug
                .split('.')
                .next_back()
                .unwrap_or(tool_slug)
                .to_string(),
            skill_name.map(str::to_string),
            "Test capability",
            Vec::new(),
            tool_slug.split('.').next().unwrap_or("maya").to_string(),
            iid,
            false,
            loaded,
            tool_group.map(str::to_string),
        )
        .with_available_groups(available_groups)
    }

    #[test]
    fn progressive_group_inactive_generates_activate_next_step() {
        let rec = make_group_record(
            "maya.abcdef01.create_sphere",
            Some("maya-modeling"),
            true,
            Some("modeling"),
            vec![crate::gateway::capability::CapabilityGroupInfo {
                name: "modeling".to_string(),
                description: "Modeling tools".to_string(),
                tools: vec!["create_sphere".to_string()],
                default_active: false,
                active: Some(false),
            }],
        );
        let hit = SearchHit {
            record: rec,
            rank: 1,
            score: 10,
            match_reasons: vec![],
        };
        let row = search_hit_to_value(hit);

        assert_eq!(row["callable"], false);
        assert_eq!(row["load_state"], "loaded");
        assert_eq!(row["disabled_by_group"], "modeling");
        assert_eq!(row["next_step"]["action"], "activate_tool_group");
        assert_eq!(row["next_step"]["arguments"]["skill_name"], "maya-modeling");
        assert_eq!(row["next_step"]["arguments"]["tool_group"], "modeling");
        assert_eq!(row["next_step"]["mcp"]["tool"], "activate_tool_group");
        assert_eq!(row["next_step"]["mcp"]["arguments"]["group"], "modeling");
        // REST block must NOT be present for activate_tool_group
        // (gateway meta-tool, not a capability-indexed action).
        assert!(row["next_step"].get("rest").is_none());
    }

    #[test]
    fn progressive_group_active_generates_describe_next_step() {
        let rec = make_group_record(
            "maya.abcdef01.create_sphere",
            Some("maya-modeling"),
            true,
            Some("modeling"),
            vec![crate::gateway::capability::CapabilityGroupInfo {
                name: "modeling".to_string(),
                description: "Modeling tools".to_string(),
                tools: vec!["create_sphere".to_string()],
                default_active: true,
                active: Some(true),
            }],
        );
        let hit = SearchHit {
            record: rec,
            rank: 1,
            score: 10,
            match_reasons: vec![],
        };
        let row = search_hit_to_value(hit);

        assert_eq!(row["callable"], true);
        assert_eq!(row["load_state"], "loaded");
        assert!(row.get("disabled_by_group").is_none());
        assert_eq!(row["next_step"]["action"], "describe");
        assert_eq!(
            row["next_step"]["arguments"]["tool_slug"],
            "maya.abcdef01.create_sphere"
        );
        // Standard describe next_step must include both MCP and REST.
        assert_eq!(row["next_step"]["rest"]["path"], "/v1/describe");
        assert_eq!(row["next_step"]["mcp"]["tool"], "describe");
    }

    #[test]
    fn loaded_no_group_is_callable_with_describe_next_step() {
        let rec = make_group_record(
            "maya.abcdef01.open_scene",
            Some("maya-scene"),
            true,
            None, // no group
            vec![],
        );
        let hit = SearchHit {
            record: rec,
            rank: 1,
            score: 10,
            match_reasons: vec![],
        };
        let row = search_hit_to_value(hit);

        assert_eq!(row["callable"], true);
        assert_eq!(row["load_state"], "loaded");
        assert!(row.get("disabled_by_group").is_none());
        assert_eq!(row["next_step"]["action"], "describe");
    }
}
