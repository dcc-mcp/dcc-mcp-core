//! Unit and integration tests for the MCP HTTP server.

use axum::http::HeaderValue;
use axum_test::TestServer;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{
    config::McpHttpConfig,
    handler::AppState,
    server::McpHttpServer,
    session::{SessionLogLevel, SessionManager},
};
use dcc_mcp_actions::{ActionDispatcher, ActionMeta, ActionRegistry};
use dcc_mcp_models::{SkillMetadata, ToolDeclaration};
use dcc_mcp_skills::SkillCatalog;

fn make_registry() -> ActionRegistry {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "get_scene_info".into(),
        description: "Get current scene info".into(),
        category: "scene".into(),
        tags: vec!["query".into()],
        dcc: "test_dcc".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    reg.register_action(ActionMeta {
        name: "list_objects".into(),
        description: "List all objects".into(),
        category: "scene".into(),
        tags: vec!["query".into(), "list".into()],
        dcc: "test_dcc".into(),
        version: "1.0.0".into(),
        ..Default::default()
    });
    reg
}

fn make_app_state() -> AppState {
    let registry = Arc::new(make_registry());
    let catalog = Arc::new(SkillCatalog::new(registry.clone()));
    let dispatcher = Arc::new(ActionDispatcher::new((*registry).clone()));
    AppState {
        registry,
        dispatcher,
        catalog,
        sessions: SessionManager::new(),
        executor: None,
        bridge_registry: crate::BridgeRegistry::new(),
        server_name: "test-dcc".to_string(),
        server_version: "0.1.0".to_string(),
        cancelled_requests: std::sync::Arc::new(dashmap::DashMap::new()),
        in_flight: crate::inflight::InFlightRequests::new(),
        pending_elicitations: std::sync::Arc::new(dashmap::DashMap::new()),
        lazy_actions: false,

        bare_tool_names: true,
        declared_capabilities: std::sync::Arc::new(Vec::new()),
        jobs: std::sync::Arc::new(crate::job::JobManager::new()),
        job_notifier: crate::notifications::JobNotifier::new(SessionManager::new(), true),
        resources: crate::resources::ResourceRegistry::new(true, false),
        enable_resources: true,
        prompts: crate::prompts::PromptRegistry::new(true),
        enable_prompts: true,
    }
}

fn make_router() -> axum::Router {
    use crate::handler::{handle_delete, handle_get, handle_post};
    use axum::{Router, routing};

    let state = make_app_state();
    Router::new()
        .route(
            "/mcp",
            routing::post(handle_post)
                .get(handle_get)
                .delete(handle_delete),
        )
        .with_state(state)
}

fn parse_sse_payload(raw_event: &str) -> Value {
    let payload = raw_event
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or("{}");
    serde_json::from_str(payload).unwrap_or_else(|_| json!({}))
}

fn drain_sse_events(
    rx: &mut tokio::sync::broadcast::Receiver<String>,
    max_events: usize,
) -> Vec<Value> {
    let mut out = Vec::new();
    for _ in 0..max_events {
        match rx.try_recv() {
            Ok(raw) => out.push(parse_sse_payload(&raw)),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
        }
    }
    out
}

// Submodules extracted from monolithic tests.rs
mod gateway;
mod lazy_actions;
mod next_tools_meta;
mod resource_link;

mod initialize;
mod elicitation;
mod tools_list;
mod tools_call;
mod jobs;
mod search_skills;
mod on_demand_loading;
mod session;
mod pagination;
mod delta_capability;
mod logging;
mod skill_discovery;
mod dispatch_with_handler;

pub use initialize::*;
pub use elicitation::*;
pub use tools_list::*;
pub use tools_call::*;
pub use jobs::*;
pub use search_skills::*;
pub use on_demand_loading::*;
pub use session::*;
pub use pagination::*;
pub use delta_capability::*;
pub use logging::*;
pub use skill_discovery::*;
pub use dispatch_with_handler::*;
