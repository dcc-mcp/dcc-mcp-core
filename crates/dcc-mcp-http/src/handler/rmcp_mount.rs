//! Mounts the rmcp-backed MCP endpoint.
//!
//! This module creates a [`StreamableHttpService`] backed by our
//! [`DccMcpHandler`] and attaches it to the axum router as a nested service.
//!
//! # Usage (called from `server/mod.rs` behind `#[cfg(feature = "rmcp-transport")]`)
//!
//! ```ignore
//! router = rmcp_mount::attach_rmcp_endpoint(router, app_state, max_request_body_bytes);
//! ```

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use dcc_mcp_http_server::rmcp_handler::{DccMcpHandler, RegistryContext};
use dcc_mcp_http_server::rmcp_providers::SkillProvider;
use dcc_mcp_jsonrpc::NotificationBuilder;
#[cfg(feature = "mcp-2026-07-28")]
use http::header::HeaderValue;
use http::{Request, Response, StatusCode};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{Value, json};
use tower::ServiceExt;
use tracing::info;

use super::rmcp_providers_impl::{PromptRegistryProvider, ResourceRegistryProvider};
use super::skill_provider::SkillCatalogProvider;
use crate::handler::AppState;

/// Attach the rmcp endpoint at `/mcp`.
///
/// The endpoint handles all MCP methods (`initialize`, `tools/list`,
/// `tools/call`, `resources/*`, `prompts/*`, `logging/setLevel`) via the
/// [`DccMcpHandler`] adapter. Sessions are managed by rmcp's
/// [`LocalSessionManager`].
///
/// The router passed in is already state-erased (`Router<()>`) because
/// `.with_state()` was called earlier in the builder chain.
pub fn attach_rmcp_endpoint(
    router: Router,
    app_state: &AppState,
    max_request_body_bytes: usize,
) -> Router {
    let registry_context = build_registry_context(app_state);
    let legacy_registry_context = Arc::new(RegistryContext {
        skill_provider: None,
        ..registry_context.as_ref().clone()
    });
    let service = build_legacy_service(app_state, legacy_registry_context, max_request_body_bytes);
    let server_state = Arc::new(app_state.server.clone());

    #[cfg(feature = "mcp-2026-07-28")]
    {
        let stateless = dcc_mcp_http_server::stateless::StatelessMcpService::new(
            app_state.server.clone(),
            registry_context,
        );
        let dispatcher = tower::service_fn(move |request: Request<Body>| {
            let legacy = service.clone();
            let stateless = stateless.clone();
            let server_state = server_state.clone();
            async move {
                dispatch_request(
                    request,
                    legacy,
                    stateless,
                    server_state,
                    max_request_body_bytes,
                )
                .await
            }
        });
        info!("rmcp MCP endpoint mounted at /mcp (legacy + 2026 stateless dispatcher)");
        return router.nest_service("/mcp", dispatcher);
    }

    #[cfg(not(feature = "mcp-2026-07-28"))]
    {
        let dispatcher = tower::service_fn(move |request: Request<Body>| {
            let legacy = service.clone();
            let server_state = server_state.clone();
            async move {
                dispatch_legacy_request(request, legacy, max_request_body_bytes, server_state).await
            }
        });
        info!("rmcp MCP endpoint mounted at /mcp");
        router.nest_service("/mcp", dispatcher)
    }
}

/// Build the provider and readiness context shared by legacy and stateless
/// protocol handlers.
pub(crate) fn build_registry_context(app_state: &AppState) -> Arc<RegistryContext> {
    // Build provider trait objects that bridge registries into the handler.
    let resource_provider: Option<Arc<dyn dcc_mcp_http_server::rmcp_providers::ResourceProvider>> =
        if app_state.server.features.enable_resources {
            Some(Arc::new(ResourceRegistryProvider {
                registry: app_state.resources.clone(),
            }))
        } else {
            None
        };

    let skill_provider: Option<Arc<dyn dcc_mcp_http_server::rmcp_providers::SkillProvider>> =
        if app_state.server.features.enable_resources
            && app_state.server.features.enable_skills_extension
        {
            Some(Arc::new(SkillCatalogProvider))
        } else {
            None
        };

    let prompt_provider: Option<Arc<dyn dcc_mcp_http_server::rmcp_providers::PromptProvider>> =
        if app_state.server.features.enable_prompts {
            Some(Arc::new(PromptRegistryProvider {
                registry: app_state.prompts.clone(),
            }))
        } else {
            None
        };

    let prompts = app_state.prompts.clone();
    let server_hook = app_state.server.clone();
    let enable_prompt_broadcast = app_state.server.features.enable_prompts;

    let on_skill_catalog_mutated: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        prompts.invalidate();
        if !enable_prompt_broadcast {
            return;
        }
        let event = NotificationBuilder::new("notifications/prompts/list_changed")
            .with_empty_params()
            .as_sse_event();
        for sid in server_hook.sessions.all_ids() {
            server_hook.sessions.push_event(&sid, event.clone());
        }
    });

    Arc::new(RegistryContext {
        resource_provider,
        skill_provider,
        prompt_provider,
        readiness: app_state.readiness.clone(),
        on_skill_catalog_mutated,
    })
}

