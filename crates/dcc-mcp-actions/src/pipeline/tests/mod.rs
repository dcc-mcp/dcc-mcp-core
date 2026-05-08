//! Tests for the action middleware pipeline.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use serde_json::json;

use crate::dispatcher::{DispatchError, DispatchResult, ToolDispatcher};
use crate::registry::{ToolMeta, ToolRegistry};

use super::{
    ActionMiddleware, AuditMiddleware, LoggingMiddleware, MiddlewareContext, RateLimitMiddleware,
    TimingMiddleware, ToolPipeline,
};

mod audit;
mod context;
mod custom;
pub(super) mod fixtures;
mod logging;
mod pipeline_basics;
mod rate_limit;
mod timing;
