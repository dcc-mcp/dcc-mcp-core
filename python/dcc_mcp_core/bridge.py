"""Generic WebSocket bridge for non-Python DCCs.

Implements the server-side of the dcc-mcp-core WebSocket JSON-RPC 2.0 bridge
protocol (see ``crates/dcc-mcp-protocols/src/bridge.rs`` for the full spec).

Usage
-----
::

    from dcc_mcp_core.bridge import DccBridge, BridgeConnectionError, BridgeTimeoutError

    # Start server, wait until DCC plugin connects
    bridge = DccBridge(port=9001)
    bridge.connect(wait_for_dcc=True)

    # Synchronous RPC call to the DCC plugin
    result = bridge.call("ps.getDocumentInfo")
    layers = bridge.call("ps.listLayers", include_hidden=True)

    bridge.disconnect()

Context manager::

    with DccBridge(port=9001) as bridge:
        info = bridge.call("ps.getDocumentInfo")
"""

from __future__ import annotations

import asyncio
from concurrent.futures import Future
import json
import logging
import threading
from typing import Any
import uuid

logger = logging.getLogger(__name__)

# ── Exceptions ────────────────────────────────────────────────────────────────


class BridgeError(Exception):
    """Base class for all DccBridge errors."""


class BridgeConnectionError(BridgeError):
    """Raised when the DCC plugin is not connected or the connection is lost."""


class BridgeTimeoutError(BridgeError):
    """Raised when a call to the DCC plugin times out."""


class BridgeRpcError(BridgeError):
    """Raised when the DCC plugin returns a JSON-RPC error response.

    Attributes
    ----------
    code:
        Numeric error code (e.g. ``-32601`` for method-not-found).
    message:
        Human-readable error description from the DCC plugin.
    data:
        Optional additional error payload.

    """

    def __init__(self, code: int, message: str, data: Any = None) -> None:
        super().__init__(f"[{code}] {message}")
        self.code = code
        self.message = message
        self.data = data


# ── Standard error codes (mirrors bridge.rs error_codes) ─────────────────────

PARSE_ERROR = -32700
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603
NO_ACTIVE_DOCUMENT = -32001
DCC_ERROR = -32000


# ── DccBridge ─────────────────────────────────────────────────────────────────


