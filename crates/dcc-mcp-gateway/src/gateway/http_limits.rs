//! Axum middleware: per-client request rate limiting (optional) using
//! [`axum::extract::ConnectInfo`]. Requires
//! [`Router::into_make_service_with_connect_info`](axum::Router::into_make_service_with_connect_info)
//! at the TCP acceptor.
//!
//! When [`super::resilience::GatewayLimits::xff_trusted_depth`] is greater than
//! zero, the rate-limit key prefers `X-Forwarded-For`: the **rightmost** `depth`
//! comma-separated fields are treated as trusted reverse-proxy hops; the next
//! field to the left is the client IP. If the header is missing, malformed, or
//! too short, the TCP peer address is used.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use parking_lot::Mutex;

use super::caller_attribution::effective_client_ip;
use super::resilience::GatewayLimits;

struct MinuteWindow {
    minute_epoch: u64,
    counts: HashMap<IpAddr, u32>,
}

impl MinuteWindow {
    fn new() -> Self {
        Self {
            minute_epoch: 0,
            counts: HashMap::new(),
        }
    }
}

/// Per-gateway ingress policy and rate-limit window.
pub struct GatewayIngressState {
    limits: GatewayLimits,
    rate_window: Mutex<MinuteWindow>,
}

impl GatewayIngressState {
    #[must_use]
    /// Construct an isolated ingress state from explicit limits.
    pub fn new(limits: GatewayLimits) -> Self {
        Self {
            limits,
            rate_window: Mutex::new(MinuteWindow::new()),
        }
    }

    #[must_use]
    /// Capture the current process environment for one gateway instance.
    pub fn from_env() -> Self {
        Self::new(GatewayLimits::from_env())
    }

    #[must_use]
    /// Return this gateway's captured ingress limits.
    pub fn limits(&self) -> &GatewayLimits {
        &self.limits
    }

    fn allow_request(&self, client_ip: IpAddr) -> bool {
        let limit = self.limits.rate_limit_per_minute_per_ip;
        if limit == 0 {
            return true;
        }
        let now_m = current_minute_epoch();
        let mut window = self.rate_window.lock();
        if window.minute_epoch != now_m {
            window.minute_epoch = now_m;
            window.counts.clear();
        }
        let count = window.counts.entry(client_ip).or_insert(0);
        if *count >= limit {
            return false;
        }
        *count += 1;
        true
    }
}

impl Default for GatewayIngressState {
    fn default() -> Self {
        Self::from_env()
    }
}

fn current_minute_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

pub async fn rate_limit_middleware(
    axum::extract::State(state): axum::extract::State<Arc<GatewayIngressState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }
    let client_ip = effective_client_ip(
        &addr,
        req.headers(),
        state.limits().xff_trusted_depth as usize,
    );
    if !state.allow_request(client_ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded (per client per minute)",
        )
            .into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(rate_limit: u32) -> GatewayLimits {
        GatewayLimits {
            body_max_bytes: 1024,
            rate_limit_per_minute_per_ip: rate_limit,
            xff_trusted_depth: 0,
        }
    }

    #[test]
    fn rate_windows_are_isolated_per_gateway_state() {
        let first = GatewayIngressState::new(limits(1));
        let second = GatewayIngressState::new(limits(1));
        let client = IpAddr::from([127, 0, 0, 1]);

        assert!(first.allow_request(client));
        assert!(!first.allow_request(client));
        assert!(second.allow_request(client));
        assert!(!second.allow_request(client));
    }

    #[test]
    fn explicit_limits_do_not_depend_on_process_environment() {
        let strict = GatewayIngressState::new(limits(1));
        let disabled = GatewayIngressState::new(limits(0));
        let client = IpAddr::from([127, 0, 0, 2]);

        assert!(strict.allow_request(client));
        assert!(!strict.allow_request(client));
        for _ in 0..4 {
            assert!(disabled.allow_request(client));
        }
    }
}
