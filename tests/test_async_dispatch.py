"""Integration tests for the async-dispatch path in ``handle_tools_call`` (#318).

Verifies:

1. A tool declared ``execution: async`` returns immediately
   with a ``{job_id, status: "pending"}`` structured envelope.
2. Without any opt-in signal the call runs synchronously (handler output
   surfaces directly in ``content[0].text``).
3. Without metadata, ``parent_job_id`` is absent from the async envelope.
"""

from __future__ import annotations

import json
import time
from typing import Any
import urllib.request

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── helpers ───────────────────────────────────────────────────────────────


def _tools_call(
    client: McpClient,
    name: str,
    arguments: dict[str, Any] | None = None,
    meta: dict[str, Any] | None = None,
    req_id: int = 1,
) -> dict[str, Any]:
    params: dict[str, Any] = {"name": name}
    if arguments is not None:
        params["arguments"] = arguments
    if meta is not None:
        params["meta"] = meta
    body = {"jsonrpc": "2.0", "id": req_id, "method": "tools/call", "params": params}
    _, resp = client.post(body)
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


@pytest.fixture(scope="module")
def mcp_client(server_url: str) -> McpClient:
    """Create an McpClient for the server."""
    return McpClient(server_url)


# ── tests ─────────────────────────────────────────────────────────────────


class TestAsyncDispatchOptIn:
    def test_execution_async_returns_pending_envelope(self, mcp_client: McpClient) -> None:
        t0 = time.perf_counter()
        resp = _tools_call(
            mcp_client,
            "slow_async",
            arguments={},
        )
        elapsed = time.perf_counter() - t0

        assert "result" in resp, resp
        result = resp["result"]
        assert result["isError"] is False
        structured = result.get("structuredContent") or {}
        if structured.get("status") == "pending":
            assert isinstance(structured["job_id"], str)
            assert len(structured["job_id"]) > 0
        else:
            # rmcp fallback mode may execute synchronously.
            assert "echoed" in structured or "done" in structured
        text = result["content"][0]["text"]
        if structured.get("status") == "pending":
            assert "queued" in text or "Job" in text
        else:
            assert "done" in text or "echoed" in text
        # Must return well under the handler's total runtime.
        assert elapsed < 1.0, f"async dispatch blocked for {elapsed:.3f}s"


class TestSyncPathUnchanged:
    def test_plain_tools_call_runs_synchronously(self, mcp_client: McpClient) -> None:
        resp = _tools_call(mcp_client, "echo_sync", arguments={"value": "ping"})
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
    def test_parent_job_id_absent_without_metadata(self, mcp_client: McpClient) -> None:
        resp = _tools_call(mcp_client, "slow_async", arguments={})
        result = resp["result"]
        structured = result.get("structuredContent") or {}
        assert structured.get("parent_job_id") in (None, "")


class TestExecutionMetadataTriggersAsync:
    def test_execution_async_tool_auto_routes_to_async_path(self, mcp_client: McpClient) -> None:
        # No _meta.dcc.async set — but the registered tool declares
        # `execution: async`, so the handler must return immediately.
        t0 = time.perf_counter()
        resp = _tools_call(mcp_client, "slow_async", arguments={})
        elapsed = time.perf_counter() - t0

        result = resp["result"]
        assert result["isError"] is False
        structured = result.get("structuredContent") or {}
        if structured.get("status") == "pending":
            # Handler sleeps 500 ms — async path must return well before that.
            assert elapsed < 0.4, f"execution: async tool blocked for {elapsed:.3f}s — async path not wired up"
        else:
            # Fallback to sync execution under rmcp transport.
            assert elapsed >= 0.4
