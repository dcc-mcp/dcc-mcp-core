//! Process-lifetime helpers shared by file-based discovery reads.

use sysinfo::{Pid, ProcessesToUpdate, System};

use super::types::ServiceEntry;

const ROLE_METADATA_KEY: &str = "dcc_mcp_role";
const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";
const SIDECAR_PID_METADATA_KEY: &str = "sidecar_pid";

/// Return `true` when `pid` refers to a currently running OS process.
pub(super) fn is_pid_alive(pid: u32) -> bool {
    let process_id = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[process_id]), true);
    system.process(process_id).is_some()
}

/// Recover the bound DCC PID from pre-`host_pid` sidecar rows.
///
/// Those rows stored the watched DCC in `pid`, while the actual registry owner
/// appeared in `metadata.sidecar_pid` and held the sentinel lock. Restrict the
/// inference to the explicit sidecar role and unequal, valid PIDs so ordinary
/// legacy services keep their sentinel-first owner semantics.
pub(super) fn legacy_sidecar_host_pid(entry: &ServiceEntry) -> Option<u32> {
    if entry.host_pid.is_some()
        || entry.metadata.get(ROLE_METADATA_KEY).map(String::as_str) != Some(ROLE_PER_DCC_SIDECAR)
    {
        return None;
    }
    let watched_pid = entry.pid?;
    let sidecar_pid = entry
        .metadata
        .get(SIDECAR_PID_METADATA_KEY)?
        .parse::<u32>()
        .ok()?;
    (watched_pid != sidecar_pid).then_some(watched_pid)
}
