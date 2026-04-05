//! Circuit breaker — prevents cascading failures in DCC transport connections.
//!
//! Implements the standard three-state circuit breaker pattern:
//!
//! ```text
//! Closed ──(threshold exceeded)──► Open ──(timeout elapsed)──► HalfOpen
//!   ▲                                                              │
//!   └────────────────(success)────────────────────────────────────┘
//!   (HalfOpen failure goes back to Open)
//! ```
//!
//! | State    | Behaviour                                                  |
//! |----------|------------------------------------------------------------|
//! | Closed   | Normal operation. Failures increment a counter.            |
//! | Open     | All calls fail immediately (fast-fail). No DCC connection. |
//! | HalfOpen | One probe call is allowed through; success → Closed.       |
//!
//! # Why This Matters
//!
//! When a DCC application crashes or freezes, without a circuit breaker:
//! - Every action call blocks for the full TCP timeout (default 10 s)
//! - The MCP server becomes unresponsive
//! - A frozen Blender can cascade to affect all other DCCs
//!
//! With a circuit breaker, after `failure_threshold` consecutive failures
//! the circuit opens and calls return immediately with [`TransportError::CircuitOpen`].
//! After `recovery_timeout`, one probe request is allowed through to test recovery.
//!
//! # Example
//!
//! ```rust
//! use std::time::Duration;
//! use dcc_mcp_transport::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
//!
//! let config = CircuitBreakerConfig {
//!     failure_threshold: 3,
//!     recovery_timeout: Duration::from_secs(30),
//!     probe_success_threshold: 1,
//!     ..Default::default()
//! };
//! let mut cb = CircuitBreaker::new("maya-18812", config);
//!
//! // Simulate failures
//! cb.record_failure();
//! cb.record_failure();
//! cb.record_failure(); // threshold reached → circuit opens
//!
//! // Fast-fail while open
//! assert!(!cb.allow_request());
//! ```

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{TransportError, TransportResult};

// ── CircuitState ──────────────────────────────────────────────────────────────

/// The three states of the circuit breaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests are forwarded to the real connection.
    Closed,
    /// Tripped — requests fail immediately without attempting connection.
    Open,
    /// Recovery probe — one request is allowed through to test the connection.
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half_open"),
        }
    }
}

// ── CircuitBreakerConfig ──────────────────────────────────────────────────────

/// Configuration for a [`CircuitBreaker`].
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before the circuit opens.
    ///
    /// Default: 5
    pub failure_threshold: u32,

    /// How long to wait in the Open state before transitioning to HalfOpen.
    ///
    /// Default: 30 seconds
    pub recovery_timeout: Duration,

    /// Number of consecutive successes in HalfOpen to close the circuit.
    ///
    /// Default: 1 (one successful probe closes the circuit)
    pub probe_success_threshold: u32,

    /// Optional sliding window for failure counting.
    ///
    /// When set, only failures within this window count toward the threshold.
    /// `None` means all consecutive failures count (no time window).
    pub failure_window: Option<Duration>,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
            probe_success_threshold: 1,
            failure_window: None,
        }
    }
}

// ── CircuitBreakerStats ───────────────────────────────────────────────────────

/// Operational statistics for a [`CircuitBreaker`].
#[derive(Debug, Clone, Default)]
pub struct CircuitBreakerStats {
    /// Total number of requests allowed through.
    pub total_requests: u64,
    /// Total number of successful requests.
    pub total_successes: u64,
    /// Total number of failed requests.
    pub total_failures: u64,
    /// Total number of requests rejected (circuit open fast-fail).
    pub total_rejected: u64,
    /// Number of times the circuit has transitioned to Open.
    pub trips: u64,
}

impl CircuitBreakerStats {
    /// Success rate (0.0..=1.0). Returns 1.0 if no requests yet.
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0
        } else {
            self.total_successes as f64 / self.total_requests as f64
        }
    }

    /// Failure rate (0.0..=1.0). Returns 0.0 if no requests yet.
    #[must_use]
    pub fn failure_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.total_failures as f64 / self.total_requests as f64
        }
    }
}

// ── CircuitBreakerInner ───────────────────────────────────────────────────────