/// Construct the existing rmcp service. Keeping this separate makes the
/// legacy service an explicit fallback for protocol dispatch.
pub(crate) fn build_legacy_service(
    app_state: &AppState,
    registry_context: Arc<RegistryContext>,
    max_request_body_bytes: usize,
) -> StreamableHttpService<DccMcpHandler, LocalSessionManager> {
    let server_state = app_state.server.clone();

    let session_manager = Arc::new(LocalSessionManager::default());

    let mut config = StreamableHttpServerConfig::default();
    // Keep the established JSON request/response lifecycle for legacy clients.
    // rmcp routes 2026-07-28 requests statelessly on the same endpoint, while
    // legacy requests remain compatible with the pre-extension transport.
    config.legacy_session_mode = false;
    config.json_response = true;
    config.max_request_body_bytes = max_request_body_bytes;
    // Allow any host (production should restrict via reverse proxy).
    config.allowed_hosts = vec![];

    StreamableHttpService::new(
        move || {
            Ok(DccMcpHandler::new(
                server_state.clone(),
                registry_context.clone(),
            ))
        },
        session_manager,
        config,
    )
}

#[cfg(feature = "mcp-2026-07-28")]
async fn dispatch_request(
    request: Request<Body>,
    legacy: StreamableHttpService<DccMcpHandler, LocalSessionManager>,
    stateless: dcc_mcp_http_server::stateless::StatelessMcpService,
    server_state: Arc<dcc_mcp_http_server::ServerState>,
    max_request_body_bytes: usize,
) -> Result<Response<Body>, std::convert::Infallible> {
    use dcc_mcp_jsonrpc::{
        InboundRoute, JsonRpcResponse, MCP_PROTOCOL_VERSION_HEADER, ProtocolRequestHints,
        SUPPORTED_MODERN_PROTOCOL_VERSIONS, classify_protocol_request,
    };
    // Body-less session operations stay entirely on the legacy transport.
    if request.method() != http::Method::POST {
        return legacy
            .oneshot(request)
            .await
            .map(|response| response.map(Body::new));
    }

    let (parts, body) = request.into_parts();
    let body = match read_bounded_body(body, max_request_body_bytes).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    // Join duplicates as Fetch does; accepting only the first would hide a
    // contradictory header. Do not rewrite headers on the legacy request.
    let header = |name: &str| {
        let values: Vec<_> = parts
            .headers
            .get_all(name)
            .iter()
            .map(|value| {
                value
                    .to_str()
                    .map(|value| value.trim_matches([' ', '\t']))
                    .unwrap_or("\0")
            })
            .collect();
        (!values.is_empty()).then(|| values.join(", "))
    };
    let version = header(MCP_PROTOCOL_VERSION_HEADER);
    let method = header("Mcp-Method");
    let name = header("Mcp-Name");
    let hints = ProtocolRequestHints {
        protocol_version: version.as_deref(),
        method: method.as_deref(),
        name: name.as_deref(),
        ..Default::default()
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_)
            if version
                .as_deref()
                .is_some_and(|v| v.trim_matches([' ', '\t']) >= "2026-07-28") =>
        {
            return Ok(json_error_response(JsonRpcResponse::parse_error()));
        }
        Err(_) => {
            return legacy
                .oneshot(Request::from_parts(parts, Body::from(body)))
                .await
                .map(|r| r.map(Body::new));
        }
    };
    if is_jsonrpc_response(&parsed) {
        return Ok(accepted_empty_response());
    }
    if matches!(version.as_deref(), None | Some("2025-11-25")) && method.is_none() {
        if let Some(response) = legacy_skill_response(&parsed, &server_state) {
            return Ok(response);
        }
    }
    let req =
        match classify_protocol_request("POST", hints, &parsed, SUPPORTED_MODERN_PROTOCOL_VERSIONS)
        {
            Ok(InboundRoute::Legacy) => {
                let body = prepare_legacy_body(parsed);
                let is_initialize =
                    body.get("method").and_then(serde_json::Value::as_str) == Some("initialize");
                let mut parts = parts;
                if is_initialize {
                    parts.headers.remove(MCP_PROTOCOL_VERSION_HEADER);
                }
                let response = legacy
                    .oneshot(Request::from_parts(parts, Body::from(body.to_string())))
                    .await
                    .map(|r| r.map(Body::new));
                if !is_initialize {
                    return response;
                }
                let response = match response {
                    Ok(response) => response,
                    Err(error) => return Err(error),
                };
                return Ok(normalize_legacy_initialize_response(
                    response,
                    &body,
                    max_request_body_bytes,
                )
                .await);
            }
            Ok(InboundRoute::Modern(req)) => req,
            Err(response) => return Ok(json_error_response(*response)),
        };
    use dcc_mcp_http_server::stateless::StatelessDispatchOutcome;
    let outcome = match stateless.parameter_header_error(&req, header) {
        Some(error) => error,
        None => stateless.handle_request_with_outcome(&req).await,
    };
    let status = match &outcome {
        StatelessDispatchOutcome::Notification => StatusCode::ACCEPTED,
        StatelessDispatchOutcome::Response(_) => StatusCode::OK,
        StatelessDispatchOutcome::InvalidEnvelope(_) => StatusCode::BAD_REQUEST,
        StatelessDispatchOutcome::MethodNotFound(_) => StatusCode::NOT_FOUND,
    };
    let response = outcome.into_response();
    let mut builder = Response::builder().status(status);
    builder = builder.header(
        MCP_PROTOCOL_VERSION_HEADER,
        HeaderValue::from_static("2026-07-28"),
    );
    if let Some(response) = response {
        Ok(builder
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(response.to_string()))
            .expect("valid response"))
    } else {
        Ok(builder.body(Body::empty()).expect("valid response"))
    }
}

