//! `WebSocketHostRpcClient` — `HostRpcClient` over JSON-RPC 2.0 WebSocket.
//!
//! This transport is for bridge-style DCC plug-ins that cannot host
//! Python directly. The immediate target is Photoshop UXP: the
//! `dcc-mcp-server` sidecar owns the MCP/gateway/admin/skills surface,
//! while the UXP plug-in owns only host API execution and replies over
//! a WebSocket JSON-RPC channel.
//!
//! # Wire format
//!
//! Request:
//!
//! ```json
//! {"jsonrpc":"2.0","id":"req-1","method":"dispatch",
//!  "params":{"action":"photoshop_layers__list",
//!            "args":{},"request_id":"req-1"}}
//! ```
//!
//! Successful response:
//!
//! ```json
//! {"jsonrpc":"2.0","id":"req-1","result":{"success":true}}
//! ```
//!
//! Error response:
//!
//! ```json
//! {"jsonrpc":"2.0","id":"req-1",
//!  "error":{"code":-32000,"message":"Photoshop API failed","data":{}}}
//! ```
//!
//! The client is deliberately small and DCC-agnostic. It does not know
//! Photoshop-specific JS APIs; it only speaks the shared `dispatch`
//! contract that bridge plug-ins can implement.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::{HostRpcClient, HostRpcError};

/// URI scheme for plain WebSocket bridge endpoints.
pub const WS_SCHEME: &str = "ws";

/// URI scheme for TLS WebSocket bridge endpoints.
pub const WSS_SCHEME: &str = "wss";

/// JSON-RPC method the host plug-in must expose.
pub const DISPATCH_METHOD: &str = "dispatch";

/// `HostRpcClient` over JSON-RPC 2.0 WebSocket.
#[derive(Debug)]
pub struct WebSocketHostRpcClient {
    scheme: &'static str,
    state: Arc<Mutex<Option<Connection>>>,
}

#[derive(Debug)]
struct Connection {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

#[derive(Serialize)]
struct WireRequest<'a> {
    jsonrpc: &'static str,
    id: &'a str,
    method: &'a str,
    params: Value,
}

impl WebSocketHostRpcClient {
    /// Construct a disconnected `ws://` client.
    #[must_use]
    pub fn new() -> Self {
        Self::with_scheme(WS_SCHEME)
    }

    /// Construct a disconnected client for a specific WebSocket scheme.
    ///
    /// Use this for registry dispatch so `uri_scheme()` reports the
    /// actual endpoint family (`ws` or `wss`).
    #[must_use]
    pub fn with_scheme(scheme: &'static str) -> Self {
        Self {
            scheme,
            state: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for WebSocketHostRpcClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HostRpcClient for WebSocketHostRpcClient {
    fn uri_scheme(&self) -> &'static str {
        self.scheme
    }

    async fn connect(&mut self, endpoint: &str, timeout: Duration) -> Result<(), HostRpcError> {
        parse_endpoint(endpoint, self.scheme)?;
        let (stream, _) = tokio::time::timeout(timeout, connect_async(endpoint))
            .await
            .map_err(|_| HostRpcError::Timeout {})?
            .map_err(|e| HostRpcError::transport(format!("websocket connect {endpoint}: {e}")))?;
        *self.state.lock().await = Some(Connection { stream });
        Ok(())
    }

    async fn call(
        &self,
        action: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value, HostRpcError> {
        let request = WireRequest {
            jsonrpc: "2.0",
            id: request_id,
            method: DISPATCH_METHOD,
            params: json!({
                "action": action,
                "args": args,
                "request_id": request_id,
            }),
        };
        let text = serde_json::to_string(&request)
            .map_err(|e| HostRpcError::transport(format!("encode websocket frame: {e}")))?;

        let mut guard = self.state.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            HostRpcError::transport("WebSocketHostRpcClient::call before connect")
        })?;

        if let Err(_e) = conn.stream.send(Message::Text(text.into())).await {
            *guard = None;
            return Err(HostRpcError::host_died(action, Some(args)));
        }

        loop {
            let Some(message) = conn.stream.next().await else {
                *guard = None;
                return Err(HostRpcError::host_died(action, Some(args)));
            };
            let message = match message {
                Ok(m) => m,
                Err(e) => {
                    *guard = None;
                    return Err(HostRpcError::transport(format!("websocket read: {e}")));
                }
            };

            match message {
                Message::Text(text) => {
                    let envelope: Value = serde_json::from_str(&text).map_err(|e| {
                        HostRpcError::transport(format!(
                            "websocket returned non-JSON text ({e}); raw: {text:?}",
                        ))
                    })?;
                    return interpret_envelope(envelope, action, request_id);
                }
                Message::Binary(bytes) => {
                    let envelope: Value = serde_json::from_slice(&bytes).map_err(|e| {
                        HostRpcError::transport(format!(
                            "websocket returned non-JSON binary frame ({e})",
                        ))
                    })?;
                    return interpret_envelope(envelope, action, request_id);
                }
                Message::Ping(payload) => {
                    if let Err(e) = conn.stream.send(Message::Pong(payload)).await {
                        *guard = None;
                        return Err(HostRpcError::transport(format!("websocket pong: {e}")));
                    }
                }
                Message::Pong(_) => {}
                Message::Close(_) => {
                    *guard = None;
                    return Err(HostRpcError::host_died(action, Some(args)));
                }
                Message::Frame(_) => {}
            }
        }
    }

    fn is_alive(&self) -> bool {
        match self.state.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(_) => true,
        }
    }

