use super::*;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;

/// Helper: does the sentinel advertise a newer gateway version than us?
///
/// Issue #228: the old implementation scanned every DCC instance entry and
/// compared its `version` field (which is the DCC host version — e.g. Maya
/// `"2024"`) against our crate version (e.g. `"0.14.3"`), causing semver
/// comparison to flag every running DCC as a "newer challenger" and trigger
/// a self-yield within 15 s of startup.
///
/// A newer gateway instance will always rewrite the `__gateway__` sentinel with
/// its own crate version — so that sentinel row is the **only** reliable source
/// of "is there a newer gateway challenger on the network". Any comparison must
/// therefore be restricted to the sentinel row, and it must ignore our own
/// sentinel write (same version, same host, same port).
pub(crate) fn has_newer_sentinel(
    reg: &FileRegistry,
    own_version: &str,
    stale_timeout: Duration,
) -> bool {
    reg.list_instances(GATEWAY_SENTINEL_DCC_TYPE)
        .into_iter()
        .any(|e| {
            !e.is_stale(stale_timeout)
                && e.version
                    .as_deref()
                    .map(|v| is_newer_version(v, own_version))
                    .unwrap_or(false)
        })
}
