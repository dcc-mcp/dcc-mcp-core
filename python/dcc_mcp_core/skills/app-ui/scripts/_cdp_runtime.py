"""CDP runtime helpers for app_ui script backends."""

from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess
import tempfile
import time
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from urllib.parse import urlparse
from urllib.request import Request
from urllib.request import urlopen

_DEFAULT_CDP_PORT = 9222
_REUSE_PRESETS = {"reuse", "current", "default", "profile", "browser"}
_ISOLATED_PRESETS = {"isolated", "temp", "temporary", "scoped", "sandbox"}
_AURORAVIEW_PRESETS = {"auroraview", "aurora-view", "aurora"}


class CdpBackendError(RuntimeError):
    """Raised when the CDP backend cannot complete an operation."""


class CdpClient:
    def __init__(self, ws_url: str):
        self._url = urlparse(ws_url)
        self._sock: Optional[socket.socket] = None
        self._next_id = 1

    def __enter__(self) -> CdpClient:
        host = self._url.hostname or "127.0.0.1"
        port = int(self._url.port or 80)
        path = self._url.path or "/"
        if self._url.query:
            path += "?" + self._url.query
        sock = socket.create_connection((host, port), timeout=5)
        key = base64.b64encode(secrets.token_bytes(16)).decode("ascii")
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        )
        sock.sendall(request.encode("ascii"))
        response = b""
        while b"\r\n\r\n" not in response:
            response += sock.recv(4096)
        if b" 101 " not in response.split(b"\r\n", 1)[0]:
            raise CdpBackendError("CDP websocket handshake failed")
        self._sock = sock
        return self

    def __exit__(self, *_exc: object) -> None:
        if self._sock is not None:
            try:
                self._sock.close()
            finally:
                self._sock = None

    def call(self, method: str, params: Optional[Dict[str, Any]] = None, timeout: float = 8.0) -> Dict[str, Any]:
        msg_id = self._next_id
        self._next_id += 1
        payload = {"id": msg_id, "method": method}
        if params is not None:
            payload["params"] = params
        self._send_text(json.dumps(payload))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            message = self._read_text(max(0.1, deadline - time.monotonic()))
            if not message:
                continue
            data = json.loads(message)
            if data.get("id") != msg_id:
                continue
            if "error" in data:
                raise CdpBackendError(f"CDP {method} failed: {data['error']}")
            return data.get("result") or {}
        raise CdpBackendError(f"CDP {method} timed out")

    def _send_text(self, text: str) -> None:
        assert self._sock is not None
        payload = text.encode("utf-8")
        header = bytearray([0x81])
        length = len(payload)
        if length < 126:
            header.append(0x80 | length)
        elif length < 65536:
            header.extend([0x80 | 126])
            header.extend(struct.pack("!H", length))
        else:
            header.extend([0x80 | 127])
            header.extend(struct.pack("!Q", length))
        mask = secrets.token_bytes(4)
        masked = bytes(byte ^ mask[i % 4] for i, byte in enumerate(payload))
        self._sock.sendall(bytes(header) + mask + masked)

    def _read_exact(self, size: int, timeout: float) -> bytes:
        assert self._sock is not None
        self._sock.settimeout(timeout)
        data = bytearray()
        while len(data) < size:
            chunk = self._sock.recv(size - len(data))
            if not chunk:
                raise CdpBackendError("CDP websocket closed")
            data.extend(chunk)
        return bytes(data)

    def _read_text(self, timeout: float) -> str:
        header = self._read_exact(2, timeout)
        opcode = header[0] & 0x0F
        length = header[1] & 0x7F
        masked = bool(header[1] & 0x80)
        if length == 126:
            length = struct.unpack("!H", self._read_exact(2, timeout))[0]
        elif length == 127:
            length = struct.unpack("!Q", self._read_exact(8, timeout))[0]
        mask = self._read_exact(4, timeout) if masked else b""
        payload = self._read_exact(length, timeout) if length else b""
        if masked:
            payload = bytes(byte ^ mask[i % 4] for i, byte in enumerate(payload))
        if opcode == 0x8:
            raise CdpBackendError("CDP websocket closed")
        if opcode == 0x9:
            return ""
        if opcode not in {0x1, 0x0}:
            return ""
        return payload.decode("utf-8")


def cdp_preset() -> str:
    raw = os.environ.get("DCC_MCP_APP_UI_CDP_PRESET") or os.environ.get("DCC_MCP_APP_UI_CHROME_PRESET") or "reuse"
    value = raw.strip().lower()
    if value in _REUSE_PRESETS:
        return "reuse"
    if value in _ISOLATED_PRESETS:
        return "isolated"
    if value in _AURORAVIEW_PRESETS:
        return "auroraview"
    raise CdpBackendError(f"Unsupported app_ui CDP preset {raw!r}; use reuse, isolated, or auroraview")