struct CircuitBreakerInner {
    state: CircuitState,
    consecutive_failures: u32,
    consecutive_successes_in_half_open: u32,
    last_failure_at: Option<Instant>,
    opened_at: Option<Instant>,
    stats: CircuitBreakerStats,
}

impl CircuitBreakerInner {
    fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes_in_half_open: 0,
            last_failure_at: None,
            opened_at: None,
            stats: CircuitBreakerStats::default(),
        }
    }
}

// ── CircuitBreaker ────────────────────────────────────────────────────────────

/// Thread-safe circuit breaker for a single DCC connection endpoint.
///
/// Wraps connection state with failure counting and automatic open/half-open
/// transition logic. Use one `CircuitBreaker` per `(dcc_type, instance_id)` pair.
#[derive(Clone)]
pub struct CircuitBreaker {
    /// Identifier for logging (e.g. "maya-127.0.0.1:18812").
    name: String,
    config: Arc<CircuitBreakerConfig>,
    inner: Arc<Mutex<CircuitBreakerInner>>,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("name", &self.name)
            .field("state", &self.state())
            .finish()
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given name and configuration.
    #[must_use]
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            config: Arc::new(config),
            inner: Arc::new(Mutex::new(CircuitBreakerInner::new())),
        }
    }

    /// Create a circuit breaker with default configuration.
    #[must_use]
    pub fn with_defaults(name: impl Into<String>) -> Self {
        Self::new(name, CircuitBreakerConfig::default())
    }

    /// Get the current circuit state.
    #[must_use]
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        self.maybe_transition_to_half_open(&mut inner);
        inner.state
    }

    /// Check whether a request should be allowed through.
    ///
    /// Returns `true` if the circuit is Closed or HalfOpen (probe slot available).
    /// Returns `false` if the circuit is Open (fast-fail).
    ///
    /// This method also handles the automatic transition from Open → HalfOpen
    /// when `recovery_timeout` has elapsed.
    #[must_use]
    pub fn allow_request(&self) -> bool {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        self.maybe_transition_to_half_open(&mut inner);

        match inner.state {
            CircuitState::Closed => {
                inner.stats.total_requests += 1;
                true
            }
            CircuitState::Open => {
                inner.stats.total_rejected += 1;
                false
            }
            CircuitState::HalfOpen => {
                // Allow only one probe at a time
                inner.stats.total_requests += 1;
                true
            }
        }
    }

    /// Record a successful request outcome.
    ///
    /// In HalfOpen state, enough successes will close the circuit.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        inner.stats.total_successes += 1;
        inner.consecutive_failures = 0;

        match inner.state {
            CircuitState::Closed => {
                // Already healthy, nothing to change
            }
            CircuitState::HalfOpen => {
                inner.consecutive_successes_in_half_open += 1;
                if inner.consecutive_successes_in_half_open >= self.config.probe_success_threshold {
                    tracing::info!(
                        name = %self.name,
                        "circuit breaker closing after successful probe"
                    );
                    inner.state = CircuitState::Closed;
                    inner.consecutive_successes_in_half_open = 0;
                    inner.opened_at = None;
                }
            }
            CircuitState::Open => {
                // Should not happen (success without allow_request), but handle gracefully
            }
        }
    }

    /// Record a failed request outcome.
    ///
    /// If the failure threshold is reached in Closed state, the circuit opens.
    /// In HalfOpen state, any failure immediately re-opens the circuit.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        inner.stats.total_failures += 1;
        inner.stats.total_requests += 1;
        inner.last_failure_at = Some(Instant::now());

        match inner.state {
            CircuitState::Closed => {
                // Check if failure is within the sliding window (if configured)
                let counts = if let Some(window) = self.config.failure_window {
                    if inner.last_failure_at.is_some_and(|t| t.elapsed() <= window) {
                        inner.consecutive_failures + 1
                    } else {
                        1 // Reset: failure outside window
                    }
                } else {
                    inner.consecutive_failures + 1
                };

                inner.consecutive_failures = counts;

                if inner.consecutive_failures >= self.config.failure_threshold {
                    tracing::warn!(
                        name = %self.name,
                        failures = inner.consecutive_failures,
                        threshold = self.config.failure_threshold,
                        "circuit breaker opening after consecutive failures"
                    );
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(Instant::now());
                    inner.stats.trips += 1;
                }
            }
            CircuitState::HalfOpen => {
                // Probe failed — re-open the circuit
                tracing::warn!(
                    name = %self.name,
                    "circuit breaker re-opening after failed probe"
                );
                inner.state = CircuitState::Open;
                inner.opened_at = Some(Instant::now());
                inner.stats.trips += 1;
                inner.consecutive_successes_in_half_open = 0;
                inner.consecutive_failures = 0;
            }
            CircuitState::Open => {
                // Already open, update last failure time
            }
        }
    }

    /// Forcibly reset the circuit to Closed state (e.g. after manual DCC restart).
    pub fn reset(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock poisoned");
        tracing::info!(name = %self.name, "circuit breaker manually reset");
        inner.state = CircuitState::Closed;
        inner.consecutive_failures = 0;
        inner.consecutive_successes_in_half_open = 0;
        inner.last_failure_at = None;
        inner.opened_at = None;
    }

    /// Get a snapshot of current statistics.
    #[must_use]
    pub fn stats(&self) -> CircuitBreakerStats {
        self.inner
            .lock()
            .expect("circuit breaker lock poisoned")
            .stats
            .clone()
    }

    /// Get the name/identifier of this circuit breaker.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the number of consecutive failures in Closed state.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.inner
            .lock()
            .expect("circuit breaker lock poisoned")
            .consecutive_failures
    }

    /// Execute a closure through the circuit breaker.
    ///
    /// Automatically calls `record_success` or `record_failure` based on the result.
    /// Returns [`TransportError::CircuitOpen`] if the circuit is open.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use dcc_mcp_transport::circuit_breaker::CircuitBreaker;
    /// let cb = CircuitBreaker::with_defaults("maya");
    /// let result = cb.call(|| -> Result<i32, String> { Ok(42) });
    /// assert_eq!(result.unwrap(), 42);
    /// ```
    pub fn call<F, T, E>(&self, f: F) -> TransportResult<T>
    where
        F: FnOnce() -> Result<T, E>,
        E: std::fmt::Display,
    {
        if !self.allow_request() {
            return Err(TransportError::CircuitOpen {
                name: self.name.clone(),
            });
        }

        match f() {
            Ok(value) => {
                self.record_success();
                Ok(value)
            }
            Err(e) => {
                self.record_failure();
                Err(TransportError::Internal(e.to_string()))
            }
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Transition Open → HalfOpen if recovery_timeout has elapsed.
    fn maybe_transition_to_half_open(&self, inner: &mut CircuitBreakerInner) {
        if inner.state == CircuitState::Open {
            if let Some(opened_at) = inner.opened_at {
                if opened_at.elapsed() >= self.config.recovery_timeout {
                    tracing::info!(
                        name = %self.name,
                        "circuit breaker transitioning to half-open for probe"
                    );
                    inner.state = CircuitState::HalfOpen;
                    inner.consecutive_successes_in_half_open = 0;
                }
            }
        }
    }
}

// ── CircuitBreakerRegistry ────────────────────────────────────────────────────

/// A registry of circuit breakers, one per DCC endpoint.
///
/// Provides thread-safe lookup by endpoint name, with automatic creation
/// of new circuit breakers for unknown endpoints.
#[derive(Clone, Default)]
pub struct CircuitBreakerRegistry {
    breakers: Arc<dashmap::DashMap<String, CircuitBreaker>>,
    default_config: Arc<CircuitBreakerConfig>,
}

impl CircuitBreakerRegistry {
    /// Create a new registry with the given default configuration.
    #[must_use]
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: Arc::new(dashmap::DashMap::new()),
            default_config: Arc::new(default_config),
        }
    }

    /// Get or create a circuit breaker for the given endpoint.
    pub fn get_or_create(&self, name: &str) -> CircuitBreaker {
        if let Some(cb) = self.breakers.get(name) {
            return cb.value().clone();
        }
        let cb = CircuitBreaker::new(name, (*self.default_config).clone());
        self.breakers.insert(name.to_string(), cb.clone());
        cb
    }

    /// Register a circuit breaker with a custom configuration.
    pub fn register(
        &self,
        name: impl Into<String>,
        config: CircuitBreakerConfig,
    ) -> CircuitBreaker {
        let name = name.into();
        let cb = CircuitBreaker::new(&name, config);
        self.breakers.insert(name, cb.clone());
        cb
    }

    /// Remove a circuit breaker from the registry.
    pub fn remove(&self, name: &str) -> bool {
        self.breakers.remove(name).is_some()
    }

    /// Get the number of registered circuit breakers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.breakers.len()
    }

    /// Check if the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.breakers.is_empty()
    }

    /// Get a snapshot of all circuit breaker states.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(String, CircuitState)> {
        let mut result: Vec<(String, CircuitState)> = self
            .breakers
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().state()))
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }
}

