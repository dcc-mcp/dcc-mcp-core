//! Recovery for a gateway that holds its port but no longer serves (#2405).
//!
//! A gateway process can stay alive and keep accepting TCP connections after
//! its embedded service stops answering HTTP. Such a holder never releases the
//! port on its own, so a challenger that only ever promotes by binding waits
//! until `challenger_timeout_secs` and then gives up. This module detects that
//! state and releases the port.
//!
//! Terminating another process is destructive, so a holder is only reaped when
//! its PID is provably a gateway this deployment started — see
//! [`service_dead_port_holder_pid`].

use super::bind::try_bind_port_opt;
use super::*;

/// Consecutive failed bind attempts after which the challenger stops waiting
/// for the holder to release the port on its own and reaps it instead
/// (issue #2405).
///
/// A process that is alive but has stopped serving keeps `accept()`ing, so the
/// port is never released and the bind-poll loop can never promote. Each
/// challenger attempt takes `challenger_poll_interval_secs`, and the reap runs
/// on the attempt where the count reaches this threshold, so recovery completes
/// in at most `RECOVERY_BIND_ATTEMPTS * poll_interval` seconds. At the 10 s
/// default that is 30 s — five times faster than the 120 s version takeover
/// ceiling, and unchanged for the far more common "port is free or refused"
/// paths, which promote on the first attempt.
pub(crate) const RECOVERY_BIND_ATTEMPTS: u32 = 3;

/// The process ID to terminate so a new gateway can bind `port`, or `None`
/// when the evidence is not strong enough to justify a kill.
///
/// Killing a process we do not own is destructive, so this only returns a PID
/// when every condition holds:
///
/// 1. The port genuinely has a listener PID (see
///    [`dcc_mcp_gateway_ensure::listener_pids_on_port`]; an empty result means
///    "unknown", never "reap it").
/// 2. That PID is not this process — self-termination is never recovery.
/// 3. Exactly one PID holds the listener, so a shared or hijacked port does
///    not turn into an ambiguous kill.
/// 4. The PID is recorded as this deployment's gateway in the pidfile or in a
///    gateway autolaunch manifest inside `registry_dir`. Only a PID we can
///    prove this deployment started is reaped; an unrelated application that
///    happens to hold the port is left alone.
pub(crate) fn service_dead_port_holder_pid(
    port: u16,
    registry_dir: &Path,
    pidfile: Option<&Path>,
) -> Option<u32> {
    let pids = dcc_mcp_gateway_ensure::listener_pids_on_port(port);
    if pids.len() != 1 {
        return None;
    }
    let pid = pids[0];
    if pid == std::process::id() || !dcc_mcp_gateway_ensure::is_process_alive(pid) {
        return None;
    }
    gateway_launch_owns_pid(registry_dir, pidfile, pid).then_some(pid)
}

/// True when `pid` is recorded as a gateway this deployment launched.
fn gateway_launch_owns_pid(registry_dir: &Path, pidfile: Option<&Path>, pid: u32) -> bool {
    if dcc_mcp_gateway_ensure::read_pid_from_pidfile(pidfile) == Some(pid) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(registry_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        let is_manifest = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("gateway-autolaunch-") && name.ends_with(".json"));
        if !is_manifest {
            return false;
        }
        std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|manifest| manifest.get("pid").and_then(serde_json::Value::as_u64))
            == Some(u64::from(pid))
    })
}

/// Terminate a service-dead port holder and wait for the kernel to release the
/// port so the caller's next bind attempt can succeed.
///
/// Returns `true` only when the port is observed free afterwards — a killed
/// process's TCP listener is released synchronously on Windows, but the
/// confirmation keeps the caller's decision honest on every platform.
pub(crate) async fn reap_service_dead_port_holder(
    host: &str,
    port: u16,
    pid: u32,
    release_timeout: Duration,
) -> bool {
    tracing::warn!(
        pid = pid,
        port = port,
        "Challenger: resident gateway accepted TCP but never answered readiness — reaping stale holder to release the port"
    );
    if let Err(err) = dcc_mcp_gateway_ensure::stop_process(pid) {
        tracing::warn!(pid = pid, error = %err, "Challenger: could not stop stale gateway holder");
        return false;
    }
    let deadline = tokio::time::Instant::now() + release_timeout;
    loop {
        if try_bind_port_opt(host, port).await.is_some() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            tracing::warn!(
                pid = pid,
                port = port,
                "Challenger: port still held after reaping stale gateway holder"
            );
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
#[path = "port_recovery_tests.rs"]
mod port_recovery_tests;