#[cfg(feature = "mcp-2026-07-28")]
fn is_jsonrpc_response(value: &serde_json::Value) -> bool {
    value.get("method").is_none() && (value.get("result").is_some() || value.get("error").is_some())
}

#[cfg(feature = "mcp-2026-07-28")]
fn accepted_empty_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .expect("valid acknowledgement response")
}

fn prepare_legacy_body(mut value: serde_json::Value) -> serde_json::Value {
    if value.get("method").and_then(serde_json::Value::as_str) != Some("tools/call") {
        return value;
    }
    let Some(params) = value
        .get_mut("params")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return value;
    };
    let Some(meta) = params.get("_meta").cloned() else {
        return value;
    };
    let arguments = params
        .entry("arguments")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(arguments) = arguments.as_object_mut() {
        arguments.entry("_meta").or_insert(meta);
    }
    value
}

async fn normalize_legacy_initialize_response(
    response: Response<Body>,
    request: &Value,
    max_request_body_bytes: usize,
) -> Response<Body> {
    let requested = request
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str);
    let negotiated = dcc_mcp_jsonrpc::negotiate_protocol_version(requested);
    let (parts, body) = response.into_parts();
    let body = match to_bytes(body, max_request_body_bytes).await {
        Ok(body) => body,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };
    let mut payload = match serde_json::from_slice::<Value>(&body) {
        Ok(payload) => payload,
        Err(_) => return Response::from_parts(parts, Body::from(body)),
    };
    if payload.pointer("/result/protocolVersion").is_some() {
        payload["result"]["protocolVersion"] = Value::String(negotiated.to_string());
    }
    Response::from_parts(parts, Body::from(payload.to_string()))
}

