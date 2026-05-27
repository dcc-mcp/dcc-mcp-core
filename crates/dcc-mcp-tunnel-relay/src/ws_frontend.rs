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
//! - `/tunnel/{id}/...` — proxy an HTTP request to the tunneled backend.
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
use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{
        Path, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use futures_util::{SinkExt, StreamExt, stream};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

use dcc_mcp_tunnel_protocol::{CloseReason, Frame};

use crate::control::SESSION_INBOX_QUEUE;
use crate::registry::TunnelRegistry;

const HTTP_PROXY_HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_PROXY_BODY_TIMEOUT: Duration = Duration::from_secs(60);
const HTTP_PROXY_MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;
const HTTP_PROXY_MAX_RESPONSE_HEADER: usize = 64 * 1024;

/// Build the WebSocket frontend router. Exposed for tests so they can
/// run it inside an in-process `axum::serve` without binding a real port.
pub fn router(registry: Arc<TunnelRegistry>) -> Router {
    Router::new()
        .route("/tunnel/{id}", get(upgrade))
        .route("/tunnel/{id}/{*path}", any(proxy_http))
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

async fn proxy_http(
    Path((tunnel_id, path)): Path<(String, String)>,
    State(reg): State<Arc<TunnelRegistry>>,
    req: Request<Body>,
) -> Response {
    match proxy_http_inner(tunnel_id, path, reg, req).await {
        Ok(resp) => resp,
        Err(err) => {
            warn!(error = %err, "relay HTTP proxy failed");
            (StatusCode::BAD_GATEWAY, err).into_response()
        }
    }
}

async fn proxy_http_inner(
    tunnel_id: String,
    path: String,
    reg: Arc<TunnelRegistry>,
    req: Request<Body>,
) -> Result<Response, String> {
    let Some(entry) = reg.get(&tunnel_id) else {
        warn!(%tunnel_id, "http proxy selected unknown tunnel");
        return Err("tunnel not found".to_string());
    };
    let handle = Arc::clone(&entry.handle);
    drop(entry);

    let (parts, body) = req.into_parts();
    let body = to_bytes(body, HTTP_PROXY_MAX_REQUEST_BODY)
        .await
        .map_err(|err| format!("reading request body: {err}"))?;
    let request_bytes = encode_http_request(
        &parts.method,
        &path,
        parts.uri.query(),
        &parts.headers,
        &body,
    );

    let (session_id, mut inbox_rx) = handle.open_session(SESSION_INBOX_QUEUE);
    debug!(%tunnel_id, session_id, "http proxy session opened");
    if handle
        .send(Frame::OpenSession {
            session_id,
            client_info: Some("relay-http-proxy".to_string()),
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return Err("tunnel writer closed".to_string());
    }
    if handle
        .send(Frame::Data {
            session_id,
            payload: request_bytes,
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return Err("tunnel writer closed".to_string());
    }

    let mut response_buffer = Vec::new();
    let header_end = loop {
        let chunk = tokio::time::timeout(HTTP_PROXY_HEADER_TIMEOUT, inbox_rx.recv())
            .await
            .map_err(|_| "timed out waiting for backend response headers".to_string())?
            .ok_or_else(|| "backend closed before response headers".to_string())?;
        response_buffer.extend_from_slice(&chunk);
        if response_buffer.len() > HTTP_PROXY_MAX_RESPONSE_HEADER {
            handle.close_session(session_id);
            return Err("backend response headers exceeded proxy limit".to_string());
        }
        if let Some(end) = find_header_end(&response_buffer) {
            break end;
        }
    };

    let (status, headers) = parse_response_head(&response_buffer[..header_end])?;
    let initial_body = Bytes::copy_from_slice(&response_buffer[header_end..]);
    let guard = SessionGuard { handle, session_id };
    let body_stream = stream::once(async move { Ok::<Bytes, std::io::Error>(initial_body) }).chain(
        stream::unfold((inbox_rx, guard), |(mut inbox_rx, guard)| async move {
            match tokio::time::timeout(HTTP_PROXY_BODY_TIMEOUT, inbox_rx.recv()).await {
                Ok(Some(bytes)) => Some((Ok(Bytes::from(bytes)), (inbox_rx, guard))),
                Ok(None) => None,
                Err(_) => Some((
                    Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out waiting for backend response body",
                    )),
                    (inbox_rx, guard),
                )),
            }
        }),
    );

    let mut builder = Response::builder().status(status);
    {
        let out_headers = builder
            .headers_mut()
            .ok_or_else(|| "failed to build response headers".to_string())?;
        for (name, value) in headers {
            out_headers.append(name, value);
        }
    }
    builder
        .body(Body::from_stream(body_stream))
        .map_err(|err| format!("building proxy response: {err}"))
}

fn encode_http_request(
    method: &axum::http::Method,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: &[u8],
) -> Vec<u8> {
    let mut target = format!("/{}", path.trim_start_matches('/'));
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        target.push('?');
        target.push_str(query);
    }
    let mut raw =
        format!("{method} {target} HTTP/1.1\r\nHost: dcc-mcp-tunnel-backend\r\n").into_bytes();
    for (name, value) in headers {
        if is_hop_by_hop_header(name) || name == axum::http::header::HOST {
            continue;
        }
        raw.extend_from_slice(name.as_str().as_bytes());
        raw.extend_from_slice(b": ");
        raw.extend_from_slice(value.as_bytes());
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    );
    raw.extend_from_slice(body);
    raw
}

fn parse_response_head(
    head: &[u8],
) -> Result<(StatusCode, Vec<(HeaderName, HeaderValue)>), String> {
    let text = std::str::from_utf8(head)
        .map_err(|err| format!("backend response head is not UTF-8: {err}"))?;
    let mut lines = text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "backend response missing status line".to_string())?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("backend response has invalid status line: {status_line}"))?
        .parse::<u16>()
        .map_err(|err| format!("backend response has invalid status code: {err}"))
        .and_then(|code| StatusCode::from_u16(code).map_err(|err| err.to_string()))?;

    let mut headers = Vec::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|err| format!("backend response header name invalid: {err}"))?;
        if is_hop_by_hop_header(&name) {
            continue;
        }
        let value = HeaderValue::from_str(value.trim())
            .map_err(|err| format!("backend response header value invalid: {err}"))?;
        headers.push((name, value));
    }
    Ok((status, headers))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|pos| pos + 4)
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

struct SessionGuard {
    handle: Arc<crate::TunnelHandle>,
    session_id: dcc_mcp_tunnel_protocol::SessionId,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        let _ = self.handle.try_send(Frame::CloseSession {
            session_id: self.session_id,
            reason: CloseReason::ClientGone,
        });
        self.handle.close_session(self.session_id);
    }
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
