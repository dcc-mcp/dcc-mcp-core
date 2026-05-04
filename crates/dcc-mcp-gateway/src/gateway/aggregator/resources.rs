//! Resources-list aggregation + fingerprint for the facade gateway (#732).
//!
//! Mirrors the shape of [`super::list`] / [`super::fingerprint`] for tools, so
//! the gateway fan-outs `resources/list` to every live backend, prefixes each
//! backend URI with the 8-char instance id, and merges the result with the
//! existing `dcc://<type>/<id>` admin pointers.
//!
//! Fail-soft: a backend that is unreachable on `resources/list` contributes
//! zero entries and emits a `warn!`; the fan-out does not surface an error
//! to the caller. Mirrors the [`fetch_tools`] / [`super::list`] behaviour.

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};

use super::super::backend_client::fetch_resources;
use super::super::namespace::encode_resource_uri;
use super::super::state::GatewayState;
use super::helpers::live_backends;
use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry, ServiceStatus};

fn backend_mcp_url(entry: &ServiceEntry) -> String {
    format!("http://{}:{}/mcp", entry.host, entry.port)
}

/// Full eligibility filter used by the fingerprint path, which starts
/// from the raw `FileRegistry::list_all()` and must therefore replicate
/// every filter `live_instances` applies — minus the `allow_unknown_tools`
/// toggle, which the fingerprint keeps permissive to avoid emitting a
/// spurious `resources/list_changed` when the toggle flips.
fn is_registry_row_eligible_for_resources(entry: &ServiceEntry) -> bool {
    entry.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
        && !entry.dcc_type.eq_ignore_ascii_case("unknown")
        && !matches!(
            entry.status,
            ServiceStatus::ShuttingDown | ServiceStatus::Unreachable | ServiceStatus::Booting
        )
}

async fn fetch_resources_for_entries(
    entries: &[ServiceEntry],
    client: &reqwest::Client,
    backend_timeout: Duration,
) -> Vec<(uuid::Uuid, String, Vec<Value>)> {
    let futs = entries.iter().map(|entry| async move {
        let url = backend_mcp_url(entry);
        let resources = fetch_resources(client, &url, backend_timeout).await;
        (entry.instance_id, entry.dcc_type.clone(), resources)
    });
    join_all(futs).await
}

/// Fetch every live backend's `resources/list` and return `(instance_id, dcc_type, resources)`
/// triples. Backends that fail the fetch contribute an empty vector (see
/// [`fetch_resources`] — fail-soft by design).
pub(crate) async fn fetch_backend_resources(
    gs: &GatewayState,
) -> Vec<(uuid::Uuid, String, Vec<Value>)> {
    // `live_instances` already filters out sentinel rows, own row, stale
    // rows, and rows in `ShuttingDown | Unreachable | Booting`, and
    // respects the `allow_unknown_tools` toggle — no further filter is
    // needed here.
    let instances = live_backends(gs).await;
    fetch_resources_for_entries(&instances, &gs.http_client, gs.backend_timeout).await
}

/// Build the unified `resources/list` result.
///
/// Layout:
/// 1. Admin `dcc://<type>/<id>` pointers for every live DCC instance — the
///    existing administrative affordance. These are retained so operators
///    can still read per-instance metadata.
/// 2. Backend-contributed resources from every live instance, each URI
///    rewritten to `<scheme>://<id8>/<rest>` so multiple backends can
///    expose the same scheme without collision.
///
/// Fail-soft: one unreachable backend contributes zero resources; the
/// healthy backends' resources are still returned, and `tools/list`-style
/// partial aggregation semantics hold.
pub async fn aggregate_resources_list(gs: &GatewayState) -> Value {
    // Tier 1: admin instance pointers — same payload the handler used
    // before #732, kept as an operator affordance.
    let admin_pointers: Vec<Value> = {
        let registry = gs.registry.read().await;
        gs.live_instances(&registry)
            .into_iter()
            .filter(|entry| entry.dcc_type != GATEWAY_SENTINEL_DCC_TYPE)
            .map(|entry| {
                let name = match entry.scene.as_deref() {
                    Some(scene) if !scene.is_empty() => {
                        format!(
                            "{} — {} ({}:{})",
                            entry.dcc_type, scene, entry.host, entry.port
                        )
                    }
                    _ => format!("{} @ {}:{}", entry.dcc_type, entry.host, entry.port),
                };
                json!({
                    "uri": format!("dcc://{}/{}", entry.dcc_type, entry.instance_id),
                    "name": name,
                    "description": format!(
                        "Live {} DCC instance. Version: {}.",
                        entry.dcc_type,
                        entry.version.as_deref().unwrap_or("unknown")
                    ),
                    "mimeType": "application/json"
                })
            })
            .collect()
    };

    // Tier 2: every backend's resources, with URIs rewritten to the
    // gateway-prefixed form so clients can disambiguate.
    let mut merged: Vec<Value> = admin_pointers;
    for (iid, dcc_type, backend_resources) in fetch_backend_resources(gs).await {
        for mut resource in backend_resources {
            let backend_uri = resource
                .get("uri")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let Some(backend_uri) = backend_uri else {
                // Backend emitted a resource without a URI — skip rather than
                // surface a malformed entry upstream.
                continue;
            };
            let Some(prefixed) = encode_resource_uri(&iid, &backend_uri) else {
                // URI had no `://`; cannot safely prefix. Drop it.
                tracing::warn!(
                    instance_id = %iid,
                    dcc_type = %dcc_type,
                    uri = %backend_uri,
                    "Gateway: backend resource URI has no scheme — skipping",
                );
                continue;
            };
            if let Some(obj) = resource.as_object_mut() {
                obj.insert("uri".to_string(), Value::String(prefixed));
                // Annotate with origin so agents can display context — same
                // idea as tools' `_instance_id` / `_dcc_type` injection.
                obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
                obj.insert("_dcc_type".to_string(), Value::String(dcc_type.clone()));
            }
            merged.push(resource);
        }
    }

    json!({"resources": merged})
}

/// Compute a fingerprint of the aggregated resource set across every live
/// backend.
///
/// Mirrors [`super::compute_tools_fingerprint_with_own`]: a stable, sorted
/// concatenation of `{instance_id}:{backend_uri}` tuples. The watcher in
/// [`super::super::tasks`] polls this value on a fixed cadence and emits a
/// single `notifications/resources/list_changed` whenever it changes.
///
/// Deliberately excludes resource `name` / `description` / `mimeType` — we
/// only want set-level add/remove detection, not metadata edits (the spec
/// does not give clients a way to re-fetch individual resources on mutation
/// so emitting a list_changed for a pure description edit would be
/// wasteful churn).
pub(crate) async fn compute_resources_fingerprint_with_own(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
    own_host: Option<&str>,
    own_port: u16,
) -> String {
    let instances: Vec<ServiceEntry> = {
        let reg = registry.read().await;
        reg.list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(stale_timeout)
                    && is_registry_row_eligible_for_resources(e)
                    && match own_host {
                        Some(h) => !super::super::is_own_instance(e, h, own_port),
                        None => true,
                    }
            })
            .collect()
    };

    let results = fetch_resources_for_entries(&instances, http_client, backend_timeout).await;

    let mut parts: Vec<String> = results
        .into_iter()
        .flat_map(|(iid, _, resources)| {
            resources
                .into_iter()
                .filter_map(|r| {
                    r.get("uri")
                        .and_then(Value::as_str)
                        .map(|u| format!("{iid}:{u}"))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    parts.sort_unstable();
    parts.join("|")
}
