#[cfg(feature = "gateway-daemon")]
use std::path::PathBuf;

use super::SidecarArgs;

#[cfg(feature = "gateway-daemon")]
pub(crate) fn build_gateway_daemon_options(
    args: &SidecarArgs,
    registry_dir: PathBuf,
) -> crate::gateway_daemon::EnsureGatewayOptions {
    let gateway_host = args
        .gateway_host
        .clone()
        .unwrap_or_else(|| args.host.clone());
    crate::gateway_daemon::EnsureGatewayOptions {
        host: gateway_host,
        port: args.gateway_port,
        name: args.gateway_name.clone().or_else(|| {
            args.display_name
                .as_ref()
                .map(|name| format!("gateway-for-{name}"))
        }),
        registry_dir,
        remote_host: args.gateway_remote_host.clone(),
        remote_port: args.gateway_remote_port,
    }
}

#[cfg(feature = "gateway-daemon")]
pub(crate) fn should_start_gateway_daemon_guardian(args: &SidecarArgs) -> bool {
    should_use_gateway_daemon(args)
}

#[cfg(feature = "gateway-daemon")]
pub(crate) fn should_use_gateway_daemon(args: &SidecarArgs) -> bool {
    args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election
}

#[cfg(feature = "gateway-daemon")]
pub(crate) fn sidecar_gateway_runtime_mode(args: &SidecarArgs) -> &'static str {
    if args.gateway_port == 0 {
        "not_configured"
    } else if args.no_ensure_gateway {
        "failover_disabled_by_adapter"
    } else if args.legacy_gateway_election {
        "embedded-fallback"
    } else {
        "daemon-backed"
    }
}

#[cfg(not(feature = "gateway-daemon"))]
pub(crate) fn sidecar_gateway_runtime_mode(args: &SidecarArgs) -> &'static str {
    if args.gateway_port == 0 {
        "not_configured"
    } else if args.no_ensure_gateway {
        "failover_disabled_by_adapter"
    } else if args.legacy_gateway_election {
        "embedded-fallback"
    } else {
        "daemon-unavailable"
    }
}

#[cfg(feature = "gateway-daemon")]
pub(crate) fn sidecar_gateway_guardian_enabled(args: &SidecarArgs) -> bool {
    should_start_gateway_daemon_guardian(args)
}

#[cfg(not(feature = "gateway-daemon"))]
pub(crate) fn sidecar_gateway_guardian_enabled(_args: &SidecarArgs) -> bool {
    false
}
