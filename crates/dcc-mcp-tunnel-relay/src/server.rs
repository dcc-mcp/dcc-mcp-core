//! Top-level relay server: holds the shared registry, binds the agent
//! and frontend listeners, and spawns the eviction sweeper.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

use crate::config::RelayConfig;
use crate::control::handle_agent;
use crate::data::handle_frontend;
use crate::eviction::spawn_eviction_loop;
use crate::registry::TunnelRegistry;

/// Default sweep cadence. Mirrors the `stale_timeout` default so a tunnel
/// that misses one full heartbeat cycle is evicted on the next pass.
const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(15);

/// Optional bind addresses that toggle the secondary transports on.
/// Pass via [`RelayServer::start_with`]. The default
/// [`RelayServer::start`] keeps both `None` for backward compatibility.
#[derive(Debug, Clone, Default)]
pub struct OptionalBinds {
    /// `127.0.0.1:9876`-style address for the WebSocket frontend
    /// transport. Use the same host:port that your reverse proxy
    /// terminates TLS on.
    pub ws_frontend: Option<SocketAddr>,

    /// `127.0.0.1:9877`-style address for the read-only admin endpoint
    /// (`/tunnels`, `/healthz`).
    pub admin: Option<SocketAddr>,
}

/// Running relay handle returned by [`RelayServer::start`]. Drop it (or
/// call [`RelayServer::shutdown`]) to stop accepting connections; live
/// sessions wind down on their own as their backing sockets close.
#[derive(Debug)]
pub struct RelayServer {
    /// Shared tunnel registry — exposed so the admin endpoint and any
    /// custom telemetry can read it without going through the relay
    /// state.
    pub registry: Arc<TunnelRegistry>,

    /// Address the agent listener actually bound to (resolves `:0`).
    pub agent_addr: SocketAddr,

    /// Address the TCP frontend listener actually bound to (resolves `:0`).
    pub frontend_addr: SocketAddr,

    /// Address the WebSocket frontend listener bound to, when enabled.
    pub ws_frontend_addr: Option<SocketAddr>,

    /// Address the admin endpoint bound to, when enabled.
    pub admin_addr: Option<SocketAddr>,

    agent_task: tokio::task::JoinHandle<()>,
    frontend_task: tokio::task::JoinHandle<()>,
    eviction_task: tokio::task::JoinHandle<()>,
    ws_frontend_task: Option<tokio::task::JoinHandle<()>>,
    admin_task: Option<tokio::task::JoinHandle<()>>,
}

impl RelayServer {
    /// Bind the agent + TCP frontend listeners on the supplied addresses
    /// and start serving. The WebSocket frontend and admin endpoint stay
    /// **off** in this overload — call [`RelayServer::start_with`] to
    /// enable them.
    ///
    /// Use `"127.0.0.1:0"` to let the OS pick a port; the resolved
    /// addresses are exposed on the returned struct for tests.
    pub async fn start(
        config: RelayConfig,
        agent_bind: SocketAddr,
        frontend_bind: SocketAddr,
    ) -> std::io::Result<Self> {
        Self::start_with(config, agent_bind, frontend_bind, OptionalBinds::default()).await
    }

    /// Same as [`RelayServer::start`] but additionally enables the
    /// optional WebSocket frontend and/or admin endpoint when their
    /// addresses are populated in `optional`.
    pub async fn start_with(
        config: RelayConfig,
        agent_bind: SocketAddr,
        frontend_bind: SocketAddr,
        optional: OptionalBinds,
    ) -> std::io::Result<Self> {
        let config = Arc::new(config);
        let registry = Arc::new(TunnelRegistry::new());

        let agent_listener = TcpListener::bind(agent_bind).await?;
        let frontend_listener = TcpListener::bind(frontend_bind).await?;
        let agent_addr = agent_listener.local_addr()?;
        let frontend_addr = frontend_listener.local_addr()?;
        info!(%agent_addr, %frontend_addr, "tunnel relay listening");

        let agent_task = spawn_accept_loop(
            agent_listener,
            "agent",
            Arc::clone(&registry),
            Arc::clone(&config),
            move |s, reg, cfg| Box::pin(handle_agent(s, reg, cfg)),
        );

        let frontend_task = {
            let registry = Arc::clone(&registry);
            tokio::spawn(async move {
                loop {
                    match frontend_listener.accept().await {
                        Ok((stream, peer)) => {
                            tracing::debug!(%peer, "frontend connection accepted");
                            let registry = Arc::clone(&registry);
                            tokio::spawn(handle_frontend(stream, registry));
                        }
                        Err(e) => {
                            warn!(error = %e, "frontend accept failed; backing off");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            })
        };

        let eviction_task = spawn_eviction_loop(
            Arc::clone(&registry),
            Arc::clone(&config),
            DEFAULT_SWEEP_INTERVAL,
        );

        let (ws_frontend_addr, ws_frontend_task) = match optional.ws_frontend {
            Some(bind) => {
                let (addr, task) = crate::ws_frontend::serve(bind, Arc::clone(&registry)).await?;
                (Some(addr), Some(task))
            }
            None => (None, None),
        };
        let (admin_addr, admin_task) = match optional.admin {
            Some(bind) => {
                let (addr, task) = crate::admin::serve(bind, Arc::clone(&registry)).await?;
                (Some(addr), Some(task))
            }
            None => (None, None),
        };

        Ok(Self {
            registry,
            agent_addr,
            frontend_addr,
            ws_frontend_addr,
            admin_addr,
            agent_task,
            frontend_task,
            eviction_task,
            ws_frontend_task,
            admin_task,
        })
    }

    /// Stop accepting new connections. Currently-in-flight sessions are
    /// not interrupted; they wind down when their sockets close.
    pub fn shutdown(self) {
        self.agent_task.abort();
        self.frontend_task.abort();
        self.eviction_task.abort();
        if let Some(t) = self.ws_frontend_task {
            t.abort();
        }
        if let Some(t) = self.admin_task {
            t.abort();
        }
    }
}

fn spawn_accept_loop<F>(
    listener: TcpListener,
    role: &'static str,
    registry: Arc<TunnelRegistry>,
    config: Arc<RelayConfig>,
    handler: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(
            TcpStream,
            Arc<TunnelRegistry>,
            Arc<RelayConfig>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
        + Send
        + Sync
        + 'static,
{
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    tracing::debug!(role, %peer, "connection accepted");
                    let registry = Arc::clone(&registry);
                    let config = Arc::clone(&config);
                    let fut = handler(stream, registry, config);
                    tokio::spawn(fut);
                }
                Err(e) => {
                    warn!(role, error = %e, "accept failed; backing off");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    })
}
