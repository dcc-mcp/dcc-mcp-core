//! Quota middleware — per-session call-rate limiting.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::context::CallContext;
use super::error::MiddlewareError;
use super::governance::MiddlewareGovernanceControl;
use super::traits::{BeforeCallMiddleware, MiddlewareFuture};
use serde_json::json;

struct BucketState {
    count: u64,
    window_start: Instant,
}

/// Simple sliding-window quota middleware.
///
/// Counts calls per `window` (default: 1 minute) per bucket key. The bucket
/// key defaults to `session_id` when present, otherwise `"global"`. When the
/// count exceeds `limit` within the window, the call is rejected with
/// [`MiddlewareError::QuotaExceeded`].
pub struct QuotaMiddleware {
    limit: u64,
    window: Duration,
    buckets: Mutex<HashMap<String, BucketState>>,
    allowed_total: AtomicU64,
    throttled_total: AtomicU64,
}

impl QuotaMiddleware {
    /// Create a new quota middleware with the given per-window limit.
    ///
    /// The window defaults to 60 seconds. Use [`QuotaMiddleware::with_window`]
    /// to customise the window duration.
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            window: Duration::from_secs(60),
            buckets: Mutex::new(HashMap::new()),
            allowed_total: AtomicU64::new(0),
            throttled_total: AtomicU64::new(0),
        }
    }

    /// Override the sliding window duration.
    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }
}

impl BeforeCallMiddleware for QuotaMiddleware {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
        // Determine the bucket key: prefer session_id, fall back to "global".
        let key = ctx
            .session_id
            .clone()
            .unwrap_or_else(|| "global".to_string());

        let limit = self.limit;
        let window = self.window;

        let result = {
            let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let bucket = buckets.entry(key.clone()).or_insert(BucketState {
                count: 0,
                window_start: now,
            });

            // Reset window if expired.
            if now.duration_since(bucket.window_start) >= window {
                bucket.count = 0;
                bucket.window_start = now;
            }

            bucket.count += 1;

            if bucket.count > limit {
                Err(MiddlewareError::QuotaExceeded(format!(
                    "session '{key}' exceeded {limit} calls per {}s window",
                    window.as_secs()
                )))
            } else {
                Ok(())
            }
        };
        match &result {
            Ok(()) => {
                self.allowed_total.fetch_add(1, Ordering::Relaxed);
            }
            Err(MiddlewareError::QuotaExceeded(_)) => {
                self.throttled_total.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {}
        }

        Box::pin(async move { result })
    }

    fn governance(&self) -> Option<MiddlewareGovernanceControl> {
        let bucket_count = self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len();
        Some(
            MiddlewareGovernanceControl::new(
                "quota",
                "reject",
                format!(
                    "Limits each session to {} calls per {}s window.",
                    self.limit,
                    self.window.as_secs()
                ),
            )
            .with_config(json!({
                "limit": self.limit,
                "window_secs": self.window.as_secs(),
                "bucket_key": "session_id_or_global",
                "active_buckets": bucket_count,
                "allowed_total": self.allowed_total.load(Ordering::Relaxed),
                "throttled_total": self.throttled_total.load(Ordering::Relaxed),
            })),
        )
    }
}
