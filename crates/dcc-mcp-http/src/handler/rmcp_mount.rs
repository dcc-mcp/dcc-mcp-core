//! Mounts the rmcp-backed MCP endpoint.
//!
//! This module creates a [`StreamableHttpService`] backed by our
//! [`DccMcpHandler`] and attaches it to the axum router as a nested service.
//!
//! # Usage (called from `server/mod.rs` behind `#[cfg(feature = "rmcp-transport")]`)
//!
//! ```ignore
//! router = rmcp_mount::attach_rmcp_endpoint(router, app_state);
//! ```

use std::sync::Arc;

use axum::Router;
#[cfg(feature = "mcp-2026-07-28")]
use axum::body::{Body, to_bytes};
use dcc_mcp_http_server::rmcp_handler::{DccMcpHandler, RegistryContext};
use dcc_mcp_jsonrpc::NotificationBuilder;
#[cfg(feature = "mcp-2026-07-28")]
use http::{Request, Response, StatusCode, header::HeaderValue};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
#[cfg(feature = "mcp-2026-07-28")]
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
pub fn attach_rmcp_endpoint(router: Router, app_state: &AppState) -> Router {
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
            async move { dispatch_request(request, legacy, stateless).await }
        });
        info!("rmcp MCP endpoint mounted at /mcp (legacy + 2026 stateless dispatcher)");
        return router.nest_service("/mcp", dispatcher);
    }

    #[cfg(not(feature = "mcp-2026-07-28"))]
    {
        info!("rmcp MCP endpoint mounted at /mcp");
        router.nest_service("/mcp", service)
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
) -> Result<Response<Body>, std::convert::Infallible> {
    use dcc_mcp_jsonrpc::{
        JsonRpcRequest, MCP_PROTOCOL_VERSION_HEADER, MCP_SESSION_HEADER, ProtocolMode,
        ProtocolRequestHints, select_protocol_mode_from_headers,
    };

    let headers = request.headers();
    let hints = ProtocolRequestHints {
        protocol_version: headers
            .get(MCP_PROTOCOL_VERSION_HEADER)
            .and_then(|value| value.to_str().ok()),
        has_session_id: headers.contains_key(MCP_SESSION_HEADER),
        accept: headers.get("accept").and_then(|value| value.to_str().ok()),
        method: headers
            .get("mcp-method")
            .and_then(|value| value.to_str().ok()),
        name: headers
            .get("mcp-name")
            .and_then(|value| value.to_str().ok()),
    };

    if select_protocol_mode_from_headers(hints) != ProtocolMode::Stateless {
        return legacy
            .oneshot(request)
            .await
            .map(|response| response.map(Body::new));
    }

    if request.method() != http::Method::POST {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header(http::header::ALLOW, "POST")
            .body(Body::empty())
            .expect("valid response"));
    }

    let body = match to_bytes(request.into_body(), 16 * 1024 * 1024).await {
        Ok(body) => body,
        Err(error) => return Ok(json_error_response(None, -32700, error.to_string())),
    };
    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(error) => return Ok(json_error_response(None, -32700, error.to_string())),
    };

    let response = stateless.handle_request(&req).await;
    let mut builder = Response::builder().status(if response.is_some() {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    });
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
fn json_error_response(
    id: Option<serde_json::Value>,
    code: i64,
    message: String,
) -> Response<Body> {
    use dcc_mcp_jsonrpc::JsonRpcResponse;
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(dcc_mcp_jsonrpc::JsonRpcError {
            code,
            message,
            data: None,
        }),
    };
    Response::builder()
        .status(StatusCode::OK)
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
