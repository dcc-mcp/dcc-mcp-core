//! HTTP/WebSocket frontend transport.
//!
//! Accepts plain HTTP/1.1 WebSocket upgrades — TLS is the operator's
//! reverse proxy concern (`nginx` / `caddy` / cloud LB), exactly the same
//! split the rest of the project uses. The relay is run behind a TLS
//! terminator on the public internet and listens on a private port here.
//!
//! ## Routing
//!
//! - `GET /tunnel/{id}` with `Upgrade: websocket` — connect to the tunnel
//!   with that id.
//! - `/tunnel/{id}/{*path}` — proxy one HTTP request through the tunnel to
//!   the local backend path.
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
//! - HTTP requests are serialized as HTTP/1.1 bytes, then response bytes are
//!   streamed back as an axum body.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use futures_util::{SinkExt, Stream, StreamExt};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

use dcc_mcp_tunnel_protocol::{CloseReason, Frame, SessionId};

use crate::control::SESSION_INBOX_QUEUE;
use crate::registry::TunnelRegistry;

const HTTP_PROXY_HEADER_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HTTP_RESPONSE_HEADER_BYTES: usize = 64 * 1024;

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
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path((tunnel_id, _path)): Path<(String, String)>,
    State(reg): State<Arc<TunnelRegistry>>,
    body: Bytes,
) -> Response {
    let Some(entry) = reg.get(&tunnel_id) else {
        warn!(%tunnel_id, "http frontend selected unknown tunnel");
        return (StatusCode::NOT_FOUND, "tunnel not found").into_response();
    };
    let handle = Arc::clone(&entry.handle);
    drop(entry);

    match proxy_http_once(handle, tunnel_id.clone(), method, uri, headers, body).await {
        Ok(response) => response,
        Err((status, message)) => {
            warn!(%tunnel_id, status = status.as_u16(), %message, "http tunnel proxy failed");
            (status, message).into_response()
        }
    }
}

async fn proxy_http_once(
    handle: Arc<crate::TunnelHandle>,
    tunnel_id: String,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let (session_id, mut inbox_rx) = handle.open_session(SESSION_INBOX_QUEUE);
    debug!(%tunnel_id, session_id, "http frontend session opened");

    if handle
        .send(Frame::OpenSession {
            session_id,
            client_info: None,
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return Err((
            StatusCode::BAD_GATEWAY,
            "tunnel agent disconnected".to_string(),
        ));
    }

    let request_bytes = build_http_request(&tunnel_id, &method, &uri, &headers, &body)?;
    if handle
        .send(Frame::Data {
            session_id,
            payload: request_bytes,
        })
        .await
        .is_err()
    {
        handle.close_session(session_id);
        return Err((
            StatusCode::BAD_GATEWAY,
            "tunnel agent disconnected while sending request".to_string(),
        ));
    }

    // Half-close the request side after the complete HTTP request is queued.
    // The agent forwards this as a TCP write-half shutdown to the local MCP
    // server while continuing to stream the response back through this session.
    let _ = handle
        .send(Frame::CloseSession {
            session_id,
            reason: CloseReason::ClientGone,
        })
        .await;

    let head = read_http_response_head(&mut inbox_rx).await?;
    let body_stream = TunnelBodyStream {
        first: Some(Bytes::from(head.initial_body)),
        rx: inbox_rx,
        handle,
        session_id,
    };

    let mut builder = Response::builder().status(head.status);
    for (name, value) in head.headers {
        builder = builder.header(name, value);
    }
    builder.body(Body::from_stream(body_stream)).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("response build failed: {e}"),
        )
    })
}

struct TunnelBodyStream {
    first: Option<Bytes>,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    handle: Arc<crate::TunnelHandle>,
    session_id: SessionId,
}

