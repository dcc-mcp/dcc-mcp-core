"""Persistent process bridge to the standalone ``dcc-cua`` CLI."""

from __future__ import annotations

from contextlib import suppress
import ctypes
import ctypes.util
from functools import lru_cache
import json
import mmap
import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from typing import Any
from typing import Dict
from typing import List
from typing import Mapping
from typing import NamedTuple
from typing import Optional
from typing import Sequence
from typing import Tuple

from dcc_mcp_core.constants import ENV_CUA_BINARY
from dcc_mcp_core.constants import ENV_MCP_INSTALL_DIR
from dcc_mcp_core.errors import DccMcpError

CUA_BINARY_ENV = ENV_CUA_BINARY
MCP_INSTALL_DIR_ENV = ENV_MCP_INSTALL_DIR
MINIMUM_CUA_VERSION = (0, 4, 0)
MINIMUM_CUA_VERSION_TEXT = ".".join(str(part) for part in MINIMUM_CUA_VERSION)
_MANIFEST_TIMEOUT_SECONDS = 5.0
_ENSURE_TIMEOUT_SECONDS = 20.0
_DEFAULT_CALL_TIMEOUT_SECONDS = 35.0
_MAX_MANIFEST_BYTES = 256 * 1024
_MAX_IMAGE_BYTES = 512 * 1024 * 1024
_EOF = object()
_BINARY_OUTPUT_NAME = re.compile(r"response-[0-9]+[.]bin")
_SHARED_IMAGE_NAME = re.compile(r"cua_([0-9a-f]{16})")
_SHARED_IMAGE_HEADER_BYTES = 48
_SHARED_IMAGE_MAGIC = 0x4355_4100_5348_4D01
_STABLE_SEMVER = re.compile(r"^(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)(?:[+][0-9A-Za-z.-]+)?$")


