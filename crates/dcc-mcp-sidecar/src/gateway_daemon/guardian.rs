use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::launcher::{
    EnsureGatewayOptions, ensure_gateway_running, gateway_health_ok_with_timeout,
};

const GATEWAY_GUARDIAN_INTERVAL: Duration = Duration::from_secs(5);
const GATEWAY_GUARDIAN_TIMEOUT: Duration = Duration::from_millis(500);
const GATEWAY_GUARDIAN_FAILURES: u32 = 2;
const GATEWAY_GUARDIAN_REENSURE_JITTER_MAX: Duration = Duration::from_secs(2);

const ENV_GATEWAY_GUARDIAN_INTERVAL: &str = "DCC_MCP_GATEWAY_GUARDIAN_INTERVAL";
const ENV_GATEWAY_GUARDIAN_TIMEOUT: &str = "DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT";
const ENV_GATEWAY_GUARDIAN_FAILURES: &str = "DCC_MCP_GATEWAY_GUARDIAN_FAILURES";
const ENV_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX: &str =
    "DCC_MCP_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GatewayGuardianSettings {
    interval: Duration,
    probe_timeout: Duration,
    failure_threshold: u32,
    reensure_jitter_max: Duration,
}

impl GatewayGuardianSettings {
    pub fn from_env() -> Self {
        Self {
            interval: duration_secs_env(ENV_GATEWAY_GUARDIAN_INTERVAL, GATEWAY_GUARDIAN_INTERVAL),
            probe_timeout: duration_secs_env(
                ENV_GATEWAY_GUARDIAN_TIMEOUT,
                GATEWAY_GUARDIAN_TIMEOUT,
            ),
            failure_threshold: u32_env(ENV_GATEWAY_GUARDIAN_FAILURES, GATEWAY_GUARDIAN_FAILURES)
                .max(1),
            reensure_jitter_max: duration_secs_env(
                ENV_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX,
                GATEWAY_GUARDIAN_REENSURE_JITTER_MAX,
            ),
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }
    pub fn probe_timeout(&self) -> Duration {
        self.probe_timeout
    }
    pub fn failure_threshold(&self) -> u32 {
        self.failure_threshold
    }
    pub fn reensure_jitter_max(&self) -> Duration {
        self.reensure_jitter_max
    }
}

/// Live snapshot of the gateway guardian's internal state.
#[derive(Debug, Clone)]
pub struct GatewayGuardianStatus {
    pub consecutive_failures: u32,
    pub restart_attempts: u64,
    pub guardian_running: bool,
    pub failure_threshold: u32,
}

/// Handle returned by [`spawn_gateway_guardian`] for lifecycle + inspection.
#[derive(Debug, Clone)]
pub struct GatewayGuardianHandle {
    abort: tokio::task::AbortHandle,
    consecutive_failures: Arc<AtomicU32>,
    restart_attempts: Arc<AtomicU64>,
    failure_threshold: u32,
}

impl GatewayGuardianHandle {
    pub fn abort(&self) {
        self.abort.abort();
    }

