//! Built-in [`MethodHandler`] wrappers for every JSON-RPC method that
//! `dispatch_request` historically routed via a hand-coded `match`.
//!
//! Each wrapper is a unit struct that delegates to the existing free
//! function in [`crate::handler::dispatch`] or [`crate::handlers`].
//! Capability gating (`enable_resources` / `enable_prompts`) lives here
//! so the router itself stays free of feature flags. When a capability
//! is disabled the wrapper returns `method_not_found` — exact parity
//! with the previous fall-through to the wildcard arm.

use std::sync::Arc;

use super::router::{EmptyAckHandler, HandlerFuture, MethodHandler, MethodRouter};
use super::state::AppState;
use crate::error::HttpError;
use crate::handlers::{
    handle_elicitation_create, handle_logging_set_level, handle_prompts_get, handle_prompts_list,
    handle_resources_list, handle_resources_read, handle_resources_subscribe,
    handle_resources_unsubscribe, handle_tools_call,
};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse, LOGGING_SET_LEVEL_METHOD};

/// Register every built-in MCP method into `router`.
pub(crate) fn register_builtins(router: &MethodRouter) {
    router.register("initialize", Arc::new(InitializeHandler));
    router.register("notifications/initialized", Arc::new(EmptyAckHandler));
    router.register("ping", Arc::new(EmptyAckHandler));
    router.register(LOGGING_SET_LEVEL_METHOD, Arc::new(LoggingSetLevelHandler));
    router.register("tools/list", Arc::new(ToolsListHandler));
    router.register("tools/call", Arc::new(ToolsCallHandler));
    router.register("resources/list", Arc::new(ResourcesListHandler));
    router.register("resources/read", Arc::new(ResourcesReadHandler));
    router.register("resources/subscribe", Arc::new(ResourcesSubscribeHandler));
    router.register(
        "resources/unsubscribe",
        Arc::new(ResourcesUnsubscribeHandler),
    );
    router.register("prompts/list", Arc::new(PromptsListHandler));
    router.register("prompts/get", Arc::new(PromptsGetHandler));
    router.register("elicitation/create", Arc::new(ElicitationCreateHandler));
}

/// Helper: emit `method_not_found` when a capability gate fails.
fn capability_disabled(req: &JsonRpcRequest) -> Result<JsonRpcResponse, HttpError> {
    Ok(JsonRpcResponse::method_not_found(
        req.id.clone(),
        &req.method,
    ))
}

// ── core ───────────────────────────────────────────────────────────────

pub(crate) struct InitializeHandler;
impl MethodHandler for InitializeHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(super::dispatch::handle_initialize(state, req, session_id))
    }
}

pub(crate) struct LoggingSetLevelHandler;
impl MethodHandler for LoggingSetLevelHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(handle_logging_set_level(state, req, session_id))
    }
}

pub(crate) struct ToolsListHandler;
impl MethodHandler for ToolsListHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(super::dispatch::handle_tools_list(state, req, session_id))
    }
}

pub(crate) struct ToolsCallHandler;
impl MethodHandler for ToolsCallHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(handle_tools_call(state, req, session_id))
    }
}

pub(crate) struct ElicitationCreateHandler;
impl MethodHandler for ElicitationCreateHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(handle_elicitation_create(state, req, session_id))
    }
}

// ── resources/* (gated on `state.enable_resources`) ────────────────────

pub(crate) struct ResourcesListHandler;
impl MethodHandler for ResourcesListHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_resources {
                return capability_disabled(req);
            }
            handle_resources_list(state, req).await
        })
    }
}

pub(crate) struct ResourcesReadHandler;
impl MethodHandler for ResourcesReadHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_resources {
                return capability_disabled(req);
            }
            handle_resources_read(state, req).await
        })
    }
}

pub(crate) struct ResourcesSubscribeHandler;
impl MethodHandler for ResourcesSubscribeHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_resources {
                return capability_disabled(req);
            }
            handle_resources_subscribe(state, req, session_id).await
        })
    }
}

pub(crate) struct ResourcesUnsubscribeHandler;
impl MethodHandler for ResourcesUnsubscribeHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_resources {
                return capability_disabled(req);
            }
            handle_resources_unsubscribe(state, req, session_id).await
        })
    }
}

// ── prompts/* (gated on `state.enable_prompts`) ────────────────────────

pub(crate) struct PromptsListHandler;
impl MethodHandler for PromptsListHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_prompts {
                return capability_disabled(req);
            }
            handle_prompts_list(state, req).await
        })
    }
}

pub(crate) struct PromptsGetHandler;
impl MethodHandler for PromptsGetHandler {
    fn handle<'a>(
        &'a self,
        state: &'a AppState,
        req: &'a JsonRpcRequest,
        _session_id: Option<&'a str>,
    ) -> HandlerFuture<'a> {
        Box::pin(async move {
            if !state.enable_prompts {
                return capability_disabled(req);
            }
            handle_prompts_get(state, req).await
        })
    }
}
