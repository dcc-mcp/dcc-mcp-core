//! Tests for the action middleware pipeline.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use serde_json::json;

use crate::dispatcher::{ActionDispatcher, DispatchError, DispatchResult};
use crate::registry::{ActionMeta, ActionRegistry};

use super::{
    ActionMiddleware, ActionPipeline, AuditMiddleware, LoggingMiddleware, MiddlewareContext,
    RateLimitMiddleware, TimingMiddleware,
};

mod audit;
mod context;
mod custom;
pub(super) mod fixtures;
mod logging;
mod pipeline_basics;
mod rate_limit;
mod timing;
