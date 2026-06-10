//! Axum request handlers for the gateway HTTP server.

pub(crate) use std::convert::Infallible;
pub(crate) use std::pin::Pin;
pub(crate) use std::task::{Context, Poll};

pub(crate) use axum::Json;
pub(crate) use axum::extract::{Path, Query, State};
pub(crate) use axum::http::{HeaderMap, StatusCode};
pub(crate) use axum::response::sse::{Event, KeepAlive, Sse};
pub(crate) use axum::response::{Html, IntoResponse, Response};
pub(crate) use futures::stream;
pub(crate) use serde::Deserialize;
pub(crate) use serde_json::{Value, json};
pub(crate) use tokio_stream::StreamExt;
pub(crate) use tokio_stream::wrappers::BroadcastStream;

pub(crate) use super::super::gateway::is_newer_version;
pub(crate) use super::aggregator;
pub(crate) use super::proxy::proxy_request;
pub(crate) use super::state::{GatewayState, ResolveInstanceError};
pub(crate) use dcc_mcp_jsonrpc::negotiate_protocol_version;
pub(crate) use dcc_mcp_transport::discovery::types::ServiceStatus;

#[cfg(feature = "admin")]
mod debug_openapi;
mod lifecycle_impl;
mod mcp_impl;
mod notification_impl;
mod proxy_impl;
mod registration_impl;
pub(crate) mod resources;
mod rest_impl;
mod rest_support;
mod rest_trace;
mod sse_impl;
mod update_impl;

pub use lifecycle_impl::handle_v1_dcc_instance_stop;
pub use mcp_impl::handle_gateway_mcp;
pub use proxy_impl::{handle_proxy_dcc, handle_proxy_instance};
pub use registration_impl::{
    handle_v1_instances_deregister, handle_v1_instances_heartbeat, handle_v1_instances_register,
};
pub use rest_impl::{
    handle_gateway_yield, handle_health, handle_instances, handle_v1_call, handle_v1_call_batch,
    handle_v1_context, handle_v1_dcc_instance_call, handle_v1_dcc_instance_describe,
    handle_v1_describe, handle_v1_describe_path, handle_v1_docs, handle_v1_healthz,
    handle_v1_list_skills, handle_v1_load_skill, handle_v1_openapi, handle_v1_readyz,
    handle_v1_search, handle_v1_skills, handle_v1_unload_skill,
};
pub use sse_impl::handle_gateway_get;
pub(crate) use update_impl::{handle_v1_update_check, handle_v1_update_download};

pub(crate) use mcp_impl::JsonRpcRequest;
pub(crate) use notification_impl::handle_notification;
