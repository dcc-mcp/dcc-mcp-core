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
use dcc_mcp_jsonrpc::NotificationBuilder;
#[cfg(feature = "mcp-2026-07-28")]
use http::header::HeaderValue;
use http::{Request, Response, StatusCode};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tower::ServiceExt;
use tracing::info;

use super::rmcp_providers_impl::{PromptRegistryProvider, ResourceRegistryProvider};
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
    let service = build_legacy_service(app_state, registry_context.clone());

    #[cfg(feature = "mcp-2026-07-28")]
    {
        let stateless = dcc_mcp_http_server::stateless::StatelessMcpService::new(
            app_state.server.clone(),
            registry_context,
        );
        let dispatcher = tower::service_fn(move |request: Request<Body>| {
            let legacy = service.clone();
            let stateless = stateless.clone();
            async move { dispatch_request(request, legacy, stateless, max_request_body_bytes).await }
        });
        info!("rmcp MCP endpoint mounted at /mcp (legacy + 2026 stateless dispatcher)");
        return router.nest_service("/mcp", dispatcher);
    }

    #[cfg(not(feature = "mcp-2026-07-28"))]
    {
        let dispatcher = tower::service_fn(move |request: Request<Body>| {
            let legacy = service.clone();
            async move { dispatch_legacy_request(request, legacy, max_request_body_bytes).await }
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
) -> StreamableHttpService<DccMcpHandler, LocalSessionManager> {
    let server_state = app_state.server.clone();

    let session_manager = Arc::new(LocalSessionManager::default());

    let mut config = StreamableHttpServerConfig::default();
    // Stateless + JSON-direct mode: each request is independent and
    // responses are plain application/json (no SSE framing). This is
    // compliant with MCP Streamable HTTP spec (2025-06-18) and matches
    // the DCC embedding scenario where the server has at most one active
    // client and does not need cross-request session state.
    config.stateful_mode = false;
    config.json_response = true;
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
            .map(|value| value.to_str().unwrap_or("<invalid-header>"))
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
    let req =
        match classify_protocol_request("POST", hints, &parsed, SUPPORTED_MODERN_PROTOCOL_VERSIONS)
        {
            Ok(InboundRoute::Legacy) => {
                return legacy
                    .oneshot(Request::from_parts(parts, Body::from(body)))
                    .await
                    .map(|r| r.map(Body::new));
            }
            Ok(InboundRoute::Modern(req)) => req,
            Err(response) => return Ok(json_error_response(response)),
        };
    use dcc_mcp_http_server::stateless::StatelessDispatchOutcome;
    let outcome = stateless.handle_request_with_outcome(&req).await;
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

#[cfg(not(feature = "mcp-2026-07-28"))]
async fn dispatch_legacy_request(
    request: Request<Body>,
    legacy: StreamableHttpService<DccMcpHandler, LocalSessionManager>,
    max_request_body_bytes: usize,
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
    legacy.oneshot(request).await.map(|r| r.map(Body::new))
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
