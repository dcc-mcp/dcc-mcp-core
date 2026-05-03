//! Prompts aggregation and routing for the facade gateway (issue #731).
//!
//! Mirrors the shape of [`super::list`] and [`super::call`] for the MCP
//! prompts primitive:
//!
//! * [`aggregate_prompts_list`] fans `prompts/list` out to every live
//!   backend, merges the results, and namespaces each entry with the
//!   same `i_<id8>__<escaped>` / `<id8>.<name>` prefix scheme used for
//!   tools so identical prompt names across multiple DCCs never clash.
//! * [`route_prompts_get`] decodes a prefixed prompt name back to
//!   `(id8, original)` and forwards `prompts/get` to the owning
//!   backend.
//!
//! Both helpers are fail-soft: an unreachable or erroring backend is
//! logged at WARN level and skipped, matching the `tools/list`
//! contract so one stale DCC never 500s the whole gateway endpoint.

use super::*;

use super::super::backend_client::{fetch_prompts, forward_prompts_get};
use super::super::namespace::{encode_tool_name, encode_tool_name_cursor_safe};

/// Build the unified `prompts/list` result by aggregating every live backend.
///
/// Each backend's prompts are emitted under the instance-prefixed form
/// selected by [`GatewayState::cursor_safe_tool_names`]: the preferred
/// cursor-safe `i_<id8>__<escaped>` name introduced in issue #656, or
/// the legacy SEP-986 `<id8>.<name>` form for diagnostic parity. The
/// wire-level namespace is intentionally shared with tool slugs so
/// the same `decode_tool_name` helper can route both primitives.
///
/// Backends that fail or are unreachable are skipped with a WARN log
/// (see [`fetch_prompts`]) — one stale DCC must never 500 the whole
/// aggregated call. A zero-backend gateway returns `{"prompts": []}`
/// rather than a `Method not found` so clients can uniformly call
/// `prompts/list` regardless of topology.
pub async fn aggregate_prompts_list(gs: &GatewayState) -> Value {
    let instances: Vec<_> = live_backends(gs)
        .await
        .into_iter()
        .filter(|e| {
            !matches!(
                e.status,
                dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                    | dcc_mcp_transport::discovery::types::ServiceStatus::Booting
            )
        })
        .collect();
    let client = &gs.http_client;
    let backend_timeout = gs.backend_timeout;
    let cursor_safe = gs.cursor_safe_tool_names;

    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let prompts = fetch_prompts(client, &url, backend_timeout).await;
        (entry.instance_id, entry.dcc_type.clone(), prompts)
    });
    let results = join_all(futs).await;

    let mut prompts: Vec<Value> = Vec::new();
    for (iid, dcc_type, backend_prompts) in results {
        for mut prompt in backend_prompts {
            let encoded = if cursor_safe {
                encode_tool_name_cursor_safe(&iid, &prompt.name)
            } else {
                encode_tool_name(&iid, &prompt.name)
            };
            prompt.name = encoded;
            let mut json_val = serde_json::to_value(&prompt).unwrap_or(Value::Null);
            if let Some(obj) = json_val.as_object_mut() {
                obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
                obj.insert(
                    "_instance_short".to_string(),
                    Value::String(instance_short(&iid)),
                );
                obj.insert("_dcc_type".to_string(), Value::String(dcc_type.clone()));
            }
            prompts.push(json_val);
        }
    }

    json!({ "prompts": prompts })
}

/// Forward a gateway `prompts/get` to the owning backend.
///
/// Returns the backend's raw result envelope on success. Any
/// transport / protocol / not-found condition is mapped to a JSON-RPC
/// error payload (`{ "error": { "code", "message" } }`) that the
/// caller can wrap into a full response.
///
/// Decoding mirrors [`super::call::route_tools_call`]: accepts the
/// preferred cursor-safe form plus the legacy encodings exposed by
/// [`decode_tool_name`]. With a single live backend, a bare
/// (un-prefixed) prompt name is accepted as a convenience so clients
/// can address an un-ambiguous target without going through
/// `prompts/list` first.
pub async fn route_prompts_get(
    gs: &GatewayState,
    name: &str,
    arguments: Option<Value>,
    request_id: Option<String>,
) -> Result<Value, PromptsGetError> {
    let (entry, original) = match decode_tool_name(name) {
        Some((prefix, original)) => match find_instance_by_prefix(gs, &prefix).await {
            Some(entry) => (entry, original),
            None => {
                return Err(PromptsGetError::NoInstanceForPrefix {
                    prefix,
                    full: name.to_string(),
                });
            }
        },
        None => {
            let instances = live_backends(gs).await;
            match instances.len() {
                0 => return Err(PromptsGetError::NoLiveBackend),
                1 => (instances.into_iter().next().unwrap(), name.to_string()),
                _ => return Err(PromptsGetError::AmbiguousBareName(name.to_string())),
            }
        }
    };

    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
    forward_prompts_get(
        &gs.http_client,
        &url,
        &original,
        arguments,
        request_id,
        gs.backend_timeout,
    )
    .await
    .map_err(PromptsGetError::Backend)
}