class DccBridge:
    """WebSocket bridge server that waits for a DCC plugin to connect.

    The bridge starts a WebSocket server in a background thread that owns an
    ``asyncio`` event loop.  Synchronous :py:meth:`call` is thread-safe and can
    be used from any thread (including DCC main threads).

    Parameters
    ----------
    host:
        Bind address for the WebSocket server (default ``"localhost"``).
    port:
        Port for the WebSocket server (default ``9001``).
    timeout:
        Default timeout in seconds for :py:meth:`call` (default ``30.0``).
    server_name:
        Name advertised in the ``hello_ack`` handshake.
    server_version:
        Version advertised in the ``hello_ack`` handshake.

    """

    def __init__(
        self,
        host: str = "localhost",
        port: int = 9001,
        timeout: float = 30.0,
        server_name: str = "dcc-mcp-server",
        server_version: str = "0.12.18",
    ) -> None:
        self._host = host
        self._port = port
        self._timeout = timeout
        self._server_name = server_name
        self._server_version = server_version

        self._loop: asyncio.AbstractEventLoop | None = None
        self._thread: threading.Thread | None = None
        self._ws_server = None  # asyncio-ws server handle

        # Set once the TCP server is bound and accepting connections.
        self._server_ready = threading.Event()
        # Set once a DCC plugin completes the hello handshake.
        self._dcc_connected = threading.Event()
        # Active WebSocket connection (asyncio transport object).
        self._ws = None  # type: Any

        # Pending futures keyed by request id.
        self._pending: dict[str | int, Future] = {}
        self._pending_lock = threading.Lock()
        self._next_id = 0
        self._id_lock = threading.Lock()

        self._connected = False
        self._closed = False

    # ── Public API ───────────────────────────────────────────────────────────

    @property
    def endpoint(self) -> str:
        """WebSocket endpoint URL (e.g. ``"ws://localhost:9001"``)."""
        return f"ws://{self._host}:{self._port}"

    def is_connected(self) -> bool:
        """Return ``True`` once a DCC plugin has completed the handshake."""
        return self._connected

    def connect(self, wait_for_dcc: bool = False) -> None:
        """Start the WebSocket server.

        Parameters
        ----------
        wait_for_dcc:
            If ``True``, block until the DCC plugin connects and completes the
            ``hello`` handshake.  If ``False``, return immediately after the
            TCP port is bound.

        """
        if self._thread is not None:
            raise BridgeError("Bridge is already started.")

        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._run_event_loop, daemon=True, name="dcc-bridge")
        self._thread.start()

        if not self._server_ready.wait(timeout=10.0):
            raise BridgeConnectionError("WebSocket server failed to start within 10 seconds.")

        if wait_for_dcc and not self._dcc_connected.wait(timeout=self._timeout):
            raise BridgeConnectionError(f"DCC plugin did not connect within {self._timeout}s.")

    def disconnect(self) -> None:
        """Shut down the WebSocket server and close any active connection."""
        self._closed = True
        self._connected = False
        if self._loop is not None:
            self._loop.call_soon_threadsafe(self._loop.stop)
        if self._thread is not None:
            self._thread.join(timeout=5.0)
            self._thread = None
        # Fail all pending futures.
        with self._pending_lock:
            for fut in self._pending.values():
                if not fut.done():
                    fut.set_exception(BridgeConnectionError("Bridge disconnected."))
            self._pending.clear()

    def call(self, method: str, **params: Any) -> Any:
        """Invoke a method on the connected DCC plugin (synchronous).

        Parameters
        ----------
        method:
            The method name to invoke (e.g. ``"ps.getDocumentInfo"``).
        **params:
            Keyword arguments forwarded as the JSON-RPC ``params`` object.

        Returns
        -------
        Any
            The ``result`` value from the DCC plugin's response.

        Raises
        ------
        BridgeConnectionError
            If no DCC plugin is currently connected.
        BridgeTimeoutError
            If the DCC plugin does not respond within ``timeout`` seconds.
        BridgeRpcError
            If the DCC plugin returns a JSON-RPC error response.

        """
        if not self._connected:
            raise BridgeConnectionError("No DCC plugin is connected.")

        req_id = self._next_request_id()
        fut: Future = Future()

        with self._pending_lock:
            self._pending[req_id] = fut

        message = {
            "type": "request",
            "jsonrpc": "2.0",
            "id": req_id,
            "method": method,
        }
        if params:
            message["params"] = params

        asyncio.run_coroutine_threadsafe(self._send(json.dumps(message)), self._loop)

        try:
            result = fut.result(timeout=self._timeout)
        except TimeoutError as exc:
            with self._pending_lock:
                self._pending.pop(req_id, None)
            raise BridgeTimeoutError(f"Method '{method}' (id={req_id}) timed out after {self._timeout}s.") from exc

        return result

    # ── Context manager ──────────────────────────────────────────────────────

    def __enter__(self) -> DccBridge:
        self.connect(wait_for_dcc=True)
        return self

    def __exit__(self, *_: Any) -> None:
        self.disconnect()

    # ── Internal: event loop thread ──────────────────────────────────────────

    def _run_event_loop(self) -> None:
        asyncio.set_event_loop(self._loop)
        try:
            self._loop.run_until_complete(self._serve())
        except Exception:
            logger.exception("DccBridge event loop crashed")
        finally:
            self._loop.close()

    async def _serve(self) -> None:
        try:
            import websockets  # type: ignore[import-untyped]
        except ImportError as exc:
            raise ImportError(
                "The 'websockets' package is required for DccBridge. Install it with: pip install websockets"
            ) from exc

        async with websockets.serve(self._handle_dcc, self._host, self._port) as server:
            self._ws_server = server
            self._server_ready.set()
            logger.debug("DccBridge listening on %s", self.endpoint)
            # Run until stop() is called.
            await asyncio.get_event_loop().create_future()

    async def _handle_dcc(self, ws: Any) -> None:
        """Handle a single DCC plugin WebSocket connection."""
        logger.debug("DCC plugin connected from %s", ws.remote_address)
        self._ws = ws

        try:
            async for raw in ws:
                await self._dispatch(raw)
        except Exception as exc:
            logger.debug("DCC plugin connection closed: %s", exc)
        finally:
            self._connected = False
            self._ws = None
            # Fail all pending requests.
            with self._pending_lock:
                for fut in list(self._pending.values()):
                    if not fut.done():
                        fut.set_exception(BridgeConnectionError("DCC plugin disconnected."))
                self._pending.clear()
            logger.debug("DCC plugin disconnected")

    async def _dispatch(self, raw: str) -> None:
        """Parse an incoming message and route it."""
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError as exc:
            await self._send(
                json.dumps(
                    {
                        "type": "parse_error",
                        "message": str(exc),
                    }
                )
            )
            return

        msg_type = msg.get("type")

        if msg_type == "hello":
            await self._handle_hello(msg)
        elif msg_type == "response":
            self._handle_response(msg)
        elif msg_type == "event":
            self._handle_event(msg)
        elif msg_type == "disconnect":
            logger.debug("DCC plugin sent disconnect: %s", msg.get("reason"))
        else:
            logger.warning("Unknown bridge message type: %r", msg_type)

    async def _handle_hello(self, msg: dict) -> None:
        client = msg.get("client", "unknown")
        version = msg.get("version", "?")
        logger.info("DCC plugin hello: client=%s version=%s", client, version)

        ack = {
            "type": "hello_ack",
            "server": self._server_name,
            "version": self._server_version,
            "session_id": str(uuid.uuid4()),
        }
        await self._send(json.dumps(ack))
        self._connected = True
        self._dcc_connected.set()

    def _handle_response(self, msg: dict) -> None:
        req_id = msg.get("id")
        with self._pending_lock:
            fut = self._pending.pop(req_id, None)
        if fut is None or fut.done():
            logger.warning("Received response for unknown id: %r", req_id)
            return

        if "error" in msg and msg["error"] is not None:
            err = msg["error"]
            fut.set_exception(
                BridgeRpcError(
                    code=err.get("code", INTERNAL_ERROR),
                    message=err.get("message", "unknown error"),
                    data=err.get("data"),
                )
            )
        else:
            fut.set_result(msg.get("result"))

    def _handle_event(self, msg: dict) -> None:
        event = msg.get("event", "unknown")
        data = msg.get("data")
        logger.debug("DCC event: %s data=%r", event, data)

    async def _send(self, text: str) -> None:
        if self._ws is not None:
            try:
                await self._ws.send(text)
            except Exception as exc:
                logger.debug("Failed to send message: %s", exc)

    # ── Internal: id generation ──────────────────────────────────────────────

    def _next_request_id(self) -> int:
        with self._id_lock:
            rid = self._next_id
            self._next_id += 1
        return rid

    def __repr__(self) -> str:
        return f"DccBridge(endpoint={self.endpoint!r}, connected={self._connected}, pending={len(self._pending)})"
