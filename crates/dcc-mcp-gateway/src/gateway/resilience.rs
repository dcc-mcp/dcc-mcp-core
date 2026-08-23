//! Gateway hardening: per-backend circuit breaker, env-backed limits, and
//! helpers for classifying retryable / circuit-worthy backend failures.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime};

use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::backend_client::error::BackendCallError;

// ── environment parsing captured by each gateway instance ───────────────

fn parse_env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

fn parse_env_u32_allow_zero(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

fn parse_env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

// ── HTTP ingress limits (also surfaced on `/admin/api/health`) ─────────

/// Tunable gateway HTTP limits captured when a gateway state is constructed.
#[derive(Debug, Clone)]
pub struct GatewayLimits {
    /// Hard cap on non-streaming request bodies (Axum `RequestBodyLimitLayer`).
    pub body_max_bytes: usize,
    /// Per-IP requests per rolling minute; `0` disables rate limiting.
    pub rate_limit_per_minute_per_ip: u32,
    /// Number of **rightmost** `X-Forwarded-For` hops treated as trusted
    /// infrastructure; client key for rate limiting is the next field to the
    /// left. `0` = use the TCP peer address only (ignore the header).
    pub xff_trusted_depth: u32,
}

fn parse_env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

impl GatewayLimits {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            body_max_bytes: parse_env_usize(
                "DCC_MCP_GATEWAY_HTTP_BODY_LIMIT_BYTES",
                16 * 1024 * 1024,
            ),
            rate_limit_per_minute_per_ip: std::env::var("DCC_MCP_GATEWAY_RATE_LIMIT_PER_MINUTE")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0),
            xff_trusted_depth: std::env::var("DCC_MCP_GATEWAY_XFF_TRUSTED_DEPTH")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0),
        }
    }
}

// ── per-backend circuit registry ───────────────────────────────────────

struct CircuitEntry {
    consecutive_failures: AtomicU32,
    open_until: Mutex<Option<Instant>>,
}

impl Default for CircuitEntry {
    fn default() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            open_until: Mutex::new(None),
        }
    }
}

pub struct CircuitRegistry {
    inner: DashMap<String, std::sync::Arc<CircuitEntry>>,
    failure_threshold: u32,
    open_duration: Duration,
}

impl CircuitRegistry {
    fn new(failure_threshold: u32, open_duration: Duration) -> Self {
        Self {
            inner: DashMap::new(),
            failure_threshold: failure_threshold.max(1),
            open_duration,
        }
    }

    fn key(backend_base: &str) -> String {
        backend_base.trim_end_matches('/').to_string()
    }

    /// Returns `Err` when the circuit for this backend is open.
    pub fn check_open(&self, backend_base: &str) -> Result<(), String> {
        let k = Self::key(backend_base);
        let entry = self
            .inner
            .entry(k)
            .or_insert_with(|| std::sync::Arc::new(CircuitEntry::default()))
            .clone();
        let mut guard = entry.open_until.lock();
        if let Some(until) = *guard {
            if Instant::now() < until {
                let wait = until.saturating_duration_since(Instant::now());
                return Err(format!(
                    "circuit breaker open for {}s (backend {})",
                    wait.as_secs().max(1),
                    backend_base
                ));
            }
            *guard = None;
        }
        Ok(())
    }

    pub fn on_success(&self, backend_base: &str) {
        let k = Self::key(backend_base);
        if let Some(entry) = self.inner.get(&k) {
            entry.consecutive_failures.store(0, Ordering::Relaxed);
            *entry.open_until.lock() = None;
        }
    }

    pub fn on_transport_failure(&self, backend_base: &str) {
        let k = Self::key(backend_base);
        let entry = self
            .inner
            .entry(k)
            .or_insert_with(|| std::sync::Arc::new(CircuitEntry::default()))
            .clone();
        let n = entry.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= self.failure_threshold {
            *entry.open_until.lock() = Some(Instant::now() + self.open_duration);
            entry.consecutive_failures.store(0, Ordering::Relaxed);
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> Value {
        let mut tracked = 0usize;
        let mut open = 0usize;
        for item in self.inner.iter() {
            tracked += 1;
            let entry = item.value();
            if let Some(until) = *entry.open_until.lock()
                && Instant::now() < until
            {
                open += 1;
            }
        }
        json!({
            "tracked_backends": tracked,
            "circuits_open": open,
        })
    }
}

impl Default for CircuitRegistry {
    fn default() -> Self {
        let policy = GatewayResiliencePolicy::from_env();
        Self::new(
            policy.circuit_failure_threshold,
            Duration::from_secs(policy.circuit_open_secs),
        )
    }
}

/// Immutable outbound retry and circuit-breaker policy captured at gateway
/// construction time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayResiliencePolicy {
    pub read_retry_max: u32,
    pub circuit_failure_threshold: u32,
    pub circuit_open_secs: u64,
}

impl GatewayResiliencePolicy {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            read_retry_max: parse_env_u32_allow_zero("DCC_MCP_GATEWAY_READ_RETRY_MAX", 2),
            circuit_failure_threshold: parse_env_u32(
                "DCC_MCP_GATEWAY_CIRCUIT_FAILURE_THRESHOLD",
                5,
            ),
            circuit_open_secs: parse_env_u64("DCC_MCP_GATEWAY_CIRCUIT_OPEN_SECS", 30).max(1),
        }
    }
}

