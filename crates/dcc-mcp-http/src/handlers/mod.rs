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

pub(crate) use job_tools::*;
pub(crate) use lazy_actions::*;
pub(crate) use resources_prompts::*;
pub(crate) use skill_tools::*;
pub(crate) use tool_builder_core::*;
pub(crate) use tool_builder_skill::*;
pub(crate) use tools_call::*;
