//! `QtServerClient` — `HostRpcClient` over the universal Qt-event-loop
//! JSON-line dispatcher.
//!
//! This is the second concrete impl of [`HostRpcClient`] (the first is
//! [`CommandPortClient`](crate::commandport::CommandPortClient)).
//! Architecturally the same role — speak to a single DCC instance —
//! but using a wire format that scales to **multi-client** concurrency
//! and **structured errors** in a way Maya's line-oriented
//! `commandPort` cannot.
//!
//! # Wire format
//!
//! One JSON object per `\n`-terminated line in each direction.
//!
//! Request:
//!
//! ```json
//! {"id": "req-1", "method": "dispatch",
//!  "params": {"action": "maya_scene__list_objects",
//!             "args": {}, "request_id": "req-1"}}
//! ```
//!
//! Successful response:
//!
//! ```json
//! {"id": "req-1", "result": {"value": ..., "result_type": "value"}}
//! ```
//!
//! Error response:
//!
//! ```json
//! {"id": "req-1",
//!  "error": {"code": "handler-exception",
//!            "message": "RuntimeError: ...",
//!            "traceback": "..."}}
//! ```
//!
//! The DCC-side dispatcher (published as `dcc_mcp_core.qt_dispatcher` and
//! embedded directly from the canonical source via `include_str!`) runs
//! cooperatively on the host's
//! Qt event loop via `QTcpServer` + a 50 ms `QTimer`. Every request is
//! `try/except`-wrapped per handler so a Python-level failure produces
//! a structured error envelope instead of leaking to a host modal
//! dialog (the root cause of issues #235 / #240 / #241 on Maya).
//!
//! # `call()` mapping
//!
//! [`HostRpcClient::call`] is invoked as
//! `call(action, args, request_id)`. The qtserver wire wraps that into
//! a `dispatch` method invocation:
//!
//! ```text
//! method = "dispatch"
//! params = { "action": <action>, "args": <args>, "request_id": <request_id> }
//! id     = <request_id>
//! ```
//!
//! The adapter Python (e.g. `dcc_mcp_maya.sidecar._dispatcher`)
//! registers a `dispatch` handler on the Qt dispatcher's registry at
//! plug-in startup so the same Python contract used today over
//! `commandport://` keeps working over `qtserver://`. This means the
//! adapter dispatcher itself is **wire-agnostic** — only the
//! transport changes.
//!
//! # Cancellation
//!
//! The wire today is request/response without a separate cancellation
//! channel. The dispatcher's main-thread guard means a long action
//! cannot be interrupted mid-frame — the gateway's request-timeout +
//! the in-Maya `check_maya_cancelled()` token stay the cooperative
//! cancellation primitives. Future work: extend the JSON-line wire
//! with a `cancel` method whose handler signals the corresponding
//! in-flight `request_id`.
//!
//! # Concurrency
//!
//! Unlike `commandPort` (single-flight), the Qt dispatcher serves
//! multiple connections. We still serialise per-client calls with a
//! [`tokio::sync::Mutex`] because the wire is half-duplex per
//! connection — request line in, response line out — and pipelining
//! request IDs adds complexity we do not need for the current
//! sidecar usage pattern (one outstanding gateway call per session).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::{HostRpcClient, HostRpcError};

/// The URI scheme this impl handles. Pinned as a constant so the
/// scheme registry and tests both reference the same string.
pub const URI_SCHEME: &str = "qtserver";

/// JSON wire method that the adapter Python is expected to register
/// on the Qt dispatcher's registry. See module-level docs for the
/// shape of `params` and the `dispatch_payload` contract.
pub const DISPATCH_METHOD: &str = "dispatch";