def endpoint_candidates(preset: str) -> List[str]:
    candidates: List[str] = []
    for name in ("DCC_MCP_APP_UI_CDP_URL", "DCC_MCP_APP_UI_CHROME_CDP_URL"):
        raw = os.environ.get(name)
        if raw:
            candidates.append(_normalise_cdp_endpoint(raw))

    if preset == "auroraview":
        port_names = (
            "DCC_MCP_APP_UI_AURORAVIEW_CDP_PORT",
            "AURORAVIEW_CDP_PORT",
            "DCC_MCP_APP_UI_CDP_PORT",
        )
        default_port: Optional[int] = _DEFAULT_CDP_PORT
    elif preset == "reuse":
        port_names = (
            "DCC_MCP_APP_UI_CDP_PORT",
            "DCC_MCP_APP_UI_CHROME_CDP_PORT",
        )
        default_port = _DEFAULT_CDP_PORT
    else:
        port_names = ("DCC_MCP_APP_UI_CDP_PORT",)
        default_port = None

    for name in port_names:
        port = _env_int(name)
        if port:
            candidates.append(f"http://127.0.0.1:{port}")
    if default_port:
        candidates.append(f"http://127.0.0.1:{default_port}")

    unique: List[str] = []
    for candidate in candidates:
        if candidate not in unique:
            unique.append(candidate)
    return unique


def ensure_cdp_target(state: Dict[str, Any]) -> Dict[str, Any]:
    preset = cdp_preset()
    previous_preset = str(state.get("preset") or "")
    if previous_preset and previous_preset != preset:
        state["port"] = 0
        state["pid"] = 0
        state["web_socket_url"] = ""
        state["cdp_endpoint"] = ""
        if preset != "isolated":
            state["user_data_dir"] = ""
    state["preset"] = preset

    endpoint = str(state.get("cdp_endpoint") or "")
    if endpoint:
        connected = _try_connect_endpoint(state, endpoint, str(state.get("launch_mode") or preset))
        if connected is not None:
            return connected

    port = int(state.get("port") or 0)
    if port and _port_ready(port):
        connected = _try_connect_endpoint(
            state,
            f"http://127.0.0.1:{port}",
            str(state.get("launch_mode") or preset),
        )
        if connected is not None:
            return connected

    for candidate in endpoint_candidates(preset):
        connected = _try_connect_endpoint(state, candidate, preset)
        if connected is not None:
            return connected

    if preset == "auroraview":
        raise CdpBackendError(
            "AuroraView CDP endpoint is not available; start AuroraView with "
            f"AURORAVIEW_CDP_PORT={_DEFAULT_CDP_PORT} or set DCC_MCP_APP_UI_CDP_URL"
        )
    if preset == "isolated":
        return _launch_chrome(state, isolated=True)
    return _launch_chrome(state, isolated=False)


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _find_chrome() -> str:
    env_path = os.environ.get("DCC_MCP_CHROME_PATH")
    candidates = [
        env_path,
        str(Path(os.environ.get("PROGRAMFILES", "")) / "Google" / "Chrome" / "Application" / "chrome.exe"),
        str(Path(os.environ.get("PROGRAMFILES(X86)", "")) / "Google" / "Chrome" / "Application" / "chrome.exe"),
        str(Path(os.environ.get("LOCALAPPDATA", "")) / "Google" / "Chrome" / "Application" / "chrome.exe"),
        shutil.which("chrome"),
        shutil.which("google-chrome"),
        shutil.which("chromium"),
    ]
    for candidate in candidates:
        if candidate and Path(candidate).exists():
            return str(candidate)
    raise CdpBackendError("Chrome executable not found; set DCC_MCP_CHROME_PATH")


def _env_int(name: str) -> Optional[int]:
    raw = os.environ.get(name)
    if not raw:
        return None
    try:
        value = int(raw)
    except ValueError as exc:
        raise CdpBackendError(f"{name} must be an integer port") from exc
    if value <= 0 or value > 65535:
        raise CdpBackendError(f"{name} must be a valid TCP port")
    return value


def _normalise_cdp_endpoint(value: str) -> str:
    text = value.strip()
    if not text:
        raise CdpBackendError("CDP endpoint is empty")
    parsed = urlparse(text)
    if parsed.scheme in {"ws", "wss", "http", "https"}:
        return text.rstrip("/")
    if text.isdigit():
        return f"http://127.0.0.1:{text}"
    if ":" in text:
        return "http://" + text.rstrip("/")
    return f"http://127.0.0.1:{text}"


def _endpoint_port(endpoint: str) -> Optional[int]:
    try:
        return urlparse(endpoint).port
    except ValueError:
        return None