class CuaCliError(DccMcpError, RuntimeError):
    """Structured failure at the standalone CUA process boundary."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class CuaCliContract(NamedTuple):
    """Validated commands published by one standalone CLI manifest."""

    command: Tuple[str, ...]
    version: str
    protocol_version: int
    ensure_command: Tuple[str, ...]
    bridge_command: Tuple[str, ...]
    snapshot_transports: Tuple[str, ...]
    capabilities: Tuple[str, ...]
    parallel_discovery: bool


def resolve_cua_command(configured: Optional[str] = None) -> List[str]:
    """Resolve an explicit binary or a verified local ``dcc-cua`` installation."""
    value = (configured if configured is not None else os.environ.get(CUA_BINARY_ENV, "")).strip()
    if value:
        candidate = Path(value).expanduser()
        if not candidate.is_absolute():
            raise CuaCliError("backend_unavailable", f"{CUA_BINARY_ENV} must be an absolute path.")
        if not candidate.is_file():
            raise CuaCliError("backend_unavailable", f"{CUA_BINARY_ENV} does not name a file.")
        return [str(candidate.resolve())]
    discovered = _resolve_cli_sibling()
    if not discovered:
        discovered = next((str(path) for path in _standard_cua_candidates() if path.is_file()), None)
    if not discovered:
        discovered = _resolve_versioned_cua_install()
    if not discovered:
        discovered = shutil.which("dcc-cua")
    if not discovered:
        raise CuaCliError(
            "backend_unavailable",
            f"Install dcc-cua or set {CUA_BINARY_ENV} to its absolute path.",
        )
    return [str(Path(discovered).resolve())]


def _resolve_cli_sibling() -> Optional[str]:
    """Prefer the component installed beside the official ``dcc-mcp-cli``."""
    cli = shutil.which("dcc-mcp-cli")
    if not cli:
        return None
    file_name = "dcc-cua.exe" if os.name == "nt" else "dcc-cua"
    candidate = Path(cli).resolve().parent / file_name
    return str(candidate) if candidate.is_file() else None


def _cua_executable_name() -> str:
    return "dcc-cua.exe" if os.name == "nt" else "dcc-cua"


def _standard_cua_candidates() -> Tuple[Path, ...]:
    """Return bounded candidates used by the official dcc-mcp installers."""
    directories = []
    configured_dir = os.environ.get(MCP_INSTALL_DIR_ENV, "").strip()
    if configured_dir:
        configured_path = Path(configured_dir).expanduser()
        if configured_path.is_absolute():
            directories.append(configured_path)
    if os.name == "nt":
        local_app_data = os.environ.get("LOCALAPPDATA", "").strip()
        if local_app_data:
            directories.append(Path(local_app_data) / "dcc-mcp" / "bin")
    else:
        directories.append(Path.home() / ".local" / "bin")

    name = _cua_executable_name()
    candidates = []
    seen = set()
    for directory in directories:
        candidate = directory / name
        key = os.path.normcase(str(candidate.resolve()))
        if key not in seen:
            seen.add(key)
            candidates.append(candidate)
    return tuple(candidates)


def _standalone_cua_versions_dir() -> Optional[Path]:
    """Return the platform data directory used by versioned dcc-cua installs."""
    if os.name == "nt":
        windows_root = os.environ.get("LOCALAPPDATA", "").strip()
        return Path(windows_root) / "dcc-cua" / "versions" if windows_root else None
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "dcc-cua" / "versions"
    xdg_data_home = os.environ.get("XDG_DATA_HOME", "").strip()
    unix_root = Path(xdg_data_home).expanduser() if xdg_data_home else Path.home() / ".local" / "share"
    return unix_root / "dcc-cua" / "versions"


def _resolve_versioned_cua_install() -> Optional[str]:
    versions_dir = _standalone_cua_versions_dir()
    if versions_dir is None or not versions_dir.is_dir():
        return None
    installed = []
    try:
        version_directories = tuple(versions_dir.iterdir())
    except OSError:
        return None
    for version_dir in version_directories:
        version = _stable_version_tuple(version_dir.name)
        candidate = version_dir / _cua_executable_name()
        if version >= MINIMUM_CUA_VERSION and candidate.is_file():
            installed.append((version, version_dir.name, candidate))
    if not installed:
        return None
    return str(max(installed, key=lambda item: (item[0], item[1]))[2])


def inspect_cua_contract(command: Sequence[str]) -> CuaCliContract:
    """Read and validate the CLI-owned Core integration contract."""
    if not command or any(not isinstance(part, str) or not part for part in command):
        raise CuaCliError("backend_unavailable", "The dcc-cua command is invalid.")
    result = _run_json_command([*command, "manifest"], _MANIFEST_TIMEOUT_SECONDS)
    if result.get("name") != "dcc-cua" or result.get("schema_version") != 1:
        raise CuaCliError("protocol_mismatch", "dcc-cua returned an unsupported manifest.")
    host = result.get("host")
    bridge = result.get("core_bridge")
    runtime = result.get("runtime")
    if not isinstance(host, dict) or not isinstance(bridge, dict) or not isinstance(runtime, dict):
        raise CuaCliError("protocol_mismatch", "dcc-cua manifest is missing Core integration fields.")
    if runtime.get("separate_driver_required") is not False:
        raise CuaCliError("protocol_mismatch", "dcc-cua must embed its CUA runtime.")
    version = str(result.get("version") or "")
    if _stable_version_tuple(version) < MINIMUM_CUA_VERSION:
        raise CuaCliError(
            "protocol_mismatch",
            f"dcc-cua {MINIMUM_CUA_VERSION_TEXT} or newer is required; found {version or 'unknown'}.",
        )
    protocol_version = host.get("protocol_version")
    ensure_command = _string_command(host.get("ensure_command"), "host.ensure_command")
    bridge_command = _string_command(bridge.get("command"), "core_bridge.command")
    snapshot_transports = _string_command(host.get("snapshot_transports"), "host.snapshot_transports")
    if (
        protocol_version != 1
        or ensure_command[0] != "host-ensure"
        or bridge_command != ("host-jsonl",)
        or not set(snapshot_transports).issubset({"binary_frame", "shared_memory"})
    ):
        raise CuaCliError("protocol_mismatch", "dcc-cua Host protocol is incompatible with Core.")
    capabilities = host.get("capabilities")
    if not isinstance(capabilities, list) or any(not isinstance(item, str) or not item for item in capabilities):
        raise CuaCliError("protocol_mismatch", "dcc-cua manifest capabilities are invalid.")
    return CuaCliContract(
        command=tuple(command),
        version=version,
        protocol_version=protocol_version,
        ensure_command=ensure_command,
        bridge_command=bridge_command,
        snapshot_transports=snapshot_transports,
        capabilities=tuple(capabilities),
        parallel_discovery="parallel_discovery_requests" in capabilities,
    )


class CuaCliBridge:
    """One serialized JSONL connection to a shared standalone CUA Host."""

    def __init__(
        self,
        command: Optional[Sequence[str]] = None,
        *,
        shared_host: bool = True,
        environment: Optional[Mapping[str, str]] = None,
        snapshot_transport: Optional[str] = None,
    ) -> None:
        resolved = list(command) if command is not None else resolve_cua_command()
        self.contract = inspect_cua_contract(resolved)
        self._environment = dict(os.environ if environment is None else environment)
        self._lock = threading.RLock()
        self._responses = queue.Queue()  # type: queue.Queue[Any]
        self._request_index = 0
        self._closed = False
        preferred_transport = "shared_memory" if os.name in {"nt", "posix"} else "binary_frame"
        self.snapshot_transport = snapshot_transport or preferred_transport
        if self.snapshot_transport not in self.contract.snapshot_transports:
            raise CuaCliError("protocol_mismatch", "dcc-cua does not support the required snapshot transport.")
        self._output_directory = None
        self.host_status: Optional[Dict[str, Any]] = None

        bridge_command = [
            *self.contract.command,
            *self.contract.bridge_command,
            "--snapshot-transport",
            self.snapshot_transport,
        ]
        if self.snapshot_transport == "binary_frame":
            self._output_directory = tempfile.TemporaryDirectory(prefix="dcc-cua-core-")
            bridge_command.extend(["--output-dir", self._output_directory.name])
        if shared_host:
            self.host_status = self._ensure_host()
        else:
            if len(self.contract.command) != 1:
                raise CuaCliError("invalid_request", "Managed Host mode requires one executable path.")
            bridge_command.extend(["--spawn", self.contract.command[0]])
        if self.contract.parallel_discovery:
            bridge_command.append("--parallel-discovery")
        self._process = self._start_bridge(bridge_command)
        self._reader = threading.Thread(
            target=self._read_responses,
            name="dcc-cua-jsonl-reader",
            daemon=True,
        )
        self._reader.start()

    def read_image(self, response: Dict[str, Any]) -> bytes:
        """Read and validate one image returned by the negotiated transport."""
        image = response.get("image")
        if not isinstance(image, dict) or image.get("encoding") != self.snapshot_transport:
            raise CuaCliError("capture_failed", "dcc-cua returned an invalid image descriptor.")
        expected_length = image.get("length")
        if not isinstance(expected_length, int) or not 0 < expected_length <= _MAX_IMAGE_BYTES:
            raise CuaCliError("capture_failed", "dcc-cua returned an invalid image length.")
        if self.snapshot_transport == "shared_memory":
            try:
                pixels = _read_shared_image(image)
            except Exception as exc:
                raise CuaCliError("capture_failed", f"Cannot read the CUA screenshot buffer: {exc}") from exc
        else:
            pixels = self._read_binary_output(response)
        if len(pixels) != expected_length:
            raise CuaCliError("capture_failed", "The CUA screenshot length is invalid.")
        return pixels

    def call(
        self,
        method: str,
        params: Optional[Dict[str, Any]] = None,
        *,
        request_id: Optional[str] = None,
        timeout: float = _DEFAULT_CALL_TIMEOUT_SECONDS,
    ) -> Dict[str, Any]:
        """Send one bounded Host request and return its correlated response."""
        if not isinstance(method, str) or not method:
            raise CuaCliError("invalid_request", "CUA method must be a non-empty string.")
        arguments = {} if params is None else params
        if not isinstance(arguments, dict):
            raise CuaCliError("invalid_request", "CUA params must be an object.")
        with self._lock:
            self._require_running()
            correlation = request_id or self._next_request_id()
            request = {"request_id": correlation, "method": method, "params": arguments}
            try:
                self._process.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
                self._process.stdin.flush()
            except (AttributeError, OSError, ValueError):
                self._close_process()
                raise CuaCliError("transport_error", "Cannot write to the dcc-cua bridge.") from None
            try:
                response = self._responses.get(timeout=timeout)
            except queue.Empty:
                self._close_process()
                raise CuaCliError("timeout", f"dcc-cua did not answer {method!r} in time.") from None
            if response is _EOF:
                self._close_process()
                raise CuaCliError("transport_error", "dcc-cua closed the JSONL bridge.")
            if isinstance(response, CuaCliError):
                self._close_process()
                raise response
            if response.get("request_id") != correlation:
                self._close_process()
                raise CuaCliError("protocol_mismatch", "dcc-cua returned a mismatched request_id.")
            if response.get("type") == "error":
                raise CuaCliError(
                    str(response.get("code") or "backend_unavailable"),
                    str(response.get("message") or "dcc-cua rejected the request."),
                )
            return response

    def close(self) -> None:
        """Close the bridge; Host cleans every session owned by this connection."""
        with self._lock:
            if self._closed:
                return
            self._closed = True
            with suppress(OSError, ValueError):
                self._process.stdin.close()
            try:
                self._process.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                self._process.terminate()
                try:
                    self._process.wait(timeout=2.0)
                except subprocess.TimeoutExpired:
                    self._process.kill()
                    self._process.wait(timeout=2.0)
            with suppress(OSError, ValueError):
                self._process.stdout.close()
            self._cleanup_output_directory()

    def __enter__(self) -> CuaCliBridge:
        return self

    def __exit__(self, _exc_type: Any, _exc: Any, _traceback: Any) -> None:
        self.close()

    def _ensure_host(self) -> Dict[str, Any]:
        response = _run_json_command(
            [*self.contract.command, *self.contract.ensure_command],
            _ENSURE_TIMEOUT_SECONDS,
            environment=self._environment,
        )
        if (
            response.get("type") != "host_ready"
            or response.get("status") not in {"started", "existing"}
            or response.get("protocol_version") != self.contract.protocol_version
        ):
            raise CuaCliError("protocol_mismatch", "dcc-cua did not establish a compatible Host.")
        return response

    def _start_bridge(self, command: Sequence[str]) -> Any:
        try:
            return subprocess.Popen(
                list(command),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                universal_newlines=True,
                encoding="utf-8",
                bufsize=1,
                close_fds=True,
                env=self._environment,
                creationflags=_no_window_flags(),
            )
        except (OSError, ValueError):
            raise CuaCliError("backend_unavailable", "Cannot start the dcc-cua JSONL bridge.") from None

    def _read_responses(self) -> None:
        try:
            for line in self._process.stdout:
                try:
                    response = json.loads(line)
                except (TypeError, ValueError):
                    self._responses.put(CuaCliError("protocol_mismatch", "dcc-cua returned invalid JSONL."))
                    return
                if not isinstance(response, dict):
                    self._responses.put(CuaCliError("protocol_mismatch", "dcc-cua returned a non-object."))
                    return
                self._responses.put(response)
        except (OSError, ValueError):
            pass
        finally:
            self._responses.put(_EOF)

    def _next_request_id(self) -> str:
        self._request_index += 1
        return f"dcc-core-cua-{self._request_index}"

    def _require_running(self) -> None:
        if self._closed or self._process.poll() is not None:
            raise CuaCliError("transport_error", "The dcc-cua JSONL bridge is not running.")

    def _close_process(self) -> None:
        if self._closed:
            return
        self._closed = True
        with suppress(OSError, ValueError):
            self._process.terminate()
        with suppress(OSError, subprocess.TimeoutExpired, ValueError):
            self._process.wait(timeout=2.0)
        self._cleanup_output_directory()

    def _read_binary_output(self, response: Dict[str, Any]) -> bytes:
        if self._output_directory is None:
            raise CuaCliError("capture_failed", "The CUA binary output directory is unavailable.")
        raw_path = response.get("_dcc_cua_binary_output")
        if raw_path is None:
            raw_path = response.get("_dcc_mcp_binary_output")
        if not isinstance(raw_path, str):
            raise CuaCliError("capture_failed", "dcc-cua returned no binary image output.")
        output_root = Path(self._output_directory.name).resolve()
        output_path = Path(raw_path).resolve()
        if output_path.parent != output_root or not _BINARY_OUTPUT_NAME.fullmatch(output_path.name):
            raise CuaCliError("capture_failed", "dcc-cua returned an unsafe binary image path.")
        try:
            return output_path.read_bytes()
        except OSError as exc:
            raise CuaCliError("capture_failed", f"Cannot read the CUA screenshot output: {exc}") from exc
        finally:
            with suppress(OSError):
                output_path.unlink()

    def _cleanup_output_directory(self) -> None:
        if self._output_directory is not None:
            self._output_directory.cleanup()
            self._output_directory = None


def _string_command(value: Any, field: str) -> Tuple[str, ...]:
    if not isinstance(value, list) or not value or any(not isinstance(item, str) or not item for item in value):
        raise CuaCliError("protocol_mismatch", f"dcc-cua manifest field {field} is invalid.")
    return tuple(value)


def _stable_version_tuple(value: str) -> Tuple[int, int, int]:
    match = _STABLE_SEMVER.fullmatch(value)
    if match is None:
        return (-1, -1, -1)
    return tuple(int(part) for part in match.groups())


def _run_json_command(
    command: Sequence[str],
    timeout: float,
    *,
    environment: Optional[Mapping[str, str]] = None,
) -> Dict[str, Any]:
    try:
        result = subprocess.run(
            list(command),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            timeout=timeout,
            check=False,
            close_fds=True,
            env=None if environment is None else dict(environment),
            creationflags=_no_window_flags(),
        )
    except (OSError, subprocess.TimeoutExpired, UnicodeError, ValueError):
        raise CuaCliError("backend_unavailable", f"Cannot run dcc-cua command {command[-1]!r}.") from None
    if result.returncode != 0 or len(result.stdout.encode("utf-8")) > _MAX_MANIFEST_BYTES:
        raise CuaCliError("backend_unavailable", f"dcc-cua command {command[-1]!r} failed.")
    try:
        value = json.loads(result.stdout)
    except (TypeError, ValueError):
        raise CuaCliError("protocol_mismatch", f"dcc-cua command {command[-1]!r} returned invalid JSON.") from None
    if not isinstance(value, dict):
        raise CuaCliError("protocol_mismatch", f"dcc-cua command {command[-1]!r} returned a non-object.")
    return value


def _no_window_flags() -> int:
    return getattr(subprocess, "CREATE_NO_WINDOW", 0) if os.name == "nt" else 0


def _read_shared_image(descriptor: Dict[str, Any]) -> bytes:
    name = descriptor.get("name")
    image_id = descriptor.get("id")
    length = descriptor.get("length")
    match = _SHARED_IMAGE_NAME.fullmatch(name) if isinstance(name, str) else None
    if (
        match is None
        or not isinstance(image_id, str)
        or image_id != match.group(1)
        or not isinstance(length, int)
        or not 0 < length <= _MAX_IMAGE_BYTES
    ):
        raise ValueError("invalid CUA shared image descriptor")
    total = _SHARED_IMAGE_HEADER_BYTES + length
    memory = _open_shared_memory(name, total)
    try:
        header = memory[:_SHARED_IMAGE_HEADER_BYTES]
        pixels = bytes(memory[_SHARED_IMAGE_HEADER_BYTES:total])
    finally:
        memory.close()
    fields = [int.from_bytes(header[offset : offset + 8], byteorder=sys.byteorder) for offset in range(0, 40, 8)]
    magic, capacity, written, created_at, ttl = fields
    now = int(time.time())
    if (
        magic != _SHARED_IMAGE_MAGIC
        or capacity != length
        or written != length
        or created_at > now
        or ttl <= 0
        or now - created_at > ttl
    ):
        raise ValueError("CUA shared image header does not match its descriptor")
    return pixels


def _open_shared_memory(name: str, size: int) -> mmap.mmap:
    if os.name == "nt":
        return mmap.mmap(-1, size, tagname=name, access=mmap.ACCESS_READ)
    if os.name != "posix":
        raise OSError("shared memory is unsupported on this platform")
    encoded_name = (name if name.startswith("/") else f"/{name}").encode("ascii")
    library = _posix_shm_library()
    shm_open = library.shm_open
    shm_open.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_uint]
    shm_open.restype = ctypes.c_int
    descriptor = shm_open(encoded_name, os.O_RDONLY, 0)
    if descriptor < 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), name)
    try:
        return mmap.mmap(descriptor, size, access=mmap.ACCESS_READ)
    finally:
        os.close(descriptor)


@lru_cache(maxsize=1)
def _posix_shm_library() -> Any:
    candidates = [None, ctypes.util.find_library("rt"), ctypes.util.find_library("c")]
    for candidate in dict.fromkeys(candidates):
        try:
            library = ctypes.CDLL(candidate, use_errno=True)
            _ = library.shm_open
            return library
        except (AttributeError, OSError):
            continue
    raise OSError("the platform does not expose shm_open")