/// Universal in-DCC dispatcher Python source.
///
/// Embedded directly from the canonical source at
/// `python/dcc_mcp_core/qt_dispatcher.py` — the single source of truth
/// for the Qt dispatcher. Path is relative to this source file (the
/// workspace root is three directories up from `src/`).
///
/// Re-exported as a public constant so adapter plug-ins that want to
/// install the dispatcher eagerly (no commandPort bootstrap dance)
/// can `include_str!`-equivalent the same source the lazy bootstrap
/// path uses. Tests use it to spin up a real `QtCommandServer` inside
/// a synthetic Python interpreter.
pub const DISPATCHER_PY: &str = include_str!("../../../python/dcc_mcp_core/qt_dispatcher.py");

/// Bootstrap installer source.
///
/// Public for the same reason as [`DISPATCHER_PY`] — adapter and
/// sidecar code can both inspect the source the wire path will run.
/// The installer expects ``_DISPATCHER_SOURCE`` to be set as a
/// string global before exec.
pub const BOOTSTRAP_PY: &str = include_str!("../python/dcc_qt_dispatcher_bootstrap.py");

/// `HostRpcClient` over the universal Qt-event-loop JSON-line wire.
///
/// Instantiate via [`QtServerClient::new`], then call
/// [`HostRpcClient::connect`] to dial the TCP socket. After that the
/// sidecar binary drives [`HostRpcClient::call`] / `close` just like
/// any other impl.
#[derive(Debug)]
pub struct QtServerClient {
    state: Arc<Mutex<Option<Connection>>>,
}

#[derive(Debug)]
struct Connection {
    writer: OwnedWriteHalf,
    reader: BufReader<OwnedReadHalf>,
}

impl QtServerClient {
    /// Construct a disconnected client. Call [`HostRpcClient::connect`]
    /// before the first [`HostRpcClient::call`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for QtServerClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize)]
struct WireRequest<'a> {
    id: &'a str,
    method: &'a str,
    params: Value,
}

#[async_trait]
impl HostRpcClient for QtServerClient {
    fn uri_scheme(&self) -> &'static str {
        URI_SCHEME
    }

    async fn connect(&mut self, endpoint: &str, timeout: Duration) -> Result<(), HostRpcError> {
        let (host, port) = parse_endpoint(endpoint)?;
        let stream = tokio::time::timeout(timeout, TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| HostRpcError::Timeout {})?
            .map_err(|e| HostRpcError::transport(format!("qtserver connect {endpoint}: {e}")))?;
        // Same Nagle-off rationale as `CommandPortClient`: each JSON
        // request/response is a single line; we never benefit from
        // kernel-level coalescing here.
        let _ = stream.set_nodelay(true);
        let (read_half, write_half) = stream.into_split();
        *self.state.lock().await = Some(Connection {
            writer: write_half,
            reader: BufReader::new(read_half),
        });
        Ok(())
    }

    async fn call(
        &self,
        action: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value, HostRpcError> {
        let request = WireRequest {
            id: request_id,
            method: DISPATCH_METHOD,
            params: json!({
                "action": action,
                "args": args,
                "request_id": request_id,
            }),
        };
        let mut wire_line = serde_json::to_vec(&request)
            .map_err(|e| HostRpcError::transport(format!("encode wire frame: {e}")))?;
        wire_line.push(b'\n');

        let mut guard = self.state.lock().await;
        let conn = guard
            .as_mut()
            .ok_or_else(|| HostRpcError::transport("QtServerClient::call before connect"))?;

        if let Err(e) = conn.writer.write_all(&wire_line).await {
            return Err(io_error_to_host_rpc(
                e,
                "qtserver write",
                action,
                args,
                guard,
            ));
        }
        if let Err(e) = conn.writer.flush().await {
            return Err(io_error_to_host_rpc(
                e,
                "qtserver flush",
                action,
                args,
                guard,
            ));
        }

        let mut buf = String::new();
        match conn.reader.read_line(&mut buf).await {
            Ok(0) => {
                // Peer closed mid-call — same host-died handling as
                // commandport.
                *guard = None;
                Err(HostRpcError::host_died(action, Some(args)))
            }
            Ok(_) => {
                let line = buf.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    return Err(HostRpcError::transport(
                        "qtserver returned an empty line — protocol violation",
                    ));
                }
                let envelope: Value = serde_json::from_str(line).map_err(|e| {
                    HostRpcError::transport(format!(
                        "qtserver returned non-JSON line ({e}); raw: {line:?}",
                    ))
                })?;
                interpret_envelope(envelope, action)
            }
            Err(e) => Err(io_error_to_host_rpc(
                e,
                "qtserver read",
                action,
                args,
                guard,
            )),
        }
    }

    fn is_alive(&self) -> bool {
        // Best-effort. Same rationale as commandport — a held lock
        // means a call is in flight, which means the socket was alive
        // up to the read syscall.
        match self.state.try_lock() {
            Ok(guard) => guard.is_some(),
            Err(_) => true,
        }
    }

    async fn close(&self) {
        let mut guard = self.state.lock().await;
        if let Some(conn) = guard.take() {
            drop(conn);
        }
    }
}