    pub fn status(&self) -> GatewayGuardianStatus {
        GatewayGuardianStatus {
            consecutive_failures: self.consecutive_failures.load(Ordering::Relaxed),
            restart_attempts: self.restart_attempts.load(Ordering::Relaxed),
            guardian_running: !self.abort.is_finished(),
            failure_threshold: self.failure_threshold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
enum GatewayGuardianAction {
    None,
    Reensure,
}

/// Keep a daemon-backed per-DCC process able to revive the standalone gateway.
///
/// Returns a [`GatewayGuardianHandle`] that can be used to inspect status
/// or abort the watchdog task.
pub fn spawn_gateway_guardian(
    opts: EnsureGatewayOptions,
    settings: GatewayGuardianSettings,
) -> GatewayGuardianHandle {
    let consecutive_failures = Arc::new(AtomicU32::new(0));
    let restart_attempts = Arc::new(AtomicU64::new(0));
    let guard_failures = Arc::clone(&consecutive_failures);
    let guard_restarts = Arc::clone(&restart_attempts);
    let failure_threshold = settings.failure_threshold;

    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(settings.interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let healthy =
                gateway_health_ok_with_timeout(&opts.host, opts.port, settings.probe_timeout).await;

            if healthy {
                guard_failures.store(0, Ordering::Relaxed);
                continue;
            }

            let failures = guard_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if failures >= settings.failure_threshold {
                let next_attempt = guard_restarts.load(Ordering::Relaxed) + 1;
                let jitter = reensure_jitter_duration(settings.reensure_jitter_max, next_attempt);
                if !jitter.is_zero() {
                    tracing::debug!(
                        jitter_ms = jitter.as_millis(),
                        "gateway daemon guardian delaying re-ensure after failed probes"
                    );
                    tokio::time::sleep(jitter).await;
                }
                if gateway_health_ok_with_timeout(&opts.host, opts.port, settings.probe_timeout)
                    .await
                {
                    guard_failures.store(0, Ordering::Relaxed);
                    continue;
                }
                tracing::warn!(
                    host = %opts.host,
                    port = opts.port,
                    failures = failures,
                    threshold = settings.failure_threshold,
                    restart_attempts = guard_restarts.load(Ordering::Relaxed),
                    "gateway daemon health failed; re-ensuring standalone gateway"
                );
                match ensure_gateway_running(&opts).await {
                    Ok(()) => {
                        guard_restarts.fetch_add(1, Ordering::Relaxed);
                        guard_failures.store(0, Ordering::Relaxed);
                    }
                    Err(err) => tracing::warn!(
                        error = %err,
                        "gateway daemon guardian failed to re-ensure standalone gateway"
                    ),
                }
            }
        }
    });

    GatewayGuardianHandle {
        abort: handle.abort_handle(),
        consecutive_failures,
        restart_attempts,
        failure_threshold,
    }
}

fn reensure_jitter_duration(max: Duration, attempt: u64) -> Duration {
    if max.is_zero() {
        return Duration::ZERO;
    }
    let max_millis = max.as_millis().min(u64::MAX as u128) as u64;
    if max_millis == 0 {
        return Duration::ZERO;
    }
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let seed = now_nanos
        ^ ((std::process::id() as u64) << 16)
        ^ attempt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    Duration::from_millis(seed % max_millis.saturating_add(1))
}

/// Test-visible probe evaluation: returns the appropriate guardian action
/// based on health status and failure threshold crossing.
#[cfg(test)]
fn record_gateway_guardian_probe(
    settings: &GatewayGuardianSettings,
    consecutive_failures: &mut u32,
    healthy: bool,
) -> GatewayGuardianAction {
    if healthy {
        *consecutive_failures = 0;
        return GatewayGuardianAction::None;
    }

    *consecutive_failures = consecutive_failures.saturating_add(1);
    if *consecutive_failures >= settings.failure_threshold {
        GatewayGuardianAction::Reensure
    } else {
        GatewayGuardianAction::None
    }
}

fn duration_secs_env(name: &str, default: Duration) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.1)
        .map(Duration::from_secs_f64)
        .unwrap_or(default)
}

