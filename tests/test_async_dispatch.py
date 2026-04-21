"""Integration tests for the async-dispatch path in ``handle_tools_call`` (#318).

Verifies:

1. A ``tools/call`` carrying ``_meta.dcc.async = true`` returns immediately
   with a ``{job_id, status: "pending"}`` structured envelope.
2. Without any opt-in signal the call runs synchronously (handler output
   surfaces directly in ``content[0].text``).
3. ``_meta.dcc.parentJobId`` propagates: the returned ``structured_content``
   carries ``parent_job_id`` matching what the client sent.
4. Declaring ``execution: async`` on the registered tool also triggers the
   async path even without ``_meta.dcc.async``.
"""

from __future__ import annotations

import json
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── helpers ───────────────────────────────────────────────────────────────


def _post(url: str, body: Any) -> tuple[int, dict[str, Any]]:
    """POST JSON-RPC to ``url`` and return ``(status, parsed_json)``."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        raw = resp.read().decode()
        return resp.status, json.loads(raw) if raw else {}


def _tools_call(
    url: str, name: str, arguments: dict[str, Any] | None = None, meta: dict[str, Any] | None = None, req_id: int = 1
) -> dict[str, Any]:
    params: dict[str, Any] = {"name": name}
    if arguments is not None:
        params["arguments"] = arguments
    if meta is not None:
        params["_meta"] = meta
    body = {"jsonrpc": "2.0", "id": req_id, "method": "tools/call", "params": params}
    _, resp = _post(url, body)
    return resp


# ── fixtures ──────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def server_url() -> Any:
    """Boot an MCP HTTP server with a mix of sync and async tools."""
    reg = ToolRegistry()
    # Plain sync tool.
    reg.register(
        "echo_sync",
        description="Echo argument synchronously",
        category="test",
        dcc="test",
        version="1.0.0",
    )
    # Tool declared `execution: async` — should auto-route through the
    # async path even without _meta.dcc.async.
    reg.register(
        "slow_async",
        description="Long-running tool",
        category="test",
        dcc="test",
        version="1.0.0",
        execution="async",
        timeout_hint_secs=30,
    )

    server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="async-dispatch-test"))
    server.register_handler("echo_sync", lambda params: {"echoed": params.get("value")})
    # Handler that would block for 500 ms — async dispatch must return
    # *immediately*, not after this sleeps.
    server.register_handler(
        "slow_async",
        lambda params: (time.sleep(0.5), {"done": True})[1],
    )
    handle = server.start()
    try:
        yield handle.mcp_url()
    finally:
        handle.shutdown()


# ── tests ─────────────────────────────────────────────────────────────────


class TestAsyncDispatchOptIn:
    def test_explicit_meta_dcc_async_returns_pending_envelope(self, server_url: str) -> None:
        t0 = time.perf_counter()
        resp = _tools_call(
            server_url,
            "echo_sync",
            arguments={"value": "hello"},
            meta={"dcc": {"async": True}},
        )
        elapsed = time.perf_counter() - t0

        assert "result" in resp, resp
        result = resp["result"]
        assert result["isError"] is False
        assert result["structuredContent"]["status"] == "pending"
        assert isinstance(result["structuredContent"]["job_id"], str)
        assert len(result["structuredContent"]["job_id"]) > 0
        # "Job <uuid> queued" text surface.
        text = result["content"][0]["text"]
        assert "queued" in text or "Job" in text
        # Must return well under the handler's total runtime.
        assert elapsed < 1.0, f"async dispatch blocked for {elapsed:.3f}s"


class TestSyncPathUnchanged:
    def test_plain_tools_call_runs_synchronously(self, server_url: str) -> None:
        resp = _tools_call(server_url, "echo_sync", arguments={"value": "ping"})
        assert "result" in resp
        result = resp["result"]
        assert result["isError"] is False
        # Sync path returns the handler output directly, no "pending" status.
        structured = result.get("structuredContent")
        if structured is not None:
            assert structured.get("status") != "pending"
        # Text content carries the echoed value.
        text = result["content"][0]["text"]
        assert "ping" in text


class TestParentJobIdPropagation:
    def test_parent_job_id_round_trips_in_structured_content(self, server_url: str) -> None:
        parent = "11111111-2222-3333-4444-555555555555"
        resp = _tools_call(
            server_url,
            "echo_sync",
            arguments={"value": "x"},
            meta={"dcc": {"async": True, "parentJobId": parent}},
        )
        result = resp["result"]
        assert result["structuredContent"]["parent_job_id"] == parent


class TestExecutionMetadataTriggersAsync:
    def test_execution_async_tool_auto_routes_to_async_path(self, server_url: str) -> None:
        # No _meta.dcc.async set — but the registered tool declares
        # `execution: async`, so the handler must return immediately.
        t0 = time.perf_counter()
        resp = _tools_call(server_url, "slow_async", arguments={})
        elapsed = time.perf_counter() - t0

        result = resp["result"]
        assert result["isError"] is False
        assert result["structuredContent"]["status"] == "pending"
        # Handler sleeps 500 ms — async path must return well before that.
        assert elapsed < 0.4, f"execution: async tool blocked for {elapsed:.3f}s — async path not wired up"
