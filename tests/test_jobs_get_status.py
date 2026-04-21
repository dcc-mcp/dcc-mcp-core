"""End-to-end tests for the built-in ``jobs.get_status`` tool (issue #319).

The tool is always registered by ``McpHttpServer`` — regardless of which
skills are loaded — and is SEP-986 compliant (``validate_tool_name``).

Covers:

1. ``jobs.get_status`` is visible in ``tools/list`` with the expected
   SEP-986 name and ``ToolAnnotations`` (read-only, idempotent).
2. Calling ``jobs.get_status`` with an unknown ``job_id`` returns an
   ``isError=true`` ``CallToolResult`` — never a JSON-RPC transport error.
3. Dispatching an async tool via ``_meta.dcc.async=true`` produces a
   ``job_id``; polling that id transitions ``pending → running →
   completed`` and surfaces the final ``ToolResult`` in the envelope.
4. The SEP-986 naming validator accepts the name.
"""

from __future__ import annotations

import json
import time
from typing import Any
import urllib.request

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import validate_tool_name


def _post(url: str, body: dict[str, Any], sid: str | None = None) -> dict[str, Any]:
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    if sid is not None:
        headers["Mcp-Session-Id"] = sid
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        return json.loads(resp.read())


def _initialize_session(url: str) -> str:
    body = _post(
        url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest-319", "version": "1.0"},
            },
        },
    )
    return body["result"]["__session_id"]


def _make_server() -> tuple[Any, str]:
    reg = ToolRegistry()
    reg.register(
        "echo_tool",
        description="Simple echo used by #319 tests",
        category="test",
        tags=[],
        dcc="test",
        version="1.0.0",
    )
    cfg = McpHttpConfig(port=0, server_name="jobs-get-status-test")
    cfg.enable_job_notifications = True
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "echo_tool",
        lambda params: {"echoed": params, "ok": True},
    )
    handle = server.start()
    # Return both so the test can keep the server alive for the duration.
    return server, handle, handle.mcp_url()


def test_sep986_validate_tool_name_accepts_jobs_get_status():
    # Belt-and-braces check on the Python-exposed validator.
    validate_tool_name("jobs.get_status")


def test_jobs_get_status_listed_in_tools_list():
    _server, handle, url = _make_server()
    try:
        body = _post(
            url,
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        )
        tools = body["result"]["tools"]
        names = [t["name"] for t in tools]
        assert "jobs.get_status" in names, f"tools/list missing jobs.get_status: {names}"

        meta = next(t for t in tools if t["name"] == "jobs.get_status")
        ann = meta.get("annotations") or {}
        # Read-only + idempotent + non-destructive — polling a status must
        # never mutate anything server-side.
        assert ann.get("readOnlyHint") is True
        assert ann.get("idempotentHint") is True
        assert ann.get("destructiveHint") is False
        # Input schema has the three documented fields.
        props = meta["inputSchema"]["properties"]
        assert set(props.keys()) >= {"job_id", "include_logs", "include_result"}
        assert meta["inputSchema"]["required"] == ["job_id"]
    finally:
        handle.shutdown()


def test_jobs_get_status_unknown_id_returns_is_error_envelope():
    _server, handle, url = _make_server()
    try:
        body = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "jobs.get_status",
                    "arguments": {"job_id": "does-not-exist"},
                },
            },
        )
        assert "error" not in body, f"unknown job id must not produce a JSON-RPC transport error: {body}"
        assert body["result"]["isError"] is True
        text = body["result"]["content"][0]["text"]
        assert "does-not-exist" in text
    finally:
        handle.shutdown()


def test_jobs_get_status_polls_async_dispatch_to_completion():
    _server, handle, url = _make_server()
    try:
        sid = _initialize_session(url)
        # Opt into the async dispatch path (#318) — the server returns a
        # `{job_id, status: "pending"}` envelope instead of the result.
        body = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "echo_tool",
                    "arguments": {"hello": "world"},
                    "_meta": {"dcc": {"async": True}},
                },
            },
            sid=sid,
        )
        assert body["result"]["isError"] is False, body
        sc = body["result"].get("structuredContent") or json.loads(body["result"]["content"][0]["text"])
        job_id = sc["job_id"]
        assert isinstance(job_id, str) and job_id
        # Initial status is "pending" or "running" depending on timing.
        assert sc["status"] in {"pending", "running"}

        # Poll jobs.get_status until terminal. Guard with a hard timeout so
        # a regression doesn't hang the suite.
        deadline = time.monotonic() + 5.0
        final = None
        seen_statuses: set[str] = set()
        while time.monotonic() < deadline:
            poll = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "tools/call",
                    "params": {
                        "name": "jobs.get_status",
                        "arguments": {"job_id": job_id, "include_result": True},
                    },
                },
                sid=sid,
            )
            assert poll["result"]["isError"] is False, poll
            env = poll["result"]["structuredContent"]
            seen_statuses.add(env["status"])
            if env["status"] in {"completed", "failed", "cancelled", "interrupted"}:
                final = env
                break
            time.sleep(0.05)

        assert final is not None, f"polling timed out; saw statuses {seen_statuses}"
        assert final["status"] == "completed", final
        assert final["job_id"] == job_id
        assert final["tool"] == "echo_tool"
        assert final["created_at"]
        assert final["started_at"]
        assert final["completed_at"]
        # `result` is present once terminal + include_result=true.
        assert "result" in final, f"missing result once completed: {final}"
        assert final["result"]["ok"] is True
        assert final["result"]["echoed"] == {"hello": "world"}
    finally:
        handle.shutdown()


def test_jobs_get_status_include_result_false_omits_result():
    _server, handle, url = _make_server()
    try:
        sid = _initialize_session(url)
        body = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "echo_tool",
                    "arguments": {"x": 1},
                    "_meta": {"dcc": {"async": True}},
                },
            },
            sid=sid,
        )
        sc = body["result"].get("structuredContent") or json.loads(body["result"]["content"][0]["text"])
        job_id = sc["job_id"]

        # Wait for completion.
        deadline = time.monotonic() + 5.0
        env = None
        while time.monotonic() < deadline:
            poll = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "tools/call",
                    "params": {
                        "name": "jobs.get_status",
                        "arguments": {"job_id": job_id, "include_result": False},
                    },
                },
                sid=sid,
            )
            env = poll["result"]["structuredContent"]
            if env["status"] in {"completed", "failed", "cancelled", "interrupted"}:
                break
            time.sleep(0.05)

        assert env is not None
        assert env["status"] == "completed"
        assert "result" not in env, f"include_result=false must omit result: {env}"
    finally:
        handle.shutdown()