def _json_get_base(endpoint: str, path: str, timeout: float = 3.0) -> Any:
    if endpoint.startswith(("ws://", "wss://")):
        raise CdpBackendError("HTTP DevTools endpoint required for JSON discovery")
    suffix = path if path.startswith("/") else f"/{path}"
    with urlopen(f"{endpoint.rstrip('/')}{suffix}", timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def _json_get(port: int, path: str, timeout: float = 3.0) -> Any:
    return _json_get_base(f"http://127.0.0.1:{port}", path, timeout=timeout)


def _json_open_target(endpoint: str) -> str:
    try:
        data = _json_get_base(endpoint, "/json/new?about:blank")
    except Exception:
        req = Request(f"{endpoint.rstrip('/')}/json/new?about:blank", method="PUT")
        with urlopen(req, timeout=3) as response:
            data = json.loads(response.read().decode("utf-8"))
    ws_url = data.get("webSocketDebuggerUrl")
    if not ws_url:
        raise CdpBackendError("CDP endpoint did not return a page websocket URL")
    return str(ws_url)


def _page_websocket(endpoint: str) -> str:
    if endpoint.startswith(("ws://", "wss://")):
        return endpoint
    pages = _json_get_base(endpoint, "/json/list")
    if not isinstance(pages, list):
        pages = []
    for page in pages:
        page_type = page.get("type")
        if page_type in {"page", "webview"} and page.get("webSocketDebuggerUrl"):
            return str(page["webSocketDebuggerUrl"])
    for page in pages:
        if page.get("webSocketDebuggerUrl"):
            return str(page["webSocketDebuggerUrl"])
    return _json_open_target(endpoint)


def _port_ready(port: int) -> bool:
    try:
        _json_get(port, "/json/version", timeout=1.0)
        return True
    except Exception:
        return False


def _connect_endpoint(state: Dict[str, Any], endpoint: str, launch_mode: str) -> Dict[str, Any]:
    ws_url = _page_websocket(endpoint)
    _probe_cdp_page(ws_url)
    state["web_socket_url"] = ws_url
    state["cdp_endpoint"] = endpoint
    state["launch_mode"] = launch_mode
    port = _endpoint_port(endpoint) or _endpoint_port(ws_url)
    if port:
        state["port"] = int(port)
    if launch_mode in {"reuse", "auroraview"}:
        state["pid"] = 0
        state["user_data_dir"] = ""
    return state


def _try_connect_endpoint(state: Dict[str, Any], endpoint: str, launch_mode: str) -> Optional[Dict[str, Any]]:
    try:
        return _connect_endpoint(state, endpoint, launch_mode)
    except Exception:
        return None


def _probe_cdp_page(ws_url: str) -> None:
    with CdpClient(ws_url) as cdp:
        cdp.call("Runtime.evaluate", {"expression": "1", "returnByValue": True}, timeout=3.0)


def _launch_chrome(state: Dict[str, Any], isolated: bool) -> Dict[str, Any]:
    chrome = _find_chrome()
    port = _env_int("DCC_MCP_APP_UI_CDP_PORT") or _free_port()
    args = [
        chrome,
        f"--remote-debugging-port={port}",
    ]
    if isolated:
        user_data = state.get("user_data_dir") or tempfile.mkdtemp(prefix="dcc-app-ui-chrome-profile-")
        args.append(f"--user-data-dir={user_data}")
    else:
        user_data = os.environ.get("DCC_MCP_APP_UI_CHROME_USER_DATA_DIR", "")
        profile_dir = os.environ.get("DCC_MCP_APP_UI_CHROME_PROFILE_DIRECTORY", "")
        if user_data:
            args.append(f"--user-data-dir={user_data}")
        if profile_dir:
            args.append(f"--profile-directory={profile_dir}")
    args.extend(
        [
            "--new-window",
            "about:blank",
        ]
    )
    proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = time.monotonic() + 8.0
    while time.monotonic() < deadline:
        if _port_ready(port):
            state["port"] = port
            state["pid"] = int(proc.pid)
            state["user_data_dir"] = user_data
            state["cdp_endpoint"] = f"http://127.0.0.1:{port}"
            state["launch_mode"] = "isolated" if isolated else "reuse"
            state["web_socket_url"] = _page_websocket(str(state["cdp_endpoint"]))
            return state
        time.sleep(0.1)
    if isolated:
        raise CdpBackendError("Chrome DevTools endpoint did not become ready")
    raise CdpBackendError(
        "Reusable Chrome DevTools endpoint did not become ready; start Chrome "
        f"with --remote-debugging-port={_DEFAULT_CDP_PORT}, set "
        "DCC_MCP_APP_UI_CDP_URL, or use DCC_MCP_APP_UI_CDP_PRESET=isolated"
    )
