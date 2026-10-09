"""Bounded stdlib MCP stdio transport for one integrity-pinned owned process."""

from __future__ import annotations

from contextlib import suppress
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import threading
import time
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions

_MAX_LINE_BYTES = 96 * 1024 * 1024
_EOF = object()


class OwnedCuaMcpTransport:
    """Own exactly ``binary mcp-server``; never discover or ensure a shared Host.

    Requests are serialized and never retried. A timeout or protocol mismatch
    closes this process, so an uncertain mutation cannot be replayed. The one
    reader thread only drains this child's stdout; it owns no UI or server.
    """

    def __init__(self, options: UiControlRuntimeOptions) -> None:
        self.options = options
        self._lock = threading.RLock()
        self._closed = False
        self._cleanup_error: str | None = None
        self._reader_output_error = False
        self._index = 0
        self._responses = queue.Queue(maxsize=16)
        path = Path(options.binary)
        try:
            digest = hashlib.sha256()
            with path.open("rb") as source:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(block)
        except OSError:
            raise CuaCliError("backend_unavailable", "Cannot read the selected UI Control executable.") from None
        if digest.hexdigest() != options.sha256:
            raise CuaCliError("integrity_mismatch", "The selected UI Control executable failed SHA-256 verification.")
        try:
            child_env = os.environ.copy()
            child_env.pop("DCC_CUA_RECORDING_OUTPUT_ROOT", None)
            child_env.pop("DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT", None)
            if options.capture_preparation is not None:
                child_env["DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT"] = options.capture_preparation.journal_root
            if options.recording is not None:
                child_env["DCC_CUA_RECORDING_OUTPUT_ROOT"] = options.recording.output_root
            self._process = subprocess.Popen(
                [str(path), "mcp-server"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                close_fds=True,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
                env=child_env,
            )
        except (OSError, ValueError):
            raise CuaCliError("backend_unavailable", "Cannot start the selected UI Control MCP runtime.") from None
        self._reader = threading.Thread(target=self._read, name="dcc-cua-owned-mcp-reader", daemon=True)
        self._reader.start()
        try:
            initialized = self.rpc(
                "initialize",
                {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "dcc-mcp-core-ui-control", "version": "1"},
                },
            )
            info = initialized.get("serverInfo") or {}
            capabilities = initialized.get("capabilities")
            if (
                initialized.get("protocolVersion") != "2025-06-18"
                or not isinstance(info, dict)
                or info.get("name") != "dcc-cua-task-automation"
                or info.get("version") != options.runtime_version
                or not isinstance(capabilities, dict)
                or not isinstance(capabilities.get("tools"), dict)
            ):
                raise CuaCliError("protocol_mismatch", "The owned UI Control MCP identity/version does not match.")
            self._write({"jsonrpc": "2.0", "method": "notifications/initialized"})
            catalog = self.rpc("tools/list", {})
            tools = catalog.get("tools")
            if not isinstance(tools, list) or not _public_tool_schemas_valid(
                tools, recording=options.recording is not None, preparation=options.capture_preparation is not None
            ):
                raise CuaCliError("protocol_mismatch", "The owned runtime lacks the public bounded-task MCP tools.")
        except Exception:
            with suppress(Exception):
                self.close()
            raise

    def _read(self) -> None:
        try:
            while True:
                line = self._process.stdout.readline(_MAX_LINE_BYTES + 1)
                if not line:
                    break
                if len(line) > _MAX_LINE_BYTES or not line.endswith(b"\n"):
                    raise ValueError("invalid MCP line")
                value = json.loads(line.decode("utf-8"))
                if not isinstance(value, dict) or value.get("jsonrpc") != "2.0":
                    raise ValueError("invalid MCP object")
                if "id" not in value and str(value.get("method", "")).startswith("notifications/"):
                    continue
                self._responses.put_nowait(value)
        except (OSError, ValueError, queue.Full):
            with suppress(queue.Full):
                self._responses.put_nowait(CuaCliError("protocol_mismatch", "Invalid owned MCP response."))
        finally:
            # Only the reader may close its BufferedReader. Closing it from a
            # different thread can wait forever on a blocked readline's lock.
            try:
                self._process.stdout.close()
            except Exception:
                self._reader_output_error = True
            with suppress(queue.Full):
                self._responses.put_nowait(_EOF)

    def _write(self, message: dict[str, Any]) -> None:
        try:
            self._process.stdin.write((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))
            self._process.stdin.flush()
        except (OSError, ValueError, AttributeError):
            with suppress(Exception):
                self.close()
            raise CuaCliError("transport_error", "Cannot write to the owned UI Control runtime.") from None

    def rpc(self, method: str, params: dict[str, Any], *, timeout: float | None = None) -> dict[str, Any]:
        """Perform one correlated MCP request without retries."""
        with self._lock:
            if self._closed or self._process.poll() is not None:
                raise CuaCliError("transport_error", "The owned UI Control runtime is closed.")
            self._index += 1
            request_id = self._index
            self._write({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
            wait_seconds = self.options.timeout_seconds if timeout is None else timeout
            deadline = time.monotonic() + wait_seconds
            try:
                response = self._responses.get(timeout=max(0, min(wait_seconds, deadline - time.monotonic())))
                if response is _EOF:
                    raise CuaCliError("transport_error", "The owned UI Control runtime closed its output.")
                if isinstance(response, Exception):
                    raise response
                if type(response.get("id")) is not int or response["id"] != request_id:
                    raise CuaCliError("protocol_mismatch", "The owned MCP response id does not match.")
                if "error" in response:
                    raise CuaCliError("runtime_rejected", "The owned MCP runtime rejected the request.")
                if not isinstance(response.get("result"), dict):
                    raise CuaCliError("protocol_mismatch", "The owned MCP response has no object result.")
                return response["result"]
            except queue.Empty:
                with suppress(Exception):
                    self.close(grace_seconds=0 if timeout is not None else None)
                raise CuaCliError("timeout", "The owned UI Control request timed out; it was not retried.") from None
            except CuaCliError as exc:
                if exc.code != "runtime_rejected":
                    with suppress(Exception):
                        self.close()
                raise

    def tool(self, name: str, arguments: dict[str, Any], *, cleanup: bool = False) -> dict[str, Any]:
        """Call one public tool and preserve its structured data and image blocks."""
        recording = self.options.recording
        finalizing = cleanup or (name == "dcc_cua_task_call" and arguments.get("method") == "recording_stop")
        timeout = (recording.cleanup_timeout_seconds if recording is not None else 5) if finalizing else None
        result = self.rpc("tools/call", {"name": name, "arguments": arguments}, timeout=timeout)
        payload = result.get("structuredContent")
        if not isinstance(payload, dict):
            with suppress(Exception):
                self.close()
            raise CuaCliError("protocol_mismatch", "The owned MCP tool omitted structuredContent.")
        # Cleanup failures are protocol results, not permission to erase the
        # native acknowledgement or retry a potentially completed stop.
        if not cleanup and (result.get("isError") or payload.get("type") == "error" or payload.get("ok") is False):
            raise OwnedCuaMcpError(
                str(payload.get("code") or "runtime_rejected"),
                str(payload.get("message") or payload.get("error") or "UI Control request rejected."),
                payload,
            )
        return result

    def close(self, *, grace_seconds: float | None = None) -> None:
        """Close and, if necessary, terminate only this transport's owned child."""
        with self._lock:
            if self._closed:
                if self._cleanup_error is not None:
                    raise CuaCliError("cleanup_failed", self._cleanup_error)
                return
            self._closed = True
            try:
                self._process.stdin.close()
            except Exception:
                self._cleanup_error = "The owned UI Control runtime input did not close cleanly."
            try:
                recording = self.options.recording
                grace = recording.cleanup_timeout_seconds if recording is not None else 5
                self._process.wait(timeout=grace if grace_seconds is None else max(0, min(grace, grace_seconds)))
            except Exception as exc:
                self._cleanup_error = (
                    "The owned UI Control runtime required forced termination; cleanup is unverified."
                    if isinstance(exc, subprocess.TimeoutExpired)
                    else "The owned UI Control runtime could not acknowledge process exit."
                )
                try:
                    self._process.terminate()
                    self._process.wait(timeout=2)
                except Exception:
                    try:
                        self._process.kill()
                        self._process.wait(timeout=2)
                    except Exception:
                        self._cleanup_error = "The owned UI Control runtime did not acknowledge forced process exit."
            finally:
                try:
                    self._reader.join(timeout=1)
                except Exception:
                    self._cleanup_error = "The owned UI Control response reader did not finish cleanly."
                try:
                    if self._reader_output_error:
                        self._cleanup_error = "The owned UI Control runtime output did not close cleanly."
                    if self._reader.is_alive() or self._process.poll() != 0:
                        self._cleanup_error = (
                            self._cleanup_error or "The owned UI Control runtime did not finish cleanly."
                        )
                except Exception:
                    self._cleanup_error = "The owned UI Control runtime exit state is unknown."
            if self._cleanup_error is not None:
                raise CuaCliError("cleanup_failed", self._cleanup_error)


def _public_tool_schemas_valid(tools: list[Any], *, recording: bool = False, preparation: bool = False) -> bool:
    """Check the public envelope we consume, without interpreting task leases."""
    schemas = {item.get("name"): item.get("inputSchema") for item in tools if isinstance(item, dict)}
    expected = {
        "start_task": {
            "application_label": "string",
            "target_process_id": "integer",
            "target_window_handle": "integer",
            "surface": "string",
            "observation_mode": "string",
            "allowed_methods": "array",
            "allowed_actions": "array",
            "ttl_minutes": "integer",
        },
        "dcc_cua_task_call": {"task_id": "string", "method": "string", "params": "object"},
        "task_status": {"task_id": "string"},
        "stop_task": {"task_id": "string"},
    }
    if recording:
        expected["start_task"]["allow_recording"] = "boolean"
    if preparation:
        expected["start_task"]["allow_capture_preparation"] = "boolean"
    for name, properties in expected.items():
        schema = schemas.get(name)
        if (
            not isinstance(schema, dict)
            or schema.get("type") != "object"
            or schema.get("additionalProperties") is not False
        ):
            return False
        declared = schema.get("properties")
        if not isinstance(declared, dict) or any(
            not isinstance(declared.get(key), dict) or declared[key].get("type") != kind
            for key, kind in properties.items()
        ):
            return False
        required = schema.get("required")
        required_keys = (
            {"application_label", "surface", "allowed_methods", "allowed_actions"}
            if name == "start_task"
            else set(properties)
        )
        if not isinstance(required, list) or not required_keys.issubset(required):
            return False
    mode = schemas["start_task"]["properties"]["observation_mode"].get("enum")
    return isinstance(mode, list) and "pixels_only" in mode