impl Stream for TunnelBodyStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first) = self.first.take().filter(|bytes| !bytes.is_empty()) {
            return Poll::Ready(Some(Ok(first)));
        }
        match Pin::new(&mut self.rx).poll_recv(cx) {
            Poll::Ready(Some(bytes)) => Poll::Ready(Some(Ok(Bytes::from(bytes)))),
            Poll::Ready(None) => {
                self.handle.close_session(self.session_id);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for TunnelBodyStream {
    fn drop(&mut self) {
        let _ = self.handle.try_send(Frame::CloseSession {
            session_id: self.session_id,
            reason: CloseReason::ClientGone,
        });
        self.handle.close_session(self.session_id);
    }
}

struct HttpResponseHead {
    status: StatusCode,
    headers: Vec<(String, String)>,
    initial_body: Vec<u8>,
}

async fn read_http_response_head(
    inbox_rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
) -> Result<HttpResponseHead, (StatusCode, String)> {
    let mut buf = Vec::with_capacity(4096);
    loop {
        let next = tokio::time::timeout(HTTP_PROXY_HEADER_TIMEOUT, inbox_rx.recv())
            .await
            .map_err(|_| {
                (
                    StatusCode::BAD_GATEWAY,
                    "timed out waiting for backend response headers".to_string(),
                )
            })?
            .ok_or_else(|| {
                (
                    StatusCode::BAD_GATEWAY,
                    "backend closed before response headers".to_string(),
                )
            })?;
        buf.extend_from_slice(&next);
        if buf.len() > MAX_HTTP_RESPONSE_HEADER_BYTES {
            return Err((
                StatusCode::BAD_GATEWAY,
                "backend response headers exceeded limit".to_string(),
            ));
        }
        if let Some(header_end) = find_header_end(&buf) {
            let header_bytes = &buf[..header_end];
            let initial_body = buf[header_end + 4..].to_vec();
            return parse_http_response_head(header_bytes, initial_body);
        }
    }
}

fn parse_http_response_head(
    header_bytes: &[u8],
    initial_body: Vec<u8>,
) -> Result<HttpResponseHead, (StatusCode, String)> {
    let header_text = std::str::from_utf8(header_bytes).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("backend response headers were not UTF-8: {e}"),
        )
    })?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                format!("invalid backend status line: {status_line}"),
            )
        })?;

    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .filter_map(|(name, value)| {
            let name = name.trim().to_ascii_lowercase();
            if !forward_response_header(&name) {
                return None;
            }
            Some((name, value.trim().to_string()))
        })
        .collect();

    Ok(HttpResponseHead {
        status: status_code,
        headers,
        initial_body,
    })
}

fn build_http_request(
    tunnel_id: &str,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let path = upstream_path_from_uri(tunnel_id, uri);
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(method.as_str().as_bytes());
    out.extend_from_slice(b" ");
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");
    out.extend_from_slice(b"Host: dcc-mcp-tunnel-backend\r\n");
    out.extend_from_slice(b"Connection: close\r\n");
    out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());

    for (name, value) in headers {
        let lower = name.as_str().to_ascii_lowercase();
        if !forward_request_header(&lower) {
            continue;
        }
        let value = value.to_str().map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("request header {name} was not valid text: {e}"),
            )
        })?;
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    Ok(out)
}

fn upstream_path_from_uri(tunnel_id: &str, uri: &Uri) -> String {
    let raw = uri
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or_else(|| uri.path());
    let prefix = format!("/tunnel/{tunnel_id}");
    match raw.strip_prefix(&prefix) {
        Some("") => "/".to_string(),
        Some(rest) => rest.to_string(),
        None => raw.to_string(),
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn forward_request_header(name: &str) -> bool {
    !matches!(
        name,
        "host"
            | "content-length"
            | "connection"
            | "transfer-encoding"
            | "upgrade"
            | "sec-websocket-key"
            | "sec-websocket-version"
            | "sec-websocket-protocol"
            | "sec-websocket-extensions"
    )
}

fn forward_response_header(name: &str) -> bool {
    !matches!(
        name,
        "content-length" | "connection" | "transfer-encoding" | "upgrade"
    )
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
