"""Async subprocess session for the curated official Framer SDK facade.

Usage: async with FramerNodeSession(project=existing_project) as session:
    facade = FramerApiFacade(list_tools=session.list_tools,
                             call_tool=session.call_tool)
The operator installs framer-api and supplies existing FRAMER_API_KEY credentials.
No credentials are read by Python, copied into arguments, or logged. The local
JSON-lines protocol is ours; MCP result framing is only for the Core callback.
"""

from __future__ import annotations

import asyncio
import json
import math
import os
from pathlib import Path
from typing import Any
import uuid

_MESSAGES = {
    "project_required": "An existing Framer project ID or URL is required.",
    "auth_required": "An existing FRAMER_API_KEY is required.",
    "permission_denied": "The existing Framer authorization denied this operation.",
    "dependency_missing": "The official framer-api package must already be installed.",
    "sdk_incompatible": "The installed SDK is incompatible with this facade.",
    "connection_failed": "The official SDK could not connect to the existing project.",
    "session_closed": "The Framer SDK session is closed or unusable.",
    "tool_unavailable": "This method is unavailable in the curated SDK session.",
    "invalid_arguments": "Arguments do not match the curated SDK method schema.",
    "invalid_request": "The internal adapter rejected the request.",
    "upstream_error": "The official SDK call failed.",
    "adapter_error": "The internal Framer SDK adapter failed.",
    "result_not_serializable": "The SDK result cannot be represented as JSON.",
    "disconnect_failed": "The official SDK did not confirm disconnection.",
    "node_unavailable": "The configured Node executable is unavailable.",
    "startup_timeout": "SDK connection timed out; the session was stopped.",
    "request_timeout": "SDK call timed out; its outcome is unknown and it was not replayed.",
    "protocol_error": "The internal adapter returned an invalid response.",
    "response_id_mismatch": "The internal adapter response ID did not match the request.",
    "transport_closed": "The SDK adapter process closed its transport.",
}


class FramerAdapterError(ConnectionError):
    """A fixed public error without upstream exception messages or credentials."""

    def __init__(self, code: str) -> None:
        self.code = code if isinstance(code, str) and code in _MESSAGES else "upstream_error"
        self.status_code = {"auth_required": 401, "permission_denied": 403}.get(self.code)
        super().__init__(_MESSAGES[self.code])


