use super::*;

use dcc_mcp_transport::discovery::file_registry::FileRegistry;

/// Normalise a host string to a canonical form so that `127.0.0.1`,
/// `localhost`, and `::1` compare equal when we ask "is this entry my own
/// address?".
///
/// We can't rely on full DNS resolution here — the gateway runs in embedded
/// hosts (mayapy, Blender's Python) where blocking DNS lookups are a very
/// bad idea — so we do a cheap textual normalisation instead. This is
/// sufficient because every real binding in `GatewayConfig` is an IP
/// literal or the string `"localhost"` (see `GatewayConfig::default`).
fn normalise_host(host: &str) -> &str {
    match host {
        "localhost" | "::1" | "0.0.0.0" | "[::]" | "[::0]" => "127.0.0.1",
        other => other,
    }
}

/// Is `entry` the gateway's own advertised (host, port) pair?
///
/// Issue #419: when a DCC process (Maya/Blender/Houdini) wins the gateway
/// election it keeps its plain-instance `ServiceEntry` in `FileRegistry`
/// alongside the `__gateway__` sentinel. Without this filter the gateway's
/// backend-SSE subscriber would open a connection to its own `/mcp` endpoint
/// — a classic self-loop that wastes a socket and floods the reconnect logs
/// whenever the facade blips.
///
/// The sentinel row is filtered elsewhere via `GATEWAY_SENTINEL_DCC_TYPE`;
/// this helper is for the plain DCC row.
pub(crate) fn is_own_instance(
    entry: &dcc_mcp_transport::discovery::types::ServiceEntry,
    own_host: &str,
    own_port: u16,
) -> bool {
    entry.port == own_port && normalise_host(&entry.host) == normalise_host(own_host)
}

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

#[cfg(test)]
mod own_instance_tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceEntry;

    #[test]
    fn exact_host_and_port_match() {
        let e = ServiceEntry::new("maya", "127.0.0.1", 9765);
        assert!(is_own_instance(&e, "127.0.0.1", 9765));
    }

    #[test]
    fn port_mismatch_is_not_self() {
        let e = ServiceEntry::new("maya", "127.0.0.1", 18812);
        assert!(!is_own_instance(&e, "127.0.0.1", 9765));
    }

    #[test]
    fn localhost_aliases_collapse_to_loopback() {
        for alias in ["localhost", "::1", "0.0.0.0", "[::]"] {
            let e = ServiceEntry::new("maya", alias, 9765);
            assert!(
                is_own_instance(&e, "127.0.0.1", 9765),
                "alias {alias} must match 127.0.0.1"
            );
        }
    }

    #[test]
    fn distinct_remote_host_is_not_self() {
        let e = ServiceEntry::new("maya", "10.0.0.5", 9765);
        assert!(!is_own_instance(&e, "127.0.0.1", 9765));
    }
}
