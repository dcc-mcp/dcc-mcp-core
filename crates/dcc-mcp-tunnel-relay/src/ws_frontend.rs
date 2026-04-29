//! WebSocket frontend transport.
//!
//! Accepts plain HTTP/1.1 WebSocket upgrades — TLS is the operator's
//! reverse proxy concern (`nginx` / `caddy` / cloud LB), exactly the same
//! split the rest of the project uses. The relay is run behind a TLS
//! terminator on the public internet and listens on a private port here.
//!
//! ## Routing
//!
//! - `/tunnel/{id}` — connect to the tunnel with that id.
//! - Any other path returns `404`.
//!
//! ## Wire mapping
//!
//! - **Client → agent**: every binary WS message becomes one
//!   [`Frame::Data`] with the same payload.
//! - **Agent → client**: every payload received on the per-session inbox
//!   is sent as one binary WS message.
//! - Text messages, ping/pong, and close are handled by axum/tungstenite
//!   directly; close on either side tears down the multiplexed session.

use std::sync::Arc;

use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

use dcc_mcp_tunnel_protocol::{CloseReason, Frame};

use crate::control::SESSION_INBOX_QUEUE;
use crate::registry::TunnelRegistry;

/// Build the WebSocket frontend router. Exposed for tests so they can
/// run it inside an in-process `axum::serve` without binding a real port.
pub fn router(registry: Arc<TunnelRegistry>) -> Router {
    Router::new()
        .route("/tunnel/{id}", get(upgrade))
        .with_state(registry)
}

async fn upgrade(
    ws: WebSocketUpgrade,
    Path(tunnel_id): Path<String>,
    State(reg): State<Arc<TunnelRegistry>>,
) -> impl IntoResponse {
    let Some(entry) = reg.get(&tunnel_id) else {
        warn!(%tunnel_id, "ws frontend selected unknown tunnel");
        return (StatusCode::NOT_FOUND, "tunnel not found").into_response();
    };
    let handle = Arc::clone(&entry.handle);
    drop(entry);
    ws.on_upgrade(move |socket| serve_socket(socket, tunnel_id, handle))
}

async fn serve_socket(socket: WebSocket, tunnel_id: String, handle: Arc<crate::TunnelHandle>) {
    let (session_id, mut inbox_rx) = handle.open_session(SESSION_INBOX_QUEUE);
    debug!(%tunnel_id, session_id, "ws frontend session opened");
    if handle
        .send(Frame::OpenSession {
            session_id,
            client_info: None,
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let writer_handle = Arc::clone(&handle);

    // agent → ws client.
    let writer_task = tokio::spawn(async move {
        while let Some(bytes) = inbox_rx.recv().await {
            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
        let _ = writer_handle
            .send(Frame::CloseSession {
                session_id,
                reason: CloseReason::ClientGone,
            })
            .await;
        writer_handle.close_session(session_id);
    });

    // ws client → agent.
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Binary(payload)) => {
                if handle
                    .send(Frame::Data {
                        session_id,
                        payload: payload.to_vec(),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Ok(Message::Text(_text)) => {
                // The wire protocol is binary-only; drop text frames silently
                // rather than tearing down the session.
                debug!(%tunnel_id, session_id, "ignoring text ws frame");
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) => {
                debug!(%tunnel_id, session_id, error = %e, "ws frontend read error");
                break;
            }
        }
    }
    let _ = handle
        .send(Frame::CloseSession {
            session_id,
            reason: CloseReason::ClientGone,
        })
        .await;
    handle.close_session(session_id);
    drop(handle);
    let _ = writer_task.await;
    info!(%tunnel_id, session_id, "ws frontend session closed");
}

/// Bind the WS frontend router on `bind` and serve forever.
pub async fn serve(
    bind: std::net::SocketAddr,
    registry: Arc<TunnelRegistry>,
) -> std::io::Result<(std::net::SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind(bind).await?;
    let addr = listener.local_addr()?;
    info!(%addr, "tunnel relay ws frontend listening");
    let app = router(registry);
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            warn!(error = %e, "ws frontend server exited");
        }
    });
    Ok((addr, task))
}