class FramerNodeSession:
    """Manage one SDK connection, serialized requests, and explicit disconnect.

    Request IDs are checked before delivering results. Timeout, cancellation,
    malformed responses, and transport loss invalidate the session; a mutation
    is never retried automatically. ``facade_path`` supports hermetic tests and
    operator-owned wrappers; its default is the sibling production facade.
    """

    def __init__(
        self,
        *,
        project: str | None = None,
        node_command: str = "node",
        request_timeout: float = 30.0,
        startup_timeout: float = 30.0,
        facade_path: Path | None = None,
    ) -> None:
        self._project = project or os.environ.get("FRAMER_PROJECT") or os.environ.get("FRAMER_PROJECT_ID")
        self._node_command = node_command
        if (
            not math.isfinite(request_timeout)
            or not math.isfinite(startup_timeout)
            or request_timeout <= 0
            or startup_timeout <= 0
        ):
            raise ValueError("Session timeouts must be positive.")
        self._request_timeout = request_timeout
        self._startup_timeout = startup_timeout
        self._facade_path = (
            Path(facade_path) if facade_path is not None else Path(__file__).with_name("framer_facade.mjs")
        )
        self._process: asyncio.subprocess.Process | None = None
        self._lock: asyncio.Lock | None = None
        self._entered = False
        self._usable = False

    async def __aenter__(self) -> FramerNodeSession:
        if self._entered:
            raise FramerAdapterError("session_closed")
        self._entered = True
        if not isinstance(self._project, str) or not self._project.strip():
            raise FramerAdapterError("project_required")
        self._lock = asyncio.Lock()
        try:
            self._process = await asyncio.create_subprocess_exec(
                self._node_command,
                str(self._facade_path),
                "--project",
                self._project,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                # SDK logs are intentionally not collected or published.
                stderr=asyncio.subprocess.DEVNULL,
                limit=8 * 1024 * 1024,
            )
        except OSError:
            raise FramerAdapterError("node_unavailable") from None
        try:
            ready = await asyncio.wait_for(self._read_response(), self._startup_timeout)
            if ready.get("protocol") != "framer-sdk-jsonl/v1":
                raise FramerAdapterError("protocol_error")
            if ready.get("ready") is not True:
                raise FramerAdapterError(self._error_code(ready))
            self._usable = True
        except asyncio.TimeoutError:
            await self._stop_process()
            raise FramerAdapterError("startup_timeout") from None
        except BaseException:
            await self._stop_process()
            raise
        return self

    async def __aexit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        try:
            await self.close()
        except FramerAdapterError:
            if exc_type is None:
                raise

    @staticmethod
    def _error_code(response: dict[str, Any]) -> str:
        error = response.get("error")
        return error.get("code", "upstream_error") if isinstance(error, dict) else "upstream_error"

    async def _read_response(self) -> dict[str, Any]:
        if self._process is None or self._process.stdout is None:
            raise FramerAdapterError("session_closed")
        try:
            line = await self._process.stdout.readline()
            if not line:
                raise FramerAdapterError("transport_closed")
            response = json.loads(line)
        except (ValueError, UnicodeError):
            raise FramerAdapterError("protocol_error") from None
        if not isinstance(response, dict):
            raise FramerAdapterError("protocol_error")
        return response

    async def _request(self, method: str, **fields: Any) -> Any:
        if not self._usable or self._process is None or self._lock is None:
            raise FramerAdapterError("session_closed")
        async with self._lock:
            if not self._usable or self._process.stdin is None:
                raise FramerAdapterError("session_closed")
            request_id = uuid.uuid4().hex
            try:
                payload = (
                    json.dumps(dict(id=request_id, method=method, **fields), allow_nan=False).encode("utf-8") + b"\n"
                )
            except (TypeError, ValueError):
                raise FramerAdapterError("invalid_arguments") from None
            if len(payload) > 8 * 1024 * 1024:
                raise FramerAdapterError("invalid_arguments")
            try:
                self._process.stdin.write(payload)
                await asyncio.wait_for(self._process.stdin.drain(), self._request_timeout)
                response = await asyncio.wait_for(self._read_response(), self._request_timeout)
                if response.get("id") != request_id:
                    raise FramerAdapterError("response_id_mismatch")
                if "error" in response:
                    raise FramerAdapterError(self._error_code(response))
                if "result" not in response:
                    raise FramerAdapterError("protocol_error")
                return response["result"]
            except asyncio.TimeoutError:
                self._usable = False
                await self._stop_process()
                raise FramerAdapterError("request_timeout") from None
            except asyncio.CancelledError:
                self._usable = False
                await self._stop_process()
                raise
            except FramerAdapterError as error:
                if error.code not in {"invalid_arguments", "tool_unavailable"}:
                    self._usable = False
                    await self._stop_process(graceful=True)
                raise
            except (OSError, ConnectionError):
                self._usable = False
                await self._stop_process()
                raise FramerAdapterError("transport_closed") from None

    async def list_tools(self, cursor: str | None = None) -> dict[str, Any]:
        """Core callback: a live, single-page tools/list-shaped descriptor set."""
        result = await self._request("list_tools", cursor=cursor)
        if not isinstance(result, dict) or not isinstance(result.get("tools"), list):
            self._usable = False
            await self._stop_process()
            raise FramerAdapterError("protocol_error")
        return result

    async def call_tool(self, name: str, arguments: dict[str, Any] | None = None) -> dict[str, Any]:
        """Core callback: MCP-shaped result preserving the official SDK JSON.

        MCP structuredContent is an object. Arrays/null/scalars are retained
        under its ``result`` field; content text always contains the original JSON.
        Every SDK rejection raises a typed exception. The public SDK documentation
        does not establish that a rejected promise is a received business response;
        it may be a WebSocket failure with an unknown mutation outcome. Only normal
        SDK return values receive an MCP-shaped result for the Core callback.
        """
        result = await self._request("call_tool", name=name, arguments={} if arguments is None else arguments)
        return {
            "content": [{"type": "text", "text": json.dumps(result, ensure_ascii=False, allow_nan=False)}],
            "structuredContent": result if isinstance(result, dict) else {"result": result},
            "isError": False,
        }

    async def close(self) -> None:
        """Disconnect the SDK once, then wait for or terminate the child process."""
        try:
            if self._usable:
                await self._request("close")
        finally:
            self._usable = False
            await self._stop_process(graceful=True)

    async def _stop_process(self, graceful: bool = False) -> None:
        process = self._process
        if process is None or process.returncode is not None:
            return
        if process.stdin is not None:
            process.stdin.close()
        if not graceful:
            try:
                process.terminate()
            except ProcessLookupError:
                return
        try:
            await asyncio.wait_for(process.wait(), 2.0)
        except asyncio.TimeoutError:
            try:
                process.kill()
            except ProcessLookupError:
                return
            await process.wait()