impl Default for GatewayResiliencePolicy {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Gateway-owned retry policy and circuit-breaker table.
///
/// A gateway instance captures its environment-backed policy once during
/// construction. Circuit observations are therefore isolated between gateway
/// instances and tests do not need to serialize access to process-global state.
pub struct GatewayResilienceState {
    policy: GatewayResiliencePolicy,
    circuits: CircuitRegistry,
}

impl GatewayResilienceState {
    #[must_use]
    pub fn new(read_retry_max: u32, failure_threshold: u32, open_duration: Duration) -> Self {
        let policy = GatewayResiliencePolicy {
            read_retry_max,
            circuit_failure_threshold: failure_threshold.max(1),
            circuit_open_secs: open_duration.as_secs().max(1),
        };
        Self {
            policy,
            circuits: CircuitRegistry::new(
                policy.circuit_failure_threshold,
                Duration::from_secs(policy.circuit_open_secs),
            ),
        }
    }

    #[must_use]
    pub fn from_policy(policy: GatewayResiliencePolicy) -> Self {
        Self::new(
            policy.read_retry_max,
            policy.circuit_failure_threshold,
            Duration::from_secs(policy.circuit_open_secs),
        )
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self::from_policy(GatewayResiliencePolicy::from_env())
    }

    /// Max extra read attempts after the first try (`0` = no retries).
    #[must_use]
    pub fn read_retry_max(&self) -> u32 {
        self.policy.read_retry_max
    }

    #[must_use]
    pub fn policy(&self) -> GatewayResiliencePolicy {
        self.policy
    }

    #[must_use]
    pub fn circuits(&self) -> &CircuitRegistry {
        &self.circuits
    }
}

impl Default for GatewayResilienceState {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Classify `rest_get` / `rest_post` string errors for **read** retries.
#[must_use]
pub fn is_retryable_rest_error(err: &str) -> bool {
    if err.contains("transport error") || err.contains("transport desync") {
        return true;
    }
    // `rest_get` errors look like `"{url}: HTTP {status}: ..."`
    if let Some(idx) = err.find(": HTTP ") {
        let tail = &err[idx + 7..];
        if let Some(st) = tail.split_whitespace().next()
            && let Ok(code) = st.parse::<u16>()
        {
            return code == 429 || (500..600).contains(&code);
        }
    }
    err.contains("read body")
}

/// Whether this failure should advance the per-backend circuit counter.
#[must_use]
pub fn is_circuit_worthy_rest_error(err: &str) -> bool {
    is_retryable_rest_error(err)
}

#[must_use]
pub(crate) fn is_circuit_worthy_jsonrpc_error(err: &BackendCallError) -> bool {
    match err {
        BackendCallError::Transport { .. }
        | BackendCallError::ReadBody { .. }
        | BackendCallError::Unreachable { .. }
        | BackendCallError::Booting { .. }
        | BackendCallError::ResponseIdMismatch { .. } => true,
        BackendCallError::Http { status, .. } => status
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .is_some_and(|c| (500..600).contains(&c)),
        BackendCallError::InvalidJson { .. }
        | BackendCallError::Backend { .. }
        | BackendCallError::EmptyResult { .. } => false,
    }
}

pub async fn jittered_backoff(attempt: u32) {
    let base = 25u64.saturating_mul(u64::from(attempt).saturating_add(1));
    let jitter = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.subsec_micros() % 50) as u64)
        .unwrap_or(0);
    tokio::time::sleep(Duration::from_millis(base + jitter)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_instances_keep_circuit_observations_isolated() {
        let first = GatewayResilienceState::new(0, 1, Duration::from_secs(30));
        let second = GatewayResilienceState::new(0, 1, Duration::from_secs(30));
        let backend = "http://maya.local:8000";

        first.circuits().on_transport_failure(backend);

        assert!(first.circuits().check_open(backend).is_err());
        assert!(second.circuits().check_open(backend).is_ok());
        assert_eq!(first.circuits().snapshot_json()["circuits_open"], json!(1));
        assert_eq!(second.circuits().snapshot_json()["circuits_open"], json!(0));
    }

    #[test]
    fn gateway_instances_apply_their_own_circuit_policy() {
        let strict = GatewayResilienceState::new(0, 1, Duration::from_secs(30));
        let tolerant = GatewayResilienceState::new(0, 2, Duration::from_secs(30));
        let backend = "http://photoshop.local:9000";

        strict.circuits().on_transport_failure(backend);
        tolerant.circuits().on_transport_failure(backend);

        assert!(strict.circuits().check_open(backend).is_err());
        assert!(tolerant.circuits().check_open(backend).is_ok());
    }

    #[test]
    fn retry_policy_is_captured_per_gateway_instance() {
        let no_retry = GatewayResilienceState::new(0, 5, Duration::from_secs(30));
        let retry_twice = GatewayResilienceState::new(2, 5, Duration::from_secs(30));

        assert_eq!(no_retry.read_retry_max(), 0);
        assert_eq!(retry_twice.read_retry_max(), 2);
    }
}