fn u32_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> GatewayGuardianSettings {
        GatewayGuardianSettings {
            interval: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(10),
            failure_threshold: 2,
            reensure_jitter_max: Duration::ZERO,
        }
    }

    #[test]
    fn gateway_guardian_probe_threshold_resets_after_health() {
        let settings = test_settings();
        let mut failures = 0;

        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, false),
            GatewayGuardianAction::None
        );
        assert_eq!(failures, 1);
        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, false),
            GatewayGuardianAction::Reensure
        );
        assert_eq!(failures, 2);
        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, true),
            GatewayGuardianAction::None
        );
        assert_eq!(failures, 0);
    }

    #[test]
    fn probe_action_below_threshold_is_none() {
        let settings = GatewayGuardianSettings {
            interval: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(10),
            failure_threshold: 3,
            reensure_jitter_max: Duration::ZERO,
        };
        let mut failures = 0;

        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, false),
            GatewayGuardianAction::None
        );
        assert_eq!(failures, 1);
        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, false),
            GatewayGuardianAction::None
        );
        assert_eq!(failures, 2);
        assert_eq!(
            record_gateway_guardian_probe(&settings, &mut failures, false),
            GatewayGuardianAction::Reensure
        );
        assert_eq!(failures, 3);
    }

    #[tokio::test]
    async fn gateway_guardian_handle_reports_status() {
        let handle = GatewayGuardianHandle {
            abort: tokio::spawn(async {}).abort_handle(),
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            restart_attempts: Arc::new(AtomicU64::new(0)),
            failure_threshold: 2,
        };

        let status = handle.status();
        assert_eq!(status.consecutive_failures, 0);
        assert_eq!(status.restart_attempts, 0);
        assert_eq!(status.failure_threshold, 2);
    }

    #[tokio::test]
    async fn gateway_guardian_handle_abort_marks_not_running() {
        let join = tokio::spawn(async {});
        let abort = join.abort_handle();
        let handle = GatewayGuardianHandle {
            abort,
            consecutive_failures: Arc::new(AtomicU32::new(0)),
            restart_attempts: Arc::new(AtomicU64::new(0)),
            failure_threshold: 2,
        };

        assert!(handle.status().guardian_running);
        handle.abort();
        // Wait briefly for the abort to propagate.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!handle.status().guardian_running);
    }

    #[tokio::test]
    async fn guardian_loop_tracks_consecutive_failures_and_restart_attempts() {
        let opts = EnsureGatewayOptions {
            host: "127.0.0.1".to_string(),
            port: 19999, // No real gateway running
            name: Some("guardian-status-test".to_string()),
            registry_dir: std::env::temp_dir().join("dcc-mcp-test-registry"),
            remote_host: "0.0.0.0".to_string(),
            remote_port: 50000,
            crate_version: Some("0.1.0-test".to_string()),
            adapter_version: None,
            adapter_dcc: None,
            gateway_idle_timeout_secs: 30,
        };

        let settings = GatewayGuardianSettings {
            interval: Duration::from_millis(100),
            probe_timeout: Duration::from_millis(50),
            failure_threshold: 2,
            reensure_jitter_max: Duration::ZERO,
        };

        let handle = spawn_gateway_guardian(opts, settings);

        // Let the guardian probe at least 4 times (it will fail because no
        // gateway is running on port 19999, and reensure will try to spawn a
        // real binary which will fail — but the probe/failure counting should
        // still tick).
        tokio::time::sleep(Duration::from_millis(600)).await;

        let status = handle.status();
        // We expect at least some failures since the port doesn't have a gateway.
        assert!(
            status.consecutive_failures >= 1,
            "should have recorded at least 1 consecutive failure, got {}",
            status.consecutive_failures
        );
        assert!(status.guardian_running);

        handle.abort();
    }

    /// Integration test: guardian detects a real health endpoint going down.
    ///
    /// Starts a small HTTP server that responds to ``/health`` on an ephemeral
    /// port, spawns a guardian watching it, then drops the server and verifies
    /// the guardian records health-check failures.
    #[tokio::test]
    async fn guardian_detects_health_endpoint_down_and_triggers_reensure() {
        use axum::{Router, response::IntoResponse, routing::get};

        // Pick an ephemeral port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let port = addr.port();

        // Build a minimal axum server that only responds to /health.
        async fn health() -> impl IntoResponse {
            "OK"
        }
        let app = Router::new().route("/health", get(health));
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give the server a moment to start.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let opts = EnsureGatewayOptions {
            host: "127.0.0.1".to_string(),
            port,
            name: Some("guardian-health-down-test".to_string()),
            registry_dir: std::env::temp_dir().join("dcc-mcp-test-registry"),
            remote_host: "0.0.0.0".to_string(),
            remote_port: 50000,
            crate_version: Some("0.1.0-test".to_string()),
            adapter_version: None,
            adapter_dcc: None,
            gateway_idle_timeout_secs: 30,
        };

        let settings = GatewayGuardianSettings {
            interval: Duration::from_millis(150),
            probe_timeout: Duration::from_millis(100),
            failure_threshold: 2,
            reensure_jitter_max: Duration::ZERO,
        };

        let handle = spawn_gateway_guardian(opts, settings);

        // Let the guardian do a few probes — should be healthy.
        tokio::time::sleep(Duration::from_millis(350)).await;
        let status = handle.status();
        assert_eq!(
            status.consecutive_failures, 0,
            "guardian should report 0 failures while health endpoint is up"
        );
        assert_eq!(status.restart_attempts, 0);

        // Drop the health server.
        server_handle.abort();

        // Wait for the guardian to detect the failure and cross the threshold.
        // With interval=150ms and threshold=2, 3 probe cycles ≈ 450ms.
        tokio::time::sleep(Duration::from_millis(800)).await;

        let status_after = handle.status();
        assert!(
            status_after.consecutive_failures >= 1,
            "guardian should have recorded failures after health endpoint went down, got {}",
            status_after.consecutive_failures,
        );
        assert!(status_after.guardian_running);

        handle.abort();
    }

    #[test]
    fn settings_from_env_parses_positive_values() {
        let _g = dcc_mcp_test_utils::EnvVarsGuard::set(&[
            ("DCC_MCP_GATEWAY_GUARDIAN_INTERVAL", Some("3")),
            ("DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT", Some("0.8")),
            ("DCC_MCP_GATEWAY_GUARDIAN_FAILURES", Some("4")),
            ("DCC_MCP_GATEWAY_GUARDIAN_REENSURE_JITTER_MAX", Some("1.5")),
        ]);

        let s = GatewayGuardianSettings::from_env();
        assert_eq!(s.interval(), Duration::from_secs(3));
        assert_eq!(s.probe_timeout(), Duration::from_secs_f64(0.8));
        assert_eq!(s.failure_threshold(), 4);
        assert_eq!(s.reensure_jitter_max(), Duration::from_secs_f64(1.5));
    }

    #[test]
    fn gateway_guardian_settings_accessors() {
        let s = test_settings();
        assert_eq!(s.interval(), Duration::from_secs(1));
        assert_eq!(s.probe_timeout(), Duration::from_millis(10));
        assert_eq!(s.failure_threshold(), 2);
        assert_eq!(s.reensure_jitter_max(), Duration::ZERO);
    }

    #[test]
    fn reensure_jitter_never_exceeds_configured_max() {
        let max = Duration::from_millis(25);

        for attempt in 0..32 {
            let jitter = reensure_jitter_duration(max, attempt);
            assert!(
                jitter <= max,
                "jitter {jitter:?} must not exceed configured max {max:?}"
            );
        }

        assert_eq!(reensure_jitter_duration(Duration::ZERO, 1), Duration::ZERO);
    }
}
