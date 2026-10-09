//! Canonical source of truth for the gateway idle-timeout defaults.
//!
//! The same knob (`--gateway-idle-timeout-secs` / `DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS`)
//! has a different *effective* default per entry point, and those values are
//! intentionally not interchangeable:
//!
//! - `SERVER_DEFAULT` (30 s) applies when a process runs the gateway server
//!   directly (`GatewayConfig::default()`, the standalone daemon CLI, the
//!   embedded auto-gateway in `dcc-mcp-http`). Such a gateway is usually
//!   started on demand next to a live DCC, so a short grace period is what
//!   the operator expects.
//! - `AUTO_ENSURE_DEFAULT` (300 s) applies when some *other* process
//!   auto-launches a machine-wide gateway on this machine's behalf
//!   (`ensure_gateway_running`, the Python guardian). Auto-ensure is a
//!   shared, long-lived resource: several DCCs come and go underneath it,
//!   and restarting it costs a full re-election, so it gets a much longer
//!   grace period.
//! - `MIN_AUTOLAUNCH` (30 s) is the floor an auto-ensure caller may be
//!   clamped down to.
//!
//! These are *sources*, not a merged value: hard-error tests below pin each
//! number independently so a future refactor cannot silently unify them.

/// Default grace period, in seconds, for a gateway server started directly.
///
/// Used by [`crate::GatewayConfig::default`], the `gateway` CLI subcommand
/// (`crates/dcc-mcp-sidecar/src/gateway_daemon.rs`) and the embedded
/// auto-gateway in `dcc-mcp-http`.
pub const SERVER_DEFAULT: u64 = 30;

/// Default grace period, in seconds, when a gateway is auto-launched on
/// behalf of another process (Rust `ensure_gateway_running`, Python
/// `gateway_guardian`).
///
/// Longer than [`SERVER_DEFAULT`] because an auto-ensured gateway is a
/// shared machine-wide resource rather than a per-DCC companion.
pub const AUTO_ENSURE_DEFAULT: u64 = 300;

/// Lower bound, in seconds, that an auto-launch caller may clamp a
/// caller-supplied idle timeout down to.
pub const MIN_AUTOLAUNCH: u64 = 30;

/// Default grace period for `dcc-mcp-cli gateway daemon start`: an
/// explicitly managed daemon stays alive with no backends, so the entry
/// point opts out of idle shutdown entirely.
pub const CLI_DAEMON_START_DEFAULT: u64 = 0;

/// How long the gateway waits for in-flight requests to finish after the
/// idle timeout fires, before its task group is aborted.
///
/// The idle timer only advances while zero backends are live, so in the
/// common case there is nothing in flight to drain. The window exists so a
/// future gateway that can route without a live backend (built-in tools,
/// relay forwarding) does not cut connections off mid-request.
pub const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Polling cadence of the idle timer.
///
/// The timer samples the live-backend count on this interval, so shutdown
/// actually lands somewhere in `[grace, grace + IDLE_POLL)`. That is
/// immaterial for a 300 s grace period but worth knowing when debugging
/// with a small one.
pub const IDLE_POLL: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    /// The three values are semantically distinct. Unifying them is exactly
    /// the silent behaviour change this module exists to prevent.
    #[test]
    fn entry_point_defaults_stay_distinct() {
        assert_eq!(SERVER_DEFAULT, 30);
        assert_eq!(AUTO_ENSURE_DEFAULT, 300);
        assert_eq!(MIN_AUTOLAUNCH, 30);
        assert_eq!(CLI_DAEMON_START_DEFAULT, 0);
        assert_ne!(SERVER_DEFAULT, AUTO_ENSURE_DEFAULT);
    }

    #[test]
    fn drain_window_is_short_but_non_zero() {
        assert!(DRAIN_GRACE > std::time::Duration::from_secs(0));
        assert!(DRAIN_GRACE <= std::time::Duration::from_secs(10));
        assert_eq!(IDLE_POLL, std::time::Duration::from_secs(5));
    }
}
