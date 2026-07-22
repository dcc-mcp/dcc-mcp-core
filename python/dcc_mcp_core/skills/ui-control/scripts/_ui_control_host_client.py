"""Thin named-pipe client for the isolated Windows UI Control host."""

from __future__ import annotations

from contextlib import suppress
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import threading
import time
from typing import Any
from typing import BinaryIO
from typing import Dict
from typing import List
from typing import Optional

_PROTOCOL_VERSION = 2
_MAX_FRAME_BYTES = 4 * 1024 * 1024
_HOST_NAME = "dcc-mcp-ui-control-host.exe"
_SYSTEM_OPERATIONS_CAPABILITY = "typed_system_operations"
_RECORDING_CAPABILITY = "exact_window_recording"


class UiControlHostError(RuntimeError):
    """Structured, redacted host failure."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


def _windows_session_id() -> int:
    if sys.platform != "win32":
        raise UiControlHostError("backend_unavailable", "The UI Control host is only available on Windows.")
    import ctypes

    session_id = ctypes.c_ulong()
    if not ctypes.windll.kernel32.ProcessIdToSessionId(os.getpid(), ctypes.byref(session_id)):
        raise UiControlHostError("backend_unavailable", "Cannot resolve the interactive Windows session.")
    return int(session_id.value)


def _pipe_path() -> str:
    return rf"\\.\pipe\dcc-mcp-ui-control-host-v2-session-{_windows_session_id()}"


def _candidate_binaries() -> List[Path]:
    configured = os.environ.get("DCC_MCP_UI_CONTROL_HOST")
    candidates = []
    if configured:
        candidates.append(Path(configured))
    package_root = Path(__file__).resolve().parents[3]
    candidates.append(package_root / "bin" / _HOST_NAME)
    repository_root = Path(__file__).resolve().parents[5]
    candidates.append(repository_root / "target" / "release" / _HOST_NAME)
    return candidates


def _host_binary() -> Path:
    for candidate in _candidate_binaries():
        if candidate.is_file():
            return candidate
    raise UiControlHostError(
        "backend_unavailable",
        "The isolated UI Control host is not installed. Repair or reinstall the Windows dcc-mcp-core package.",
    )


def _launch_host() -> None:
    binary = _host_binary()
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0) | getattr(subprocess, "DETACHED_PROCESS", 0)
    subprocess.Popen(
        [str(binary)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        close_fds=True,
        creationflags=creationflags,
    )


def _connect_pipe(*, launch: bool = True) -> BinaryIO:
    path = _pipe_path()
    deadline = time.monotonic() + 5.0
    launched = False
    last_error: Optional[OSError] = None
    while time.monotonic() < deadline:
        try:
            stream = open(path, "r+b", buffering=0)  # noqa: PTH123, SIM115
            _validate_server_binary(stream)
            return stream
        except OSError as exc:
            last_error = exc
            if launch and not launched:
                _launch_host()
                launched = True
            time.sleep(0.05)
    raise UiControlHostError(
        "backend_unavailable",
        f"The isolated UI Control host named pipe is unavailable: {last_error}",
    )


def _validate_server_binary(stream: BinaryIO) -> None:
    """Fail closed when the pipe server is not an installed host executable."""
    import ctypes
    from ctypes import wintypes
    import msvcrt

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.GetNamedPipeServerProcessId.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.ULONG)]
    kernel32.GetNamedPipeServerProcessId.restype = wintypes.BOOL
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    kernel32.QueryFullProcessImageNameW.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL
    pipe_handle = wintypes.HANDLE(msvcrt.get_osfhandle(stream.fileno()))
    server_pid = wintypes.ULONG()
    if not kernel32.GetNamedPipeServerProcessId(pipe_handle, ctypes.byref(server_pid)):
        stream.close()
        raise UiControlHostError("backend_unavailable", "Cannot authenticate the UI Control host process.")
    process = kernel32.OpenProcess(0x1000, False, server_pid.value)
    if not process:
        stream.close()
        raise UiControlHostError("backend_unavailable", "Cannot inspect the UI Control host executable.")
    try:
        capacity = wintypes.DWORD(32768)
        buffer = ctypes.create_unicode_buffer(capacity.value)
        if not kernel32.QueryFullProcessImageNameW(process, 0, buffer, ctypes.byref(capacity)):
            raise UiControlHostError("backend_unavailable", "Cannot resolve the UI Control host executable.")
        actual = os.path.normcase(str(Path(buffer.value).resolve()))
        expected = os.path.normcase(str(_host_binary().resolve()))
        if actual != expected:
            raise UiControlHostError(
                "backend_unavailable",
                "The named pipe server is not an installed dcc-mcp-ui-control-host executable.",
            )
    except Exception:
        stream.close()
        raise
    finally:
        kernel32.CloseHandle(process)


def _read_exact(stream: BinaryIO, length: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < length:
        chunk = stream.read(length - len(chunks))
        if not chunk:
            raise UiControlHostError("backend_unavailable", "The UI Control host disconnected.")
        chunks.extend(chunk)
    return bytes(chunks)


def _exchange(
    stream: BinaryIO,
    request: Dict[str, Any],
    *,
    expected_type: str,
    allow_unsuccessful: bool = False,
) -> Dict[str, Any]:
    body = json.dumps(request, separators=(",", ":")).encode("utf-8")
    if not body or len(body) > _MAX_FRAME_BYTES:
        raise UiControlHostError("invalid_request", "The UI Control host request is outside the frame limit.")
    try:
        stream.write(struct.pack(">I", len(body)))
        stream.write(body)
        prefix = _read_exact(stream, 4)
        length = struct.unpack(">I", prefix)[0]
        if not 0 < length <= _MAX_FRAME_BYTES:
            raise UiControlHostError("backend_unavailable", "The UI Control host returned an invalid frame length.")
        response = json.loads(_read_exact(stream, length).decode("utf-8"))
    except UiControlHostError:
        raise
    except (OSError, UnicodeError, ValueError) as exc:
        raise UiControlHostError("backend_unavailable", f"The UI Control host transport failed: {exc}") from exc
    if not isinstance(response, dict):
        raise UiControlHostError("backend_unavailable", "The UI Control host returned a non-object response.")
    if response.get("type") == "error":
        raise UiControlHostError(
            str(response.get("code") or "backend_unavailable"),
            str(response.get("message") or "The UI Control host rejected the request."),
        )
    if response.get("type") != expected_type:
        raise UiControlHostError(
            "backend_unavailable",
            "The UI Control host returned {!r}; expected {!r}.".format(response.get("type"), expected_type),
        )
    if not allow_unsuccessful and response.get("success") is False:
        raise UiControlHostError(
            str(response.get("error") or "backend_unavailable"),
            str(response.get("message") or "The UI Control host operation failed."),
        )
    return response


def execute_system_operation(
    *,
    session_id: str,
    system_grant_id: str,
    operation_id: str,
    stream: Optional[BinaryIO] = None,
) -> Dict[str, Any]:
    """Execute one host-resolved operator-granted operation in a short-lived session."""
    pipe = stream or _connect_pipe()
    opened = False
    try:
        hello = _exchange(
            pipe,
            {
                "method": "hello",
                "params": {
                    "protocol_version": _PROTOCOL_VERSION,
                    "client_name": "dcc-mcp-ui-control-system-operations",
                },
            },
            expected_type="hello",
        )
        capabilities = hello.get("capabilities")
        if not isinstance(capabilities, list) or _SYSTEM_OPERATIONS_CAPABILITY not in capabilities:
            raise UiControlHostError(
                "unsupported",
                "The installed UI Control host does not support typed system operations.",
            )
        opened_response = _exchange(
            pipe,
            {
                "method": "open_system_session",
                "params": {
                    "session_id": session_id,
                    "system_grant_id": system_grant_id,
                },
            },
            expected_type="system_session_opened",
        )
        opened = True
        system_capability = opened_response.get("system_capability")
        if not isinstance(system_capability, str) or not system_capability:
            raise UiControlHostError("backend_unavailable", "The UI Control host returned no system capability.")
        return _exchange(
            pipe,
            {
                "method": "execute_system_operation",
                "params": {
                    "session_id": session_id,
                    "system_grant_id": system_grant_id,
                    "system_capability": system_capability,
                    "operation_id": operation_id,
                },
            },
            expected_type="system_operation_completed",
        )
    finally:
        if opened:
            with suppress(Exception):
                _exchange(
                    pipe,
                    {"method": "stop_system_session", "params": {"session_id": session_id}},
                    expected_type="system_session_stopped",
                )
        with suppress(OSError):
            pipe.close()


class UiControlHostClient:
    """One adapter session bound to an opaque host window capability."""

    def __init__(
        self,
        *,
        session_id: str,
        task_grant_id: str,
        dcc_type: str,
        process_id: Optional[int],
        window_handle: Optional[int],
        allow_raw_input: bool,
        stream: Optional[BinaryIO] = None,
    ) -> None:
        self.session_id = session_id
        self.task_grant_id = task_grant_id
        self.dcc_type = dcc_type
        self.process_id = process_id
        self.window_handle = window_handle
        self.allow_raw_input = allow_raw_input
        self._stream = stream or _connect_pipe()
        self._lock = threading.RLock()
        self._window_capability: Optional[str] = None
        self._target: Dict[str, Any] = {}
        self._latest_observation_id: Optional[str] = None
        self._latest_accessibility_state_id: Optional[str] = None
        hello = self._call(
            {
                "method": "hello",
                "params": {
                    "protocol_version": _PROTOCOL_VERSION,
                    "client_name": "dcc-mcp-ui-control",
                },
            },
            expected_type="hello",
        )
        capabilities = hello.get("capabilities")
        self._capabilities = (
            {str(capability) for capability in capabilities if isinstance(capability, str)}
            if isinstance(capabilities, list)
            else set()
        )
        opened = self._call(
            {
                "method": "open_session",
                "params": {
                    "session_id": session_id,
                    "grant": {
                        "task_grant_id": task_grant_id,
                        "dcc_type": dcc_type,
                        "process_id": process_id,
                        "window_handle": window_handle,
                        "allow_raw_input": allow_raw_input,
                    },
                },
            },
            expected_type="session_opened",
        )
        self._window_capability = str(opened["window_capability"])
        self._target = dict(opened.get("target") or {})

    @property
    def target(self) -> Dict[str, Any]:
        """Return the host-validated exact target."""
        return dict(self._target)

    def snapshot(self, *, max_depth: int, max_nodes: int) -> Dict[str, Any]:
        """Capture one host-owned shared-memory PNG and UIA state."""
        response = self._call(
            {
                "method": "snapshot",
                "params": {
                    **self._authority(),
                    "max_depth": max_depth,
                    "max_nodes": max_nodes,
                },
            },
            expected_type="snapshot",
        )
        self._latest_observation_id = str(response["observation_id"])
        self._latest_accessibility_state_id = str(response["accessibility_state_id"])
        self._target = dict(response.get("target") or self._target)
        image = response.get("image") or {}
        try:
            from dcc_mcp_core import PySharedBuffer

            buffer = PySharedBuffer.open(str(image["name"]), str(image["id"]))
            png = bytes(buffer.read())
        except Exception as exc:
            self._invalidate_observation()
            raise UiControlHostError(
                "capture_failed",
                f"Cannot read the host screenshot shared-memory buffer: {exc}",
            ) from exc
        expected_length = int(image.get("length") or 0)
        if expected_length <= 0 or len(png) != expected_length:
            self._invalidate_observation()
            raise UiControlHostError("capture_failed", "The host screenshot shared-memory length is invalid.")
        response["image_bytes"] = png
        return response

    def window_state(self) -> Dict[str, Any]:
        """Read exact HWND state without requiring a screenshot."""
        return self._call(
            {"method": "get_window_state", "params": self._authority()},
            expected_type="window_state",
        )

    def record_clip(
        self,
        *,
        duration_ms: int,
        frames_per_second: int,
        jpeg_quality: int,
    ) -> Dict[str, Any]:
        """Record one bounded JPEG sequence from the exact capability-bound window."""
        if _RECORDING_CAPABILITY not in self._capabilities:
            raise UiControlHostError(
                "unsupported",
                "The installed UI Control host does not support exact-window recording.",
            )
        if not 1_000 <= duration_ms <= 180_000:
            raise UiControlHostError("invalid_request", "duration_ms must be 1000..=180000.")
        if not 1 <= frames_per_second <= 60:
            raise UiControlHostError("invalid_request", "frames_per_second must be 1..=60.")
        if not 70 <= jpeg_quality <= 100:
            raise UiControlHostError("invalid_request", "jpeg_quality must be 70..=100.")
        try:
            response = self._call(
                {
                    "method": "record_clip",
                    "params": {
                        **self._authority(),
                        "duration_ms": duration_ms,
                        "frames_per_second": frames_per_second,
                        "format": "jpeg_sequence",
                        "jpeg_quality": jpeg_quality,
                    },
                },
                expected_type="clip_recorded",
            )
            target = response.get("target")
            if not isinstance(target, dict) or (
                int(target.get("process_id") or 0) != int(self._target.get("process_id") or 0)
                or int(target.get("window_handle") or 0) != int(self._target.get("window_handle") or 0)
            ):
                raise UiControlHostError(
                    "invalid_target",
                    "The completed recording does not reference the capability-bound target.",
                )
            artifact = response.get("artifact")
            digest = artifact.get("manifest_sha256") if isinstance(artifact, dict) else None
            if (
                not isinstance(artifact, dict)
                or int(artifact.get("frame_count") or 0) <= 0
                or int(artifact.get("frames_per_second") or 0) != frames_per_second
                or not isinstance(digest, str)
                or len(digest) != 64
                or any(character not in "0123456789abcdef" for character in digest)
            ):
                raise UiControlHostError(
                    "capture_failed",
                    "The exact-window recording artifact descriptor is invalid.",
                )
            self._target = dict(target)
            return response
        finally:
            self._invalidate_observation()

    def change_window_state(self, operation: str) -> Dict[str, Any]:
        """Apply one bounded non-input transition to the exact HWND."""
        if operation not in {"restore", "show", "activate"}:
            raise UiControlHostError("invalid_request", "Unsupported exact-window state operation.")
        try:
            return self._call(
                {
                    "method": "change_window_state",
                    "params": {**self._authority(), "operation": operation},
                },
                expected_type="window_state_changed",
            )
        finally:
            self._invalidate_observation()

    def execute(self, action: Dict[str, Any]) -> Dict[str, Any]:
        """Execute one action against the latest two host fences."""
        if not self._latest_observation_id or not self._latest_accessibility_state_id:
            raise UiControlHostError("stale_observation", "Take a fresh ui_control snapshot before acting.")
        try:
            response = self._call(
                {
                    "method": "execute_action",
                    "params": {
                        **self._authority(),
                        "observation_id": self._latest_observation_id,
                        "accessibility_state_id": self._latest_accessibility_state_id,
                        "action": action,
                    },
                },
                expected_type="action_completed",
                allow_unsuccessful=True,
            )
            if bool(response.get("target_closed")):
                self._window_capability = None
                with suppress(OSError):
                    self._stream.close()
            return response
        finally:
            self._invalidate_observation()

    def resume(self) -> None:
        """Ask the trusted host UI to clear the global stop latch."""
        self._call(
            {"method": "resume_session", "params": self._authority()},
            expected_type="session_resumed",
        )
        self._invalidate_observation()

    def stop(self) -> Dict[str, Any]:
        """Stop the host session and invalidate every local capability."""
        try:
            if self._window_capability is None:
                return {"type": "session_stopped", "session_id": self.session_id, "cleanup_pending": False}
            return self._call(
                {"method": "stop_session", "params": {"session_id": self.session_id}},
                expected_type="session_stopped",
            )
        finally:
            self._window_capability = None
            self._invalidate_observation()
            with suppress(OSError):
                self._stream.close()

    def _authority(self) -> Dict[str, Any]:
        if self._window_capability is None:
            raise UiControlHostError("backend_unavailable", "The UI Control host session is closed.")
        return {
            "session_id": self.session_id,
            "task_grant_id": self.task_grant_id,
            "window_capability": self._window_capability,
        }

    def _invalidate_observation(self) -> None:
        self._latest_observation_id = None
        self._latest_accessibility_state_id = None

    def _call(
        self,
        request: Dict[str, Any],
        *,
        expected_type: str,
        allow_unsuccessful: bool = False,
    ) -> Dict[str, Any]:
        with self._lock:
            return _exchange(
                self._stream,
                request,
                expected_type=expected_type,
                allow_unsuccessful=allow_unsuccessful,
            )