#[cfg(not(feature = "mcp-2026-07-28"))]
async fn dispatch_legacy_request(
    request: Request<Body>,
    legacy: StreamableHttpService<DccMcpHandler, LocalSessionManager>,
    max_request_body_bytes: usize,
    server_state: Arc<dcc_mcp_http_server::ServerState>,
) -> Result<Response<Body>, std::convert::Infallible> {
    let request = if request.method() == http::Method::POST {
        let (parts, body) = request.into_parts();
        let body = match read_bounded_body(body, max_request_body_bytes).await {
            Ok(body) => body,
            Err(response) => return Ok(response),
        };
        Request::from_parts(parts, Body::from(body))
    } else {
        request
    };
    let (mut parts, body) = request.into_parts();
    let body = match read_bounded_body(body, max_request_body_bytes).await {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let request_value = serde_json::from_slice::<Value>(&body).ok();
    if let Some(value) = request_value.as_ref()
        && let Some(response) = legacy_skill_response(value, &server_state)
    {
        return Ok(response);
    }
    if request_value.as_ref().is_some_and(is_jsonrpc_response) {
        return Ok(accepted_empty_response());
    }
    let is_initialize = request_value
        .as_ref()
        .and_then(|value| value.get("method"))
        .and_then(Value::as_str)
        == Some("initialize");
    if is_initialize {
        parts
            .headers
            .remove(dcc_mcp_jsonrpc::MCP_PROTOCOL_VERSION_HEADER);
    }
    let body = request_value
        .clone()
        .map(prepare_legacy_body)
        .map(|value| value.to_string().into_bytes())
        .unwrap_or_else(|| body.to_vec());
    let request = Request::from_parts(parts, Body::from(body));
    let response = legacy.oneshot(request).await.map(|r| r.map(Body::new));
    if !is_initialize {
        return response;
    }
    let response = match response {
        Ok(response) => response,
        Err(error) => return Err(error),
    };
    let Some(request_value) = request_value.as_ref() else {
        return Ok(response);
    };
    Ok(normalize_legacy_initialize_response(response, request_value, max_request_body_bytes).await)
}

#[cfg(not(feature = "mcp-2026-07-28"))]
fn is_jsonrpc_response(value: &Value) -> bool {
    value.get("method").is_none() && (value.get("result").is_some() || value.get("error").is_some())
}

#[cfg(not(feature = "mcp-2026-07-28"))]
fn accepted_empty_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .expect("valid acknowledgement response")
}

fn legacy_skill_response(
    request: &Value,
    state: &dcc_mcp_http_server::ServerState,
) -> Option<Response<Body>> {
    let method = request.get("method").and_then(Value::as_str)?;
    let provider = SkillCatalogProvider;
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let result = match method {
        "skills/list" => serde_json::to_value(provider.list_skills(&state.catalog))
            .map_err(|error| error.to_string()),
        "skills/get" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let params: dcc_mcp_jsonrpc::GetSkillParams = serde_json::from_value(params).ok()?;
            provider
                .get_skill(&params.uri, &state.catalog)
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        "resources/directory/read" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let params: dcc_mcp_jsonrpc::ReadResourceDirectoryParams =
                serde_json::from_value(params).ok()?;
            provider
                .read_skill_directory(&params.uri, &state.catalog)
                .map_err(|error| error.to_string())
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        _ => return None,
    };
    let response = match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(message) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32602, "message": message}
        }),
    };
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(response.to_string()))
            .expect("valid JSON response"),
    )
}

/// The same bounded stream read protects both feature configurations. In
/// particular rmcp must not turn an unknown-length overflow into HTTP 500.
async fn read_bounded_body(body: Body, limit: usize) -> Result<Bytes, Response<Body>> {
    to_bytes(body, limit).await.map_err(|error| {
        Response::builder()
            .status(if body_limit_exceeded(&error) {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            })
            .body(Body::empty())
            .expect("valid body-read response")
    })
}

fn body_limit_exceeded(error: &(dyn std::error::Error + 'static)) -> bool {
    // The router's RequestBodyLimitLayer and axum Body each wrap the stream
    // error. Follow its typed source chain rather than matching one wrapper
    // depth or depending on an unstable diagnostic string.
    let mut current = Some(error);
    while let Some(error) = current {
        if error.is::<http_body_util::LengthLimitError>() {
            return true;
        }
        current = error.source();
    }
    false
}

#[cfg(feature = "mcp-2026-07-28")]
fn json_error_response(response: dcc_mcp_jsonrpc::JsonRpcResponse) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(dcc_mcp_jsonrpc::MCP_PROTOCOL_VERSION_HEADER, "2026-07-28")
        .body(Body::from(
            serde_json::to_vec(&response).expect("JSON-RPC error serializes"),
        ))
        .expect("valid response")
}

#[cfg(all(test, feature = "mcp-2026-07-28"))]
mod tests {
    use dcc_mcp_jsonrpc::{ProtocolMode, ProtocolRequestHints, select_protocol_mode_from_headers};

    #[test]
    fn explicit_2026_header_uses_stateless_dispatch() {
        assert_eq!(
            select_protocol_mode_from_headers(ProtocolRequestHints {
                protocol_version: Some("2026-07-28"),
                ..Default::default()
            }),
            ProtocolMode::Stateless
        );
    }

    #[test]
    fn absent_or_legacy_header_uses_legacy_dispatch() {
        for protocol_version in [None, Some("2025-06-18"), Some("2025-03-26")] {
            assert_eq!(
                select_protocol_mode_from_headers(ProtocolRequestHints {
                    protocol_version,
                    ..Default::default()
                }),
                ProtocolMode::Session
            );
        }
    }
}
