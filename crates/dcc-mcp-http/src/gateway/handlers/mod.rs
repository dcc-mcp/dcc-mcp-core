//! Axum request handlers for the gateway HTTP server.

pub(crate) use std::convert::Infallible;
pub(crate) use std::pin::Pin;
pub(crate) use std::task::{Context, Poll};

pub(crate) use axum::Json;
pub(crate) use axum::extract::{Path, State};
pub(crate) use axum::http::{HeaderMap, StatusCode};
pub(crate) use axum::response::sse::{Event, KeepAlive, Sse};
pub(crate) use axum::response::{IntoResponse, Response};
pub(crate) use futures::stream;
pub(crate) use serde::Deserialize;
pub(crate) use serde_json::{Value, json};
pub(crate) use tokio_stream::StreamExt;
pub(crate) use tokio_stream::wrappers::BroadcastStream;

pub(crate) use super::super::gateway::is_newer_version;
pub(crate) use super::aggregator;
pub(crate) use super::proxy::proxy_request;
pub(crate) use super::state::{GatewayState, entry_to_json};
pub(crate) use crate::protocol::negotiate_protocol_version;
pub(crate) use dcc_mcp_transport::discovery::types::ServiceStatus;

mod mcp_impl;
mod notification_impl;
mod proxy_impl;
mod rest_impl;
mod sse_impl;

pub use mcp_impl::handle_gateway_mcp;
pub use proxy_impl::{handle_proxy_dcc, handle_proxy_instance};
pub use rest_impl::{handle_gateway_yield, handle_health, handle_instances};
pub use sse_impl::handle_gateway_get;

pub(crate) use mcp_impl::JsonRpcRequest;
pub(crate) use notification_impl::handle_notification;