    async fn close(&self) {
        let mut guard = self.state.lock().await;
        if let Some(mut conn) = guard.take() {
            let _ = conn.stream.close(None).await;
        }
    }
}

/// Validate a WebSocket endpoint URI enough for actionable diagnostics.
pub fn parse_endpoint(endpoint: &str, expected_scheme: &str) -> Result<(), HostRpcError> {
    let prefix = format!("{expected_scheme}://");
    if !endpoint
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        return Err(HostRpcError::transport(format!(
            "expected {expected_scheme}:// URI, got {endpoint:?}"
        )));
    }
    let rest = &endpoint[prefix.len()..];
    if rest.is_empty() {
        return Err(HostRpcError::transport(format!(
            "websocket URI missing host — got {endpoint:?}"
        )));
    }
    Ok(())
}

fn interpret_envelope(
    envelope: Value,
    action: &str,
    request_id: &str,
) -> Result<Value, HostRpcError> {
    if let Some(id) = envelope.get("id")
        && id != request_id
    {
        return Err(HostRpcError::transport(format!(
            "websocket response id mismatch for {action}: expected {request_id:?}, got {id}",
        )));
    }
    if let Some(result) = envelope.get("result").cloned() {
        return Ok(result);
    }
    if let Some(error) = envelope.get("error").cloned() {
        return Err(HostRpcError::backend(json!({
            "action": action,
            "error": error,
        })));
    }
    Err(HostRpcError::transport(format!(
        "websocket returned envelope without `result` or `error` for {action}: {envelope}",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    async fn spawn_fake_ws_server(response: Value) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let request = ws.next().await.unwrap().unwrap();
            let text = request.into_text().unwrap();
            let body: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(body["jsonrpc"], "2.0");
            assert_eq!(body["method"], DISPATCH_METHOD);
            assert_eq!(body["params"]["action"], "photoshop_layers__list");
            ws.send(Message::Text(response.to_string().into()))
                .await
                .unwrap();
        });
        format!("ws://{addr}")
    }

    #[tokio::test]
    async fn websocket_dispatch_roundtrip_returns_result() {
        let endpoint = spawn_fake_ws_server(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "result": {"success": true, "layers": []}
        }))
        .await;
        let mut client = WebSocketHostRpcClient::new();
        client
            .connect(&endpoint, Duration::from_secs(2))
            .await
            .unwrap();

        let result = client
            .call("photoshop_layers__list", json!({}), "req-1")
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["layers"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn websocket_jsonrpc_error_maps_to_backend_error() {
        let endpoint = spawn_fake_ws_server(json!({
            "jsonrpc": "2.0",
            "id": "req-1",
            "error": {"code": -32000, "message": "Photoshop failed"}
        }))
        .await;
        let mut client = WebSocketHostRpcClient::new();
        client
            .connect(&endpoint, Duration::from_secs(2))
            .await
            .unwrap();

        let err = client
            .call("photoshop_layers__list", json!({}), "req-1")
            .await
            .unwrap_err();
        match err {
            HostRpcError::BackendError { envelope } => {
                assert_eq!(envelope["action"], "photoshop_layers__list");
                assert_eq!(envelope["error"]["message"], "Photoshop failed");
            }
            other => panic!("expected BackendError, got {other:?}"),
        }
    }

    #[test]
    fn parse_endpoint_accepts_ws_and_rejects_wrong_scheme() {
        parse_endpoint("ws://127.0.0.1:9001", WS_SCHEME).unwrap();
        let err = parse_endpoint("http://127.0.0.1:9001", WS_SCHEME).unwrap_err();
        assert!(matches!(err, HostRpcError::TransportError { .. }));
    }
}
