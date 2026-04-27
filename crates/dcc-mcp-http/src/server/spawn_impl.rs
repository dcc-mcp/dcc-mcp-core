use axum::Router;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};

use crate::config::{McpHttpConfig, ServerSpawnMode};
use crate::error::{HttpError, HttpResult};

pub(crate) async fn spawn_http_server(
    listener: TcpListener,
    router: Router,
    config: &McpHttpConfig,
    actual_bind: String,
    port: u16,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
) -> HttpResult<(Option<JoinHandle<()>>, Option<std::thread::JoinHandle<()>>)> {
    match config.spawn_mode {
        ServerSpawnMode::Ambient => {
            spawn_ambient(
                listener,
                router,
                config,
                actual_bind,
                port,
                shutdown_tx,
                shutdown_rx,
            )
            .await
        }
        ServerSpawnMode::Dedicated => {
            spawn_dedicated(
                listener,
                router,
                config,
                actual_bind,
                port,
                shutdown_tx,
                shutdown_rx,
            )
            .await
        }
    }
}

async fn spawn_ambient(
    listener: TcpListener,
    router: Router,
    config: &McpHttpConfig,
    actual_bind: String,
    port: u16,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> HttpResult<(Option<JoinHandle<()>>, Option<std::thread::JoinHandle<()>>)> {
    let join = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                loop {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                }
            })
            .await
            .ok();
        tracing::info!("MCP HTTP server stopped");
    });

    if config.self_probe_timeout_ms > 0 {
        let probe_host = if config.host.is_unspecified() {
            "127.0.0.1".to_string()
        } else {
            config.host.to_string()
        };
        let probe_addr = format!("{probe_host}:{port}");
        if !self_probe(&probe_addr, config.self_probe_timeout_ms).await {
            let _ = shutdown_tx.send(true);
            let _ = join.await;
            return Err(HttpError::BindFailed {
                addr: actual_bind,
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "instance listener self-probe failed (issue #303 guard)",
                ),
            });
        }
    }

    Ok((Some(join), None))
}

async fn spawn_dedicated(
    listener: TcpListener,
    router: Router,
    config: &McpHttpConfig,
    actual_bind: String,
    _port: u16,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> HttpResult<(Option<JoinHandle<()>>, Option<std::thread::JoinHandle<()>>)> {
    let rebind_addr = actual_bind.clone();
    drop(listener);

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), std::io::Error>>(1);
    let self_probe_timeout_ms = config.self_probe_timeout_ms;
    let probe_bind = actual_bind.clone();

    let thread = std::thread::Builder::new()
        .name(format!("dcc-mcp-http-{}", _port))
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    let _ = ready_tx.send(Err(std::io::Error::other(format!(
                        "failed to build dedicated runtime: {err}"
                    ))));
                    return;
                }
            };
            runtime.block_on(async move {
                let listener = match TcpListener::bind(&rebind_addr).await {
                    Ok(listener) => listener,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                axum::serve(listener, router)
                    .with_graceful_shutdown(async move {
                        loop {
                            if *shutdown_rx.borrow() {
                                break;
                            }
                            if shutdown_rx.changed().await.is_err() {
                                break;
                            }
                        }
                    })
                    .await
                    .ok();
                tracing::info!("MCP HTTP server (dedicated) stopped");
            });
        })
        .map_err(|err| HttpError::BindFailed {
            addr: actual_bind.clone(),
            source: err,
        })?;

    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(HttpError::BindFailed {
                addr: actual_bind,
                source: err,
            });
        }
        Err(err) => {
            return Err(HttpError::BindFailed {
                addr: actual_bind,
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("dedicated thread did not signal readiness: {err}"),
                ),
            });
        }
    }

    if self_probe_timeout_ms > 0 && !self_probe(&probe_bind, self_probe_timeout_ms).await {
        let _ = shutdown_tx.send(true);
        return Err(HttpError::BindFailed {
            addr: probe_bind,
            source: std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "dedicated listener self-probe failed (issue #303 guard)",
            ),
        });
    }

    Ok((None, Some(thread)))
}

async fn self_probe(probe_addr: &str, timeout_ms: u64) -> bool {
    let timeout = std::time::Duration::from_millis(timeout_ms);
    for _ in 0..5 {
        match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(probe_addr)).await {
            Ok(Ok(_)) => return true,
            _ => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
    false
}
