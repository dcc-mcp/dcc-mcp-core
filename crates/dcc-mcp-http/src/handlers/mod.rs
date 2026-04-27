//! Sub-handlers for the MCP HTTP transport.
//!
//! Each module covers a single responsibility area (tools, resources, skills,
//! jobs, etc.).  All items are re-exported into `crate::handler` so the rest
//! of the crate can continue to call them without prefix changes.

// Shared imports re-exported for sub-modules that use `use super::*`.
pub(crate) use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
pub(crate) use serde_json::{Value, json};
pub(crate) use std::sync::{Arc, OnceLock};
pub(crate) use tokio::sync::oneshot;

pub(crate) use crate::{
    error::HttpError,
    inflight::{CancelToken, InFlightEntry, ProgressReporter},
    prompts::PromptError,
    protocol::{
        self, CallToolParams, CallToolResult, DELTA_TOOLS_METHOD, ElicitationCreateParams,
        ElicitationCreateResult, GetPromptParams, JsonRpcBatch, JsonRpcMessage, JsonRpcRequest,
        JsonRpcResponse, ListPromptsResult, ListResourcesResult, LoggingSetLevelParams, McpTool,
        McpToolAnnotations, RESOURCE_NOT_ENABLED_ERROR, ReadResourceParams,
        SubscribeResourceParams, format_sse_event,
    },
    resources::ResourceError,
    session::{SessionLogLevel, SessionLogMessage, SessionManager},
};
pub(crate) use dcc_mcp_models::SkillScope;
pub(crate) use dcc_mcp_protocols::DccMcpError;
pub(crate) use dcc_mcp_skills::catalog::SkillSummary;

pub(crate) use crate::gateway::namespace::{
    decode_skill_tool_name, extract_bare_tool_name, skill_tool_name,
};
pub(crate) use crate::handler::{
    AppState, CANCELLED_REQUEST_TTL, ELICITATION_TIMEOUT, ROOTS_REFRESH_TIMEOUT,
};

pub mod job_tools;
pub mod lazy_actions;
pub mod resources_prompts;
pub mod skill_tools;
#[cfg(test)]
pub mod tests;
pub mod tool_builder_core;
pub mod tool_builder_skill;
pub mod tools_call;

// Explicit re-exports — keeping the surface auditable so a single grep over
// this file enumerates everything `crate::handler::*` and `crate::handlers::*`
// can resolve to.  Adding a new public function in a submodule must be
// reflected here (compiler will complain otherwise once a caller appears).
pub(crate) use job_tools::{
    handle_activate_tool_group, handle_deactivate_tool_group, handle_jobs_cleanup,
    handle_jobs_get_status, handle_search_tools,
};
pub(crate) use lazy_actions::{
    handle_call_action, handle_describe_action, handle_list_actions, json_error_response,
    json_has_id, notify_message, notify_tools_changed, parse_body, parse_raw_values,
    refresh_roots_cache_for_session, request_id_to_string,
};
pub(crate) use resources_prompts::{
    handle_elicitation_create, handle_logging_set_level, handle_prompts_get, handle_prompts_list,
    handle_resources_list, handle_resources_read, handle_resources_subscribe,
    handle_resources_unsubscribe, notify_prompts_list_changed_all,
};
pub(crate) use skill_tools::{
    handle_get_skill_info, handle_list_skills, handle_load_skill, handle_unload_skill,
};
pub(crate) use tool_builder_core::build_core_tools;
pub(crate) use tool_builder_skill::{
    action_meta_to_mcp_tool, build_group_stub, build_lazy_action_tools, build_skill_stub,
    handle_search_skills, missing_capabilities,
};
pub(crate) use tools_call::{handle_tools_call, handle_tools_call_inner};