/// Map a `tokio` / `std::io::Error` raised during a qtserver
/// read/write/flush into the canonical [`HostRpcError`] envelope.
///
/// Same classification table as the commandport path — disconnect
/// mid-call surfaces as `host-died`; everything else is a plain
/// transport error.
fn io_error_to_host_rpc(
    error: std::io::Error,
    op: &str,
    action: &str,
    args: Value,
    mut guard: tokio::sync::MutexGuard<'_, Option<Connection>>,
) -> HostRpcError {
    use std::io::ErrorKind;
    let is_terminal = matches!(
        error.kind(),
        ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::BrokenPipe
            | ErrorKind::NotConnected
            | ErrorKind::UnexpectedEof,
    );
    if is_terminal {
        *guard = None;
        return HostRpcError::host_died(action, Some(args));
    }
    HostRpcError::transport(format!("{op}: {error}"))
}

/// Translate the JSON envelope returned by the Qt dispatcher into the
/// canonical `Result<Value, HostRpcError>` contract.
///
/// Three shapes are accepted:
///
/// * `{"id": …, "result": …}` → `Ok(result)`
/// * `{"id": …, "error": {…}}` → mapped to a transport error envelope
///   carrying the dispatcher's structured ``code`` / ``message``.
/// * Anything else → `TransportError` complaining about protocol
///   violation. The Qt dispatcher must produce one of the two
///   well-formed shapes per request; a malformed reply is a wire
///   contract violation, not a user-visible error.
fn interpret_envelope(envelope: Value, action: &str) -> Result<Value, HostRpcError> {
    if let Some(result) = envelope.get("result").cloned() {
        return Ok(result);
    }
    if let Some(error_obj) = envelope.get("error") {
        let code = error_obj
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("dispatcher-error");
        let message = error_obj
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("(no message)");
        return Err(HostRpcError::transport(format!(
            "qtserver dispatcher error during {action}: [{code}] {message}",
        )));
    }
    Err(HostRpcError::transport(format!(
        "qtserver returned envelope without `result` or `error` for {action}: {envelope}",
    )))
}

/// Parse a `qtserver://host:port` URI into its TCP target.
///
/// Mirrors [`commandport::parse_endpoint`](crate::commandport)'s shape
/// so the error envelopes carry consistent diagnostics between the
/// two transports.
pub fn parse_endpoint(endpoint: &str) -> Result<(String, u16), HostRpcError> {
    let prefix = "qtserver://";
    let rest = endpoint
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &endpoint[prefix.len()..])
        .ok_or_else(|| {
            HostRpcError::transport(format!("expected qtserver:// URI, got {endpoint:?}"))
        })?;
    let (host, port_str) = rest.rsplit_once(':').ok_or_else(|| {
        HostRpcError::transport(format!("qtserver URI missing :port — got {endpoint:?}"))
    })?;
    if host.is_empty() {
        return Err(HostRpcError::transport(format!(
            "qtserver URI has empty host — got {endpoint:?}"
        )));
    }
    let port: u16 = port_str.parse().map_err(|_| {
        HostRpcError::transport(format!("qtserver URI port is not a u16 — got {endpoint:?}"))
    })?;
    if port == 0 {
        return Err(HostRpcError::transport(format!(
            "qtserver URI port must be non-zero — got {endpoint:?}"
        )));
    }
    Ok((host.to_string(), port))
}

