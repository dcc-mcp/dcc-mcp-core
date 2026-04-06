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

#[cfg(test)]
mod tests;

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