impl std::fmt::Debug for CircuitBreakerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreakerRegistry")
            .field("count", &self.len())
            .finish()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cb(threshold: u32) -> CircuitBreaker {
        CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                failure_threshold: threshold,
                recovery_timeout: Duration::from_secs(60),
                probe_success_threshold: 1,
                failure_window: None,
            },
        )
    }

    fn make_fast_recovery_cb(threshold: u32) -> CircuitBreaker {
        CircuitBreaker::new(
            "test-fast",
            CircuitBreakerConfig {
                failure_threshold: threshold,
                recovery_timeout: Duration::from_millis(1),
                probe_success_threshold: 1,
                failure_window: None,
            },
        )
    }

    // ── CircuitState ─────────────────────────────────────────────────────────

    mod state {
        use super::*;

        #[test]
        fn test_state_display() {
            assert_eq!(CircuitState::Closed.to_string(), "closed");
            assert_eq!(CircuitState::Open.to_string(), "open");
            assert_eq!(CircuitState::HalfOpen.to_string(), "half_open");
        }

        #[test]
        fn test_initial_state_is_closed() {
            let cb = make_cb(3);
            assert_eq!(cb.state(), CircuitState::Closed);
        }
    }

    // ── Closed state ──────────────────────────────────────────────────────────

    mod closed_state {
        use super::*;

        #[test]
        fn test_allows_requests_when_closed() {
            let cb = make_cb(3);
            assert!(cb.allow_request());
        }

        #[test]
        fn test_success_keeps_circuit_closed() {
            let cb = make_cb(3);
            cb.record_success();
            assert_eq!(cb.state(), CircuitState::Closed);
        }

        #[test]
        fn test_failures_below_threshold_stay_closed() {
            let cb = make_cb(3);
            cb.allow_request();
            cb.record_failure();
            cb.allow_request();
            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Closed);
            assert_eq!(cb.consecutive_failures(), 2);
        }

        #[test]
        fn test_success_resets_failure_counter() {
            let cb = make_cb(3);
            cb.allow_request();
            cb.record_failure();
            cb.allow_request();
            cb.record_failure();
            cb.allow_request();
            cb.record_success(); // resets counter
            assert_eq!(cb.consecutive_failures(), 0);
        }
    }

    // ── Open state ───────────────────────────────────────────────────────────

    mod open_state {
        use super::*;

        #[test]
        fn test_circuit_opens_after_threshold() {
            let cb = make_cb(3);
            for _ in 0..3 {
                cb.allow_request();
                cb.record_failure();
            }
            assert_eq!(cb.state(), CircuitState::Open);
        }

        #[test]
        fn test_open_circuit_rejects_requests() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure(); // trips the circuit
            assert!(!cb.allow_request()); // fast-fail
        }

        #[test]
        fn test_open_circuit_increments_rejected_counter() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure();

            cb.allow_request(); // rejected
            cb.allow_request(); // rejected
            assert_eq!(cb.stats().total_rejected, 2);
        }

        #[test]
        fn test_trips_counter_increments() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure(); // trip 1
            cb.reset();
            cb.allow_request();
            cb.record_failure(); // trip 2
            assert_eq!(cb.stats().trips, 2);
        }
    }

    // ── HalfOpen state ───────────────────────────────────────────────────────

    mod half_open_state {
        use super::*;

        #[test]
        fn test_transitions_to_half_open_after_timeout() {
            let cb = make_fast_recovery_cb(1);
            cb.allow_request();
            cb.record_failure(); // opens

            assert_eq!(cb.state(), CircuitState::Open);

            std::thread::sleep(Duration::from_millis(2)); // wait for recovery

            assert_eq!(cb.state(), CircuitState::HalfOpen);
        }

        #[test]
        fn test_half_open_probe_success_closes_circuit() {
            let cb = make_fast_recovery_cb(1);
            cb.allow_request();
            cb.record_failure(); // opens

            std::thread::sleep(Duration::from_millis(2));
            assert_eq!(cb.state(), CircuitState::HalfOpen);

            cb.allow_request(); // probe
            cb.record_success(); // closes

            assert_eq!(cb.state(), CircuitState::Closed);
        }

        #[test]
        fn test_half_open_probe_failure_reopens_circuit() {
            let cb = make_fast_recovery_cb(1);
            cb.allow_request();
            cb.record_failure(); // opens

            std::thread::sleep(Duration::from_millis(2));
            assert_eq!(cb.state(), CircuitState::HalfOpen);

            cb.allow_request(); // probe
            cb.record_failure(); // re-opens

            assert_eq!(cb.state(), CircuitState::Open);
            assert_eq!(cb.stats().trips, 2); // opened twice
        }

        #[test]
        fn test_multiple_probe_successes_required() {
            let cb = CircuitBreaker::new(
                "test",
                CircuitBreakerConfig {
                    failure_threshold: 1,
                    recovery_timeout: Duration::from_millis(1),
                    probe_success_threshold: 2, // need 2 successes to close
                    failure_window: None,
                },
            );
            cb.allow_request();
            cb.record_failure(); // opens

            std::thread::sleep(Duration::from_millis(2));
            // State: HalfOpen

            cb.allow_request();
            cb.record_success(); // 1/2 successes
            assert_eq!(cb.state(), CircuitState::HalfOpen); // still half-open

            // Second probe
            cb.allow_request();
            cb.record_success(); // 2/2 successes → close
            assert_eq!(cb.state(), CircuitState::Closed);
        }
    }

    // ── Reset ────────────────────────────────────────────────────────────────

    mod reset {
        use super::*;

        #[test]
        fn test_reset_from_open() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure(); // opens

            cb.reset();
            assert_eq!(cb.state(), CircuitState::Closed);
            assert_eq!(cb.consecutive_failures(), 0);
        }

        #[test]
        fn test_reset_allows_requests_again() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure(); // opens
            assert!(!cb.allow_request()); // blocked

            cb.reset();
            assert!(cb.allow_request()); // allowed again
        }
    }

    // ── Statistics ───────────────────────────────────────────────────────────

    mod stats {
        use super::*;

        #[test]
        fn test_stats_success_rate() {
            let cb = make_cb(10);
            // 3 successes, 1 failure (but record_failure also increments total)
            cb.allow_request();
            cb.record_success();
            cb.allow_request();
            cb.record_success();
            cb.allow_request();
            cb.record_success();
            // record_failure increments total_requests too
            cb.record_failure();

            let stats = cb.stats();
            assert_eq!(stats.total_successes, 3);
            // total_failures = 1, total_requests includes both allow_request + record_failure calls
            assert!(stats.failure_rate() > 0.0);
        }

        #[test]
        fn test_stats_empty_success_rate() {
            let cb = make_cb(5);
            assert_eq!(cb.stats().success_rate(), 1.0); // no requests → 100%
            assert_eq!(cb.stats().failure_rate(), 0.0);
        }

        #[test]
        fn test_stats_rejected_counted() {
            let cb = make_cb(1);
            cb.allow_request();
            cb.record_failure(); // trips

            for _ in 0..5 {
                cb.allow_request(); // all rejected
            }
            assert_eq!(cb.stats().total_rejected, 5);
        }
    }

    // ── call() helper ────────────────────────────────────────────────────────

    mod call_helper {
        use super::*;

        #[test]
        fn test_call_success() {
            let cb = make_cb(3);
            let result: TransportResult<i32> = cb.call(|| -> Result<i32, String> { Ok(42) });
            assert_eq!(result.unwrap(), 42);
            assert_eq!(cb.stats().total_successes, 1);
        }

        #[test]
        fn test_call_failure_records_failure() {
            let cb = make_cb(3);
            let _: TransportResult<()> =
                cb.call(|| -> Result<(), String> { Err("oops".to_string()) });
            assert_eq!(cb.consecutive_failures(), 1);
        }

        #[test]
        fn test_call_returns_circuit_open_when_tripped() {
            let cb = make_cb(1);
            // Trip the circuit
            let _: TransportResult<()> =
                cb.call(|| -> Result<(), String> { Err("fail".to_string()) });

            // Next call should get CircuitOpen error
            let result: TransportResult<()> = cb.call(|| -> Result<(), String> { Ok(()) });
            assert!(matches!(result, Err(TransportError::CircuitOpen { .. })));
        }

        #[test]
        fn test_call_trips_after_threshold() {
            let cb = make_cb(2);
            for _ in 0..2 {
                let _: TransportResult<()> =
                    cb.call(|| -> Result<(), String> { Err("e".to_string()) });
            }
            assert_eq!(cb.state(), CircuitState::Open);
        }
    }

    // ── Debug ─────────────────────────────────────────────────────────────────

    mod debug {
        use super::*;

        #[test]
        fn test_circuit_breaker_debug() {
            let cb = make_cb(3);
            let s = format!("{cb:?}");
            assert!(s.contains("CircuitBreaker"));
        }

        #[test]
        fn test_circuit_breaker_name() {
            let cb = CircuitBreaker::with_defaults("maya-18812");
            assert_eq!(cb.name(), "maya-18812");
        }

        #[test]
        fn test_circuit_breaker_clone() {
            let cb1 = make_cb(3);
            let cb2 = cb1.clone();
            cb1.allow_request();
            cb1.record_failure();
            // Clone shares state
            assert_eq!(cb2.consecutive_failures(), 1);
        }
    }

    // ── CircuitBreakerRegistry ───────────────────────────────────────────────

    mod registry {
        use super::*;

        #[test]
        fn test_registry_get_or_create() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            let cb1 = registry.get_or_create("maya");
            let cb2 = registry.get_or_create("maya");

            // Same key should share state
            cb1.allow_request();
            cb1.record_failure();
            assert_eq!(cb2.consecutive_failures(), 1);
        }

        #[test]
        fn test_registry_different_endpoints_isolated() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            let maya_cb = registry.get_or_create("maya");
            let blender_cb = registry.get_or_create("blender");

            // Trip maya
            for _ in 0..5 {
                maya_cb.allow_request();
                maya_cb.record_failure();
            }

            // Blender should be unaffected
            assert_eq!(blender_cb.state(), CircuitState::Closed);
            assert_eq!(maya_cb.state(), CircuitState::Open);
        }

        #[test]
        fn test_registry_len() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            assert_eq!(registry.len(), 0);
            registry.get_or_create("a");
            registry.get_or_create("b");
            registry.get_or_create("c");
            assert_eq!(registry.len(), 3);
        }

        #[test]
        fn test_registry_remove() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            registry.get_or_create("maya");
            assert_eq!(registry.len(), 1);
            assert!(registry.remove("maya"));
            assert_eq!(registry.len(), 0);
            assert!(!registry.remove("maya")); // second remove returns false
        }

        #[test]
        fn test_registry_snapshot_sorted() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            registry.get_or_create("z_maya");
            registry.get_or_create("a_blender");
            registry.get_or_create("m_houdini");

            let snapshot = registry.snapshot();
            assert_eq!(snapshot[0].0, "a_blender");
            assert_eq!(snapshot[1].0, "m_houdini");
            assert_eq!(snapshot[2].0, "z_maya");
            for (_, state) in &snapshot {
                assert_eq!(*state, CircuitState::Closed);
            }
        }

        #[test]
        fn test_registry_is_empty() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            assert!(registry.is_empty());
            registry.get_or_create("x");
            assert!(!registry.is_empty());
        }

        #[test]
        fn test_registry_register_custom_config() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            let cb = registry.register(
                "critical-dcc",
                CircuitBreakerConfig {
                    failure_threshold: 1, // strict: 1 failure opens immediately
                    ..Default::default()
                },
            );
            cb.allow_request();
            cb.record_failure();
            assert_eq!(cb.state(), CircuitState::Open);
        }

        #[test]
        fn test_registry_debug() {
            let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
            let s = format!("{registry:?}");
            assert!(s.contains("CircuitBreakerRegistry"));
        }
    }
}