/// Failure modes for [`route_prompts_get`].
#[derive(Debug)]
pub enum PromptsGetError {
    /// No live backend exposes the 8-hex instance prefix encoded in the
    /// prompt name — typically because the instance shut down between
    /// `prompts/list` and `prompts/get`.
    NoInstanceForPrefix { prefix: String, full: String },
    /// A bare (un-prefixed) prompt name was requested with no live
    /// backends available.
    NoLiveBackend,
    /// A bare (un-prefixed) prompt name was requested while multiple
    /// backends are live — the caller must use a prefixed name to
    /// disambiguate.
    AmbiguousBareName(String),
    /// Backend returned a transport / protocol / error response.
    Backend(String),
}

impl PromptsGetError {
    /// JSON-RPC style error code for this failure.
    ///
    /// * `-32602` (Invalid params) — the prompt name could not be
    ///   resolved to a live backend (no instance, ambiguous bare name,
    ///   no live DCCs at all).
    /// * `-32000` (Implementation-defined server error) — the backend
    ///   itself returned an error or was unreachable.
    pub fn code(&self) -> i64 {
        match self {
            Self::NoInstanceForPrefix { .. } | Self::NoLiveBackend | Self::AmbiguousBareName(_) => {
                -32602
            }
            Self::Backend(_) => -32000,
        }
    }

    /// Human-readable message matching the tool-routing error phrasing.
    pub fn message(&self) -> String {
        match self {
            Self::NoInstanceForPrefix { prefix, full } => {
                format!("No live DCC instance matches prefix '{prefix}' in prompt '{full}'.")
            }
            Self::NoLiveBackend => "No live DCC instances for prompts/get.".to_string(),
            Self::AmbiguousBareName(name) => format!(
                "Ambiguous bare prompt name '{name}' — multiple backends live. \
                 Use the prefixed form from prompts/list."
            ),
            Self::Backend(e) => format!("Backend prompts/get failed: {e}"),
        }
    }
}

/// Compute a fingerprint of the aggregated prompt set across every live
/// backend (issue #731 — mirror of [`super::fingerprint::compute_tools_fingerprint`]).
///
/// Returns a stable, sorted concatenation of `{instance_id}:{prompt_name}`
/// that changes whenever a backend loads or unloads a prompt-bearing
/// skill, so the gateway's watcher task can broadcast a single
/// `notifications/prompts/list_changed` to connected SSE clients.
pub(crate) async fn compute_prompts_fingerprint_with_own(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
    own_host: Option<&str>,
    own_port: u16,
) -> String {
    let instances: Vec<_> = {
        let reg = registry.read().await;
        reg.list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(stale_timeout)
                    && e.dcc_type != GATEWAY_SENTINEL_DCC_TYPE
                    && !matches!(
                        e.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::ShuttingDown
                            | dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                            | dcc_mcp_transport::discovery::types::ServiceStatus::Booting
                    )
                    && match own_host {
                        Some(h) => !super::super::is_own_instance(e, h, own_port),
                        None => true,
                    }
                    && !e.dcc_type.eq_ignore_ascii_case("unknown")
            })
            .collect()
    };

    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let prompts = fetch_prompts(http_client, &url, backend_timeout).await;
        (entry.instance_id, prompts)
    });
    let results = join_all(futs).await;

    let mut parts: Vec<String> = results
        .into_iter()
        .flat_map(|(iid, prompts)| {
            prompts
                .into_iter()
                .map(move |p| format!("{iid}:{}", p.name))
                .collect::<Vec<_>>()
        })
        .collect();
    parts.sort_unstable();
    parts.join("|")
}

/// Public wrapper for [`compute_prompts_fingerprint_with_own`] that
/// disables the own-instance filter. Kept symmetric with
/// [`super::fingerprint::compute_tools_fingerprint`].
pub async fn compute_prompts_fingerprint(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
) -> String {
    compute_prompts_fingerprint_with_own(
        registry,
        stale_timeout,
        http_client,
        backend_timeout,
        None,
        0,
    )
    .await
}
