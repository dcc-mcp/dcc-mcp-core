use std::time::Duration;

use super::launcher::{
    EnsureGatewayOptions, ensure_gateway_running, gateway_health_ok_with_timeout,
};

const GATEWAY_GUARDIAN_INTERVAL: Duration = Duration::from_secs(5);
const GATEWAY_GUARDIAN_TIMEOUT: Duration = Duration::from_millis(500);
const GATEWAY_GUARDIAN_FAILURES: u32 = 2;

const ENV_GATEWAY_GUARDIAN_INTERVAL: &str = "DCC_MCP_GATEWAY_GUARDIAN_INTERVAL";
const ENV_GATEWAY_GUARDIAN_TIMEOUT: &str = "DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT";
const ENV_GATEWAY_GUARDIAN_FAILURES: &str = "DCC_MCP_GATEWAY_GUARDIAN_FAILURES";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GatewayGuardianSettings {
    interval: Duration,
    probe_timeout: Duration,
    failure_threshold: u32,
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayGuardianAction {
    None,
    Reensure,
}

/// Keep a daemon-backed per-DCC process able to revive the standalone gateway.
pub fn spawn_gateway_guardian(
    opts: EnsureGatewayOptions,
    settings: GatewayGuardianSettings,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut consecutive_failures = 0u32;
        let mut interval = tokio::time::interval(settings.interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let healthy =
                gateway_health_ok_with_timeout(&opts.host, opts.port, settings.probe_timeout).await;
            match record_gateway_guardian_probe(&settings, &mut consecutive_failures, healthy) {
                GatewayGuardianAction::None => {}
                GatewayGuardianAction::Reensure => {
                    tracing::warn!(
                        host = %opts.host,
                        port = opts.port,
                        failures = consecutive_failures,
                        threshold = settings.failure_threshold,
                        "gateway daemon health failed; re-ensuring standalone gateway"
                    );
                    match ensure_gateway_running(&opts).await {
                        Ok(()) => consecutive_failures = 0,
                        Err(err) => tracing::warn!(
                            error = %err,
                            "gateway daemon guardian failed to re-ensure standalone gateway"
                        ),
                    }
                }
            }
        }
    })
}

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

    #[test]
    fn gateway_guardian_probe_threshold_resets_after_health() {
        let settings = GatewayGuardianSettings {
            interval: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(10),
            failure_threshold: 2,
        };
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
}
