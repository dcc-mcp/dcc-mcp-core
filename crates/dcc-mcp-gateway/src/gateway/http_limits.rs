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
use std::sync::LazyLock;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use parking_lot::Mutex;

use super::caller_attribution::effective_client_ip;
use super::resilience::gateway_limits;

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

static RATE_STATE: LazyLock<Mutex<MinuteWindow>> =
    LazyLock::new(|| Mutex::new(MinuteWindow::new()));

fn current_minute_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 60)
        .unwrap_or(0)
}

fn allow_request(client_ip: IpAddr) -> bool {
    let lim = gateway_limits().rate_limit_per_minute_per_ip;
    if lim == 0 {
        return true;
    }
    let now_m = current_minute_epoch();
    let mut g = RATE_STATE.lock();
    if g.minute_epoch != now_m {
        g.minute_epoch = now_m;
        g.counts.clear();
    }
    let e = g.counts.entry(client_ip).or_insert(0);
    if *e >= lim {
        return false;
    }
    *e += 1;
    true
}

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }
    let client_ip = effective_client_ip(&addr, req.headers());
    if !allow_request(client_ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded (per client per minute)",
        )
            .into_response();
    }
    next.run(req).await
}
