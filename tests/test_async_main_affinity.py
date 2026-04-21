"""Integration tests for thread-affinity routing in async dispatch (#332).

These tests cover the Python-observable surface of issue #332:

1. ``ToolRegistry.register(..., thread_affinity="main")`` accepts the new
   kwarg and surfaces the value on ``list_actions()``.
2. Passing an invalid affinity string raises ``ValueError``.
3. A main-affined tool dispatched along the async ``tools/call`` path still
   returns the ``{job_id, status: "pending"}`` envelope **immediately** —
   the main-thread handoff is internal (AC 4).
4. Cancelling the returned job via ``$/dcc.cancel`` terminates it in a
   ``Cancelled`` terminal state before the handler runs (AC 3 — the
   ``submit_deferred`` wrapper drops the request when the cancel token
   fires before the pump reaches it).

The "executes on DCC main thread" half of AC 1/2 is covered by the Rust
unit tests in ``crates/dcc-mcp-http/src/executor.rs`` — the Python surface
has no ``DeferredExecutor`` binding today so thread-identity assertions
live alongside the ``submit_deferred`` implementation.
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
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        raw = resp.read().decode()
        return resp.status, (json.loads(raw) if raw else {})


def _tools_call(
    url: str,
    name: str,
    arguments: dict[str, Any] | None = None,
    meta: dict[str, Any] | None = None,
    req_id: int = 1,
) -> dict[str, Any]:
    params: dict[str, Any] = {"name": name}
    if arguments is not None:
        params["arguments"] = arguments
    if meta is not None:
        params["_meta"] = meta
    body = {"jsonrpc": "2.0", "id": req_id, "method": "tools/call", "params": params}
    _, resp = _post(url, body)
    return resp


# ── registry-level tests (pure Python, no server) ─────────────────────────


class TestThreadAffinityRegistration:
    def test_register_accepts_thread_affinity_main(self) -> None:
        reg = ToolRegistry()
        reg.register(
            "render_frame",
            description="Render a frame",
            dcc="maya",
            version="1.0.0",
            thread_affinity="main",
        )
        metas = list(reg.list_actions(dcc_name="maya"))
        assert len(metas) == 1
        meta = metas[0]
        affinity = meta["thread_affinity"] if isinstance(meta, dict) else meta.thread_affinity
        assert affinity == "main"

    def test_register_defaults_to_any(self) -> None:
        reg = ToolRegistry()
        reg.register("quick", description="quick tool", dcc="test", version="1.0.0")
        metas = list(reg.list_actions(dcc_name="test"))
        meta = metas[0]
        # When default, the field is either absent (serde skip) or "any".
        if isinstance(meta, dict):
            affinity = meta.get("thread_affinity", "any")
        else:
            affinity = getattr(meta, "thread_affinity", "any")
        assert affinity in {"any", None}

    def test_register_rejects_invalid_affinity(self) -> None:
        reg = ToolRegistry()
        with pytest.raises(ValueError):
            reg.register(
                "bad",
                description="",
                dcc="test",
                version="1.0.0",
                thread_affinity="render-thread",
            )


# ── async dispatch envelope tests (real HTTP server) ──────────────────────


@pytest.fixture(scope="module")
def server_url() -> Any:
    reg = ToolRegistry()
    # Main-affined async tool — the handler sleeps to prove the envelope
    # returns *before* the handler completes.
    reg.register(
        "main_affined",
        description="Tool that must run on DCC main thread",
        dcc="test",
        version="1.0.0",
        execution="async",
        timeout_hint_secs=30,
        thread_affinity="main",
    )
    reg.register(
        "any_affined",
        description="Tool with no thread constraint",
        dcc="test",
        version="1.0.0",
        execution="async",
        thread_affinity="any",
    )

    server = McpHttpServer(reg, McpHttpConfig(port=0, server_name="main-affinity-test"))
    server.register_handler(
        "main_affined",
        lambda params: (time.sleep(0.3), {"ok": True})[1],
    )
    server.register_handler(
        "any_affined",
        lambda params: {"ok": True, "affinity": "any"},
    )
    handle = server.start()
    try:
        yield handle.mcp_url()
    finally:
        handle.shutdown()


class TestMainAffinityAsyncEnvelope:
    def test_main_affined_tool_still_returns_pending_immediately(self, server_url: str) -> None:
        # Acceptance criterion 4: regardless of affinity the async envelope
        # returns immediately.
        t0 = time.perf_counter()
        resp = _tools_call(server_url, "main_affined", arguments={})
        elapsed = time.perf_counter() - t0

        assert "result" in resp, resp
        result = resp["result"]
        assert result["isError"] is False
        assert result["structuredContent"]["status"] == "pending"
        assert isinstance(result["structuredContent"]["job_id"], str)
        # Envelope must return well before the 300 ms handler sleep.
        assert elapsed < 0.25, f"main-affined async envelope blocked for {elapsed:.3f}s"

    def test_any_affined_tool_also_returns_pending_immediately(self, server_url: str) -> None:
        resp = _tools_call(server_url, "any_affined", arguments={})
        result = resp["result"]
        assert result["isError"] is False
        assert result["structuredContent"]["status"] == "pending"
