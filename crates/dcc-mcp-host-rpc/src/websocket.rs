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
use dcc_mcp_protocols::bridge::{
    BRIDGE_PROTOCOL_VERSION, BridgeEvent, BridgeHeartbeat, BridgeHello, BridgeHelloAck,
    BridgeMessage,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast};
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
    hello_ack: Arc<Mutex<Option<BridgeHelloAck>>>,
    events: broadcast::Sender<BridgeEvent>,
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
            hello_ack: Arc::new(Mutex::new(None)),
            events: broadcast::channel(64).0,
        }
    }

    /// Subscribe to asynchronous events emitted by the host plugin.
    ///
    /// Event frames are consumed while a dispatch call is waiting for its
    /// response and are made available to the adapter through this channel.
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<BridgeEvent> {
        self.events.subscribe()
    }

    /// Return the most recent server handshake acknowledgement.
    pub async fn hello_ack(&self) -> Option<BridgeHelloAck> {
        self.hello_ack.lock().await.clone()
    }

    /// Negotiate the structured plugin bridge protocol after connecting.
    ///
    /// This method is opt-in so existing plugins that only implement the
    /// historical `dispatch` contract keep working unchanged. New plugins
    /// such as the Roblox Studio adapter should call it immediately after
    /// [`HostRpcClient::connect`].
    pub async fn handshake(
        &self,
        mut hello: BridgeHello,
        timeout: Duration,
    ) -> Result<BridgeHelloAck, HostRpcError> {
        if hello.protocol_version.is_none() {
            hello.protocol_version = Some(BRIDGE_PROTOCOL_VERSION.to_string());
        }
        let text = serde_json::to_string(&BridgeMessage::Hello(hello))
            .map_err(|e| HostRpcError::transport(format!("encode websocket hello: {e}")))?;
        let mut guard = self.state.lock().await;
        let conn = guard.as_mut().ok_or_else(|| {
            HostRpcError::transport("WebSocketHostRpcClient::handshake before connect")
        })?;
        conn.stream
            .send(Message::Text(text.into()))
            .await
            .map_err(|e| HostRpcError::transport(format!("websocket hello: {e}")))?;

        let ack = tokio::time::timeout(timeout, async {
            loop {
                let Some(message) = conn.stream.next().await else {
                    return Err(HostRpcError::host_died_idle());
                };
                let message = message.map_err(|e| {
                    HostRpcError::transport(format!("websocket handshake read: {e}"))
                })?;
                match message {
                    Message::Text(text) => {
                        let parsed: BridgeMessage = serde_json::from_str(&text).map_err(|e| {
                            HostRpcError::transport(format!(
                                "websocket handshake returned invalid bridge message: {e}"
                            ))
                        })?;
                        match parsed {
                            BridgeMessage::HelloAck(ack) => break Ok(ack),
                            BridgeMessage::Event(event) => {
                                let _ = self.events.send(event);
                            }
                            BridgeMessage::Heartbeat(BridgeHeartbeat { .. }) => {}
                            other => {
                                break Err(HostRpcError::transport(format!(
                                    "unexpected websocket handshake message: {other:?}"
                                )));
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        conn.stream
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|e| {
                                HostRpcError::transport(format!("websocket handshake pong: {e}"))
                            })?;
                    }
                    Message::Close(_) => break Err(HostRpcError::host_died_idle()),
                    Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
                }
            }
        })
        .await
        .map_err(|_| HostRpcError::Timeout {})??;

        *self.hello_ack.lock().await = Some(ack.clone());
        Ok(ack)
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
        *self.hello_ack.lock().await = None;
        *self.state.lock().await = Some(Connection { stream });
        Ok(())
    }

    async fn call(
        &self,
        action: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value, HostRpcError> {
        self.call_with_trace_context(action, args, request_id, None)
            .await
    }

    async fn call_with_trace_context(
        &self,
        action: &str,
        args: Value,
        request_id: &str,
        trace_context: Option<Value>,
    ) -> Result<Value, HostRpcError> {
        let mut params = json!({
            "action": action,
            "args": args,
            "request_id": request_id,
        });
        if let Some(trace_context) = trace_context {
            params["trace_context"] = trace_context;
        }
        let request = WireRequest {
            jsonrpc: "2.0",
            id: request_id,
            method: DISPATCH_METHOD,
            params,
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
                    if consume_async_bridge_message(&envelope, &self.events) {
                        continue;
                    }
                    return interpret_envelope(envelope, action, request_id);
                }
                Message::Binary(bytes) => {
                    let envelope: Value = serde_json::from_slice(&bytes).map_err(|e| {
                        HostRpcError::transport(format!(
                            "websocket returned non-JSON binary frame ({e})",
                        ))
                    })?;
                    if consume_async_bridge_message(&envelope, &self.events) {
                        continue;
                    }
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
    let expected_id = Value::String(request_id.to_string());
    if envelope.get("id") != Some(&expected_id) {
        return Err(HostRpcError::transport(format!(
            "websocket transport desync for {action}: expected response id {request_id:?}, got {}",
            envelope
                .get("id")
                .map_or_else(|| "<missing>".to_string(), Value::to_string),
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

/// Consume lifecycle frames that may arrive while a dispatch response is in
/// flight. Returning `true` keeps the caller waiting for its matching result.
fn consume_async_bridge_message(envelope: &Value, events: &broadcast::Sender<BridgeEvent>) -> bool {
    match envelope.get("type").and_then(Value::as_str) {
        Some("event") => {
            if let Ok(BridgeMessage::Event(event)) =
                serde_json::from_value::<BridgeMessage>(envelope.clone())
            {
                let _ = events.send(event);
            }
            true
        }
        Some("heartbeat") | Some("ready") | Some("hello_ack") => true,
        _ => false,
    }
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

    #[tokio::test]
    async fn websocket_v1_handshake_stores_instance_metadata() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();
            let request = ws.next().await.unwrap().unwrap();
            let message: BridgeMessage =
                serde_json::from_str(&request.into_text().unwrap()).unwrap();
            assert!(matches!(message, BridgeMessage::Hello(_)));
            ws.send(Message::Text(
                serde_json::to_string(&BridgeMessage::HelloAck(BridgeHelloAck {
                    protocol_version: Some(BRIDGE_PROTOCOL_VERSION.to_string()),
                    server: "roblox-studio-mcp".to_string(),
                    version: "0.1.0".to_string(),
                    session_id: "session-1".to_string(),
                    instance_id: Some("studio-1".to_string()),
                    peer_id: Some("plugin".to_string()),
                    capabilities: serde_json::Map::new(),
                    ready: true,
                    metadata: serde_json::Map::new(),
                }))
                .unwrap()
                .into(),
            ))
            .await
            .unwrap();
        });

        let mut client = WebSocketHostRpcClient::new();
        client
            .connect(&format!("ws://{addr}"), Duration::from_secs(2))
            .await
            .unwrap();
        let ack = client
            .handshake(
                BridgeHello {
                    protocol_version: None,
                    client: "roblox-studio-plugin".to_string(),
                    version: "0.1.0".to_string(),
                    dcc_version: Some("0.632".to_string()),
                    instance_id: Some("studio-1".to_string()),
                    peer_id: Some("plugin".to_string()),
                    host_name: Some("Roblox Studio".to_string()),
                    host_version: Some("0.632".to_string()),
                    document: Some("place-1".to_string()),
                    project: Some("experience-1".to_string()),
                    data_model: Some("DataModel".to_string()),
                    capabilities: serde_json::Map::new(),
                    metadata: serde_json::Map::new(),
                },
                Duration::from_secs(2),
            )
            .await
            .unwrap();
        assert_eq!(ack.instance_id.as_deref(), Some("studio-1"));
        assert_eq!(client.hello_ack().await.unwrap().ready, true);
    }

    #[tokio::test]
    async fn websocket_dispatch_skips_event_frames() {
        let endpoint = {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let addr = listener.local_addr().unwrap();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws = accept_async(stream).await.unwrap();
                let _ = ws.next().await.unwrap().unwrap();
                ws.send(Message::Text(
                    serde_json::to_string(&BridgeMessage::Event(BridgeEvent {
                        event: "studio.changed".to_string(),
                        data: Some(json!({"place": "place-1"})),
                    }))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
                ws.send(Message::Text(
                    json!({
                        "jsonrpc": "2.0",
                        "id": "req-1",
                        "result": {"success": true}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            });
            format!("ws://{addr}")
        };

        let mut client = WebSocketHostRpcClient::new();
        client
            .connect(&endpoint, Duration::from_secs(2))
            .await
            .unwrap();
        let mut events = client.subscribe_events();
        let result = client
            .call("roblox.get_studio_state", json!({}), "req-1")
            .await
            .unwrap();
        assert_eq!(result["success"], true);
        let event = events.recv().await.unwrap();
        assert_eq!(event.event, "studio.changed");
    }

    #[test]
    fn websocket_response_without_id_is_a_transport_desync() {
        let error = interpret_envelope(
            json!({"jsonrpc": "2.0", "result": {"success": true}}),
            "photoshop_layers__list",
            "req-current",
        )
        .unwrap_err();

        match error {
            HostRpcError::TransportError { message } => {
                assert!(message.contains("transport desync"), "{message}");
                assert!(message.contains("req-current"), "{message}");
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn parse_endpoint_accepts_ws_and_rejects_wrong_scheme() {
        parse_endpoint("ws://127.0.0.1:9001", WS_SCHEME).unwrap();
        let err = parse_endpoint("http://127.0.0.1:9001", WS_SCHEME).unwrap_err();
        assert!(matches!(err, HostRpcError::TransportError { .. }));
    }
}
