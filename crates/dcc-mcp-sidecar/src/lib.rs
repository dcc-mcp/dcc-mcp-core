//! Sidecar and gateway-daemon runtime support for DCC-MCP.
//!
//! This crate keeps the long-lived sidecar control plane separate from the
//! user-facing `dcc-mcp-server` binary while preserving the same CLI entry
//! points through that binary.

#[cfg(feature = "gateway-daemon")]
pub mod gateway_daemon;
#[cfg(feature = "gateway-auto")]
pub mod sidecar;
#[cfg(feature = "gateway-auto")]
pub mod sidecar_gateway;
#[cfg(feature = "gateway-auto")]
pub mod sidecar_mcp;

#[cfg(feature = "gateway-auto")]
pub use sidecar::{ExitReason, SidecarArgs, run};

#[cfg(feature = "gateway-auto")]
pub(crate) use dcc_mcp_gateway_ensure::is_process_alive;

#[cfg(any(feature = "gateway-auto", feature = "gateway-daemon"))]
pub(crate) async fn select_shutdown_signal() -> anyhow::Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("ctrl_c")
            }
            _ = term.recv() => Ok("sigterm"),
        }
    }
    #[cfg(windows)]
    {
        let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
        let mut ctrl_shutdown = tokio::signal::windows::ctrl_shutdown()?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("ctrl_c")
            }
            _ = ctrl_break.recv() => Ok("ctrl_break"),
            _ = ctrl_shutdown.recv() => Ok("ctrl_shutdown"),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        tokio::signal::ctrl_c().await?;
        Ok("ctrl_c")
    }
}