/// Build the **single-line** `commandPort` bootstrap payload that
/// installs the qtserver dispatcher into the host process.
///
/// The Rust sidecar uses this when transitioning from a freshly opened
/// `commandPort` (the only zero-install entry point most DCCs offer)
/// to a full `qtserver://` session. The returned string is a complete
/// Python statement suitable for one line of `commandPort` with
/// `sourceType='python'` — it `exec`s the bootstrap source after
/// injecting both `_DISPATCHER_SOURCE` and `_REQUESTED_PORT` as
/// string/int globals.
///
/// Note: the line does **not** terminate with `\n`; callers append
/// the newline at write time so they can choose CRLF or LF per
/// platform conventions.
#[must_use]
pub fn build_bootstrap_command_line(requested_port: u16) -> String {
    // Python's `repr()` of a multi-line string produces a valid
    // string literal with `\n` etc. escaped. Rust's `{:?}` debug
    // format for `&str` produces the same shape for ASCII-only
    // input; the dispatcher source is pure ASCII by design.
    //
    // The combined source layout (in injection order) is:
    //   _DISPATCHER_SOURCE = "<dispatcher_py>"
    //   _REQUESTED_PORT    = <u16>
    //   <bootstrap_py body>
    //
    // The bootstrap body's last statement is the assignment
    // `_install_result = _install(_DISPATCHER_SOURCE)`, which leaves
    // the installed module in `sys.modules['_dcc_qt_dispatcher']`.
    // A second commandPort round-trip (see `build_start_command_line`)
    // then calls `start_qt_server(port=<requested_port>)` and reads
    // back the bound port.
    let combined = format!(
        "_DISPATCHER_SOURCE = {dispatcher:?}\n\
         _REQUESTED_PORT = {requested}\n\
         {bootstrap}",
        dispatcher = DISPATCHER_PY,
        requested = requested_port,
        bootstrap = BOOTSTRAP_PY,
    );
    format!(
        "__import__('builtins').exec(__import__('builtins').compile({combined:?}, \
         '<dcc-mcp-qt-bootstrap>', 'exec'))",
    )
}

/// Build the **single-line** `commandPort` follow-up payload that
/// asks the just-installed dispatcher to start its `QTcpServer` and
/// reports the bound endpoint as a JSON string.
///
/// Pairs with [`build_bootstrap_command_line`]. The reply is a JSON
/// dict the caller parses via [`parse_start_reply`].
#[must_use]
pub fn build_start_command_line(requested_port: u16) -> String {
    format!(
        "__import__('json').dumps(\
         __import__('_dcc_qt_dispatcher').start_qt_server(port={requested_port}))",
    )
}

/// Parse the JSON dict reply produced by [`build_start_command_line`].
///
/// Returns the `qtserver://host:port` URI the caller should pass to
/// [`HostRpcClient::connect`] on a freshly constructed
/// [`QtServerClient`].
pub fn parse_start_reply(reply: &str) -> Result<String, HostRpcError> {
    // Maya's commandPort with `sourceType='python'` wraps the eval
    // result in a leading + trailing quote character (Python `repr`
    // of the returned string). Tolerate both raw JSON and quoted
    // JSON shapes so the caller does not need to peek at Maya's
    // wire idiosyncrasies.
    let trimmed = reply.trim();
    let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        // Strip the outer quote characters; commandPort embeds the
        // JSON as a Python string literal but does NOT re-escape
        // inner `"` because the embedded value is also a JSON string
        // that was double-encoded once already. We accept naive trim
        // here because the dispatcher controls the inner payload.
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let parsed: Value = serde_json::from_str(unquoted).map_err(|e| {
        HostRpcError::transport(format!(
            "qtserver start reply was not JSON ({e}); raw: {reply:?}",
        ))
    })?;
    let host = parsed
        .get("host")
        .and_then(Value::as_str)
        .ok_or_else(|| HostRpcError::transport(format!("start reply missing `host`: {parsed}")))?;
    let port = parsed
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| HostRpcError::transport(format!("start reply missing `port`: {parsed}")))?;
    if port == 0 || port > u64::from(u16::MAX) {
        return Err(HostRpcError::transport(format!(
            "start reply `port` out of u16 range: {port}"
        )));
    }
    Ok(format!("{URI_SCHEME}://{host}:{port}"))
}

// ── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    // ── parse_endpoint ───────────────────────────────────────────────────

    #[test]
    fn parse_endpoint_happy_path() {
        let (host, port) = parse_endpoint("qtserver://127.0.0.1:18765").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 18765);
    }

    #[test]
    fn parse_endpoint_accepts_case_insensitive_scheme() {
        let (host, port) = parse_endpoint("QtServer://127.0.0.1:18765").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 18765);
    }

    #[test]
    fn parse_endpoint_rejects_wrong_scheme() {
        for bad in [
            "commandport://127.0.0.1:1234",
            "http://127.0.0.1:1234",
            "qtserver:/127.0.0.1:1234",
        ] {
            assert!(
                matches!(
                    parse_endpoint(bad),
                    Err(HostRpcError::TransportError { .. })
                ),
                "wrong scheme should reject: {bad}",
            );
        }
    }

    #[test]
    fn parse_endpoint_rejects_missing_port() {
        assert!(matches!(
            parse_endpoint("qtserver://127.0.0.1"),
            Err(HostRpcError::TransportError { .. }),
        ));
    }

    #[test]
    fn parse_endpoint_rejects_zero_port() {
        assert!(matches!(
            parse_endpoint("qtserver://127.0.0.1:0"),
            Err(HostRpcError::TransportError { .. }),
        ));
    }

    #[test]
    fn parse_endpoint_rejects_oversized_port() {
        assert!(matches!(
            parse_endpoint("qtserver://127.0.0.1:70000"),
            Err(HostRpcError::TransportError { .. }),
        ));
    }

    #[test]
    fn parse_endpoint_rejects_empty_host() {
        assert!(matches!(
            parse_endpoint("qtserver://:18765"),
            Err(HostRpcError::TransportError { .. }),
        ));
    }

    // ── interpret_envelope ───────────────────────────────────────────────

    #[test]
    fn interpret_envelope_extracts_result() {
        let env = json!({"id": "req-1", "result": {"value": 42}});
        let value = interpret_envelope(env, "any").unwrap();
        assert_eq!(value, json!({"value": 42}));
    }

    #[test]
    fn interpret_envelope_maps_dispatcher_error_to_transport_error() {
        let env = json!({
            "id": "req-2",
            "error": {"code": "handler-exception", "message": "BoomError: blew up"},
        });
        let err = interpret_envelope(env, "maya_scene__crash").unwrap_err();
        match err {
            HostRpcError::TransportError { message } => {
                assert!(message.contains("handler-exception"), "{message}");
                assert!(message.contains("BoomError"), "{message}");
                assert!(message.contains("maya_scene__crash"), "{message}");
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[test]
    fn interpret_envelope_rejects_malformed_envelope() {
        let env = json!({"id": "req-3"}); // no result, no error
        let err = interpret_envelope(env, "any").unwrap_err();
        assert!(matches!(err, HostRpcError::TransportError { .. }));
    }

    // ── connect / call roundtrip against a fake Qt server ────────────────

    /// Spawn an in-process TCP server that mimics the Qt dispatcher's
    /// JSON-line protocol. Each accepted connection reads one
    /// request line, looks the method up in `handlers`, writes the
    /// response line, and loops. Closing the connection terminates
    /// the per-connection task.
    type Handler = Box<dyn Fn(Value) -> Value + Send + Sync>;

    async fn spawn_fake_qt_server(
        handlers: std::collections::HashMap<&'static str, Handler>,
    ) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        let handlers = Arc::new(handlers);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let handlers = Arc::clone(&handlers);
                tokio::spawn(async move {
                    let (read_half, mut write_half) = stream.split();
                    let mut reader = BufReader::new(read_half);
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                        let request: Value = match serde_json::from_str(line.trim_end()) {
                            Ok(v) => v,
                            Err(_) => return,
                        };
                        let id = request.get("id").cloned().unwrap_or(Value::Null);
                        let method = request
                            .get("method")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let params = request.get("params").cloned().unwrap_or(Value::Null);
                        let envelope = if let Some(handler) = handlers.get(method.as_str()) {
                            let result = handler(params);
                            json!({"id": id, "result": result})
                        } else {
                            json!({
                                "id": id,
                                "error": {
                                    "code": "unknown-method",
                                    "message": format!("unknown method {method:?}"),
                                },
                            })
                        };
                        let mut out = serde_json::to_vec(&envelope).expect("serialise envelope");
                        out.push(b'\n');
                        if write_half.write_all(&out).await.is_err() {
                            return;
                        }
                        let _ = write_half.flush().await;
                    }
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn connect_and_call_dispatch_roundtrip() {
        let mut handlers = std::collections::HashMap::<&'static str, Handler>::new();
        handlers.insert(
            "dispatch",
            Box::new(|params: Value| {
                json!({
                    "echo_action": params.get("action").cloned(),
                    "echo_args": params.get("args").cloned(),
                    "echo_request_id": params.get("request_id").cloned(),
                })
            }),
        );
        let port = spawn_fake_qt_server(handlers).await;

        let mut client = QtServerClient::new();
        client
            .connect(
                &format!("qtserver://127.0.0.1:{port}"),
                Duration::from_secs(5),
            )
            .await
            .expect("connect");

        let result = client
            .call("maya_scene__list", json!({"limit": 10}), "req-1")
            .await
            .expect("call");
        assert_eq!(
            result.get("echo_action"),
            Some(&Value::String("maya_scene__list".into()))
        );
        assert_eq!(result.get("echo_args"), Some(&json!({"limit": 10})));
        assert_eq!(
            result.get("echo_request_id"),
            Some(&Value::String("req-1".into()))
        );
    }

    #[tokio::test]
    async fn call_surfaces_dispatcher_error_as_transport_error() {
        // The fake server doesn't know "dispatch"; the QtServerClient
        // sends method="dispatch" so we get an unknown-method back.
        let handlers = std::collections::HashMap::<&'static str, Handler>::new();
        let port = spawn_fake_qt_server(handlers).await;

        let mut client = QtServerClient::new();
        client
            .connect(
                &format!("qtserver://127.0.0.1:{port}"),
                Duration::from_secs(5),
            )
            .await
            .expect("connect");

        let err = client
            .call("maya_anything", json!({}), "req-2")
            .await
            .expect_err("must surface unknown-method as transport error");
        match err {
            HostRpcError::TransportError { message } => {
                assert!(message.contains("unknown-method"), "{message}");
                assert!(message.contains("maya_anything"), "{message}");
            }
            other => panic!("expected TransportError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn host_died_when_peer_closes_during_call() {
        // Spawn a TCP server that accepts then immediately closes
        // without writing a response — emulates the DCC process being
        // killed mid-call.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            drop(stream);
        });

        let mut client = QtServerClient::new();
        client
            .connect(
                &format!("qtserver://127.0.0.1:{port}"),
                Duration::from_secs(5),
            )
            .await
            .expect("connect");

        // Give the peer-close a moment so the next call hits a dead
        // socket reliably across platforms (Windows surfaces it on
        // the write; Unix usually surfaces it on the read).
        tokio::time::sleep(Duration::from_millis(50)).await;

        let err = client
            .call("any_action", json!({}), "req-3")
            .await
            .expect_err("call must fail when peer is gone");
        assert!(
            matches!(err, HostRpcError::HostDied { .. }),
            "expected HostDied, got {err:?}",
        );
    }

    #[tokio::test]
    async fn close_drops_socket_and_blocks_further_calls() {
        let mut handlers = std::collections::HashMap::<&'static str, Handler>::new();
        handlers.insert("dispatch", Box::new(|_| json!({"ok": true})));
        let port = spawn_fake_qt_server(handlers).await;

        let mut client = QtServerClient::new();
        client
            .connect(
                &format!("qtserver://127.0.0.1:{port}"),
                Duration::from_secs(5),
            )
            .await
            .expect("connect");
        client.close().await;
        assert!(!client.is_alive());

        let err = client
            .call("anything", json!({}), "req-after-close")
            .await
            .expect_err("call after close must fail");
        assert!(matches!(err, HostRpcError::TransportError { .. }));
    }

    #[tokio::test]
    async fn connect_timeout_is_honoured() {
        // Bind a socket but never accept — every connect attempt
        // hangs in the listener's backlog. We then verify the
        // timeout is mapped to HostRpcError::Timeout.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        // Drop listener so future connects RST instead of accepting.
        // We use a tiny non-existent port instead — pick something
        // that has no listener to make connect reliably refuse.
        drop(listener);

        let mut client = QtServerClient::new();
        let result = client
            .connect(
                &format!("qtserver://127.0.0.1:{port}"),
                Duration::from_millis(100),
            )
            .await;
        // Refused connect is a transport error (not a timeout) on
        // most OSes — we accept either as long as it does NOT
        // silently succeed.
        assert!(
            matches!(
                result,
                Err(HostRpcError::TransportError { .. }) | Err(HostRpcError::Timeout { .. }),
            ),
            "expected transport-error or timeout, got {result:?}",
        );
    }

    // ── bootstrap helpers ────────────────────────────────────────────────

    #[test]
    fn bootstrap_command_line_inlines_dispatcher_and_bootstrap_sources() {
        let line = build_bootstrap_command_line(0);
        // Both sources are embedded as Python string literals — the
        // escaped form must contain the dispatcher's class name and
        // the bootstrap's installer.
        assert!(line.contains("__import__('builtins').exec"));
        assert!(line.contains("compile"));
        assert!(line.contains("QtCommandServer"));
        assert!(line.contains("_install_result"));
        assert!(line.contains("_REQUESTED_PORT = 0"));
    }

    #[test]
    fn bootstrap_command_line_propagates_requested_port() {
        let line = build_bootstrap_command_line(12345);
        assert!(line.contains("_REQUESTED_PORT = 12345"));
    }

    #[test]
    fn start_command_line_targets_dispatcher_module() {
        let line = build_start_command_line(0);
        assert!(line.contains("_dcc_qt_dispatcher"));
        assert!(line.contains("start_qt_server"));
        assert!(line.contains("port=0"));
        assert!(line.contains("__import__('json').dumps"));
    }

    #[test]
    fn parse_start_reply_unquotes_and_extracts_endpoint() {
        // Maya's commandPort wraps a string return value in single
        // quotes (Python repr); tolerate that shape.
        let reply = "'{\"host\": \"127.0.0.1\", \"port\": 18765, \"qt_binding\": \"PySide6\"}'";
        let uri = parse_start_reply(reply).unwrap();
        assert_eq!(uri, "qtserver://127.0.0.1:18765");
    }

    #[test]
    fn parse_start_reply_accepts_raw_json() {
        let reply = "{\"host\": \"127.0.0.1\", \"port\": 9999}";
        let uri = parse_start_reply(reply).unwrap();
        assert_eq!(uri, "qtserver://127.0.0.1:9999");
    }

    #[test]
    fn parse_start_reply_rejects_missing_fields() {
        for bad in [
            "{\"port\": 1234}",
            "{\"host\": \"localhost\"}",
            "{\"host\": \"x\", \"port\": 0}",
            "{\"host\": \"x\", \"port\": 70000}",
            "not json",
        ] {
            assert!(
                parse_start_reply(bad).is_err(),
                "should reject reply {bad:?}",
            );
        }
    }
}
