"""Integration tests for the Prometheus /metrics endpoint (issue #331).

Requires the wheel to have been built with the ``prometheus`` Cargo
feature:

    maturin develop --features python-bindings,ext-module,workflow,prometheus

Without the feature, the ``enable_prometheus`` flag is accepted but
silently has no effect (the ``/metrics`` route is not mounted). The
tests probe this by checking whether the endpoint is present after
starting a server with the flag enabled; when absent (feature missing)
the whole module is skipped rather than failing, so default CI runs
that build without the feature stay green.
"""

from __future__ import annotations

import base64
import json
import time
from typing import Any
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _make_registry() -> ToolRegistry:
    reg = ToolRegistry()
    reg.register(
        "ping",
        description="Test ping tool",
        category="test",
        dcc="test",
        version="1.0.0",
    )
    return reg


def _get(url: str, headers: dict[str, str] | None = None) -> tuple[int, str, dict[str, str]]:
    req = urllib.request.Request(url, headers=headers or {}, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            body = resp.read().decode("utf-8", errors="replace")
            return resp.status, body, dict(resp.headers)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace") if e.fp else ""
        return e.code, body, dict(e.headers or {})


def _post_jsonrpc(url: str, body: dict[str, Any]) -> int:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status
    except urllib.error.HTTPError as e:
        return e.code


@pytest.fixture
def prom_server():
    """Start a server with Prometheus enabled.

    If the wheel was built without the `prometheus` Cargo feature, the
    /metrics endpoint is absent; we skip the whole module in that case
    so CI runs that didn't opt in stay green.
    """
    cfg = McpHttpConfig(port=0, server_name="prom-test", enable_prometheus=True)
    reg = _make_registry()
    server = McpHttpServer(reg, cfg)
    server.register_handler("ping", lambda _params: {"pong": True})
    handle = server.start()

    url = f"http://{handle.bind_addr}/metrics"
    status, _body, _headers = _get(url)
    if status == 404:
        handle.shutdown()
        pytest.skip(
            "wheel built without the `prometheus` Cargo feature; "
            "rebuild with `maturin develop --features ...,prometheus`"
        )
    try:
        yield handle
    finally:
        handle.shutdown()


def test_metrics_endpoint_returns_prometheus_payload(prom_server):
    url = f"http://{prom_server.bind_addr}/metrics"
    status, body, headers = _get(url)
    assert status == 200
    ctype = headers.get("Content-Type") or headers.get("content-type", "")
    assert "text/plain" in ctype
    assert "version=0.0.4" in ctype
    # Always-on series
    assert "dcc_mcp_build_info" in body
    assert "dcc_mcp_active_sessions" in body
    assert "dcc_mcp_registered_tools" in body


def test_tool_calls_advance_counter(prom_server):
    mcp_url = f"http://{prom_server.bind_addr}/mcp"
    metrics_url = f"http://{prom_server.bind_addr}/metrics"

    for i in range(5):
        status = _post_jsonrpc(
            mcp_url,
            {
                "jsonrpc": "2.0",
                "id": i,
                "method": "tools/call",
                "params": {"name": "ping", "arguments": {}},
            },
        )
        assert status == 200, f"tools/call {i} failed with {status}"

    # Poll for the counter — the record happens synchronously in the
    # wrapper, but the HTTP response may be flushed slightly before the
    # handler returns on some platforms; a short retry makes the test
    # robust without sleeping unconditionally.
    deadline = time.monotonic() + 2.0
    value = 0
    while time.monotonic() < deadline:
        _, body, _ = _get(metrics_url)
        target_prefix = 'dcc_mcp_tool_calls_total{status="success",tool="ping"}'
        for line in body.splitlines():
            if line.startswith(target_prefix):
                try:
                    value = int(line.rsplit(" ", 1)[1])
                except ValueError:
                    value = 0
                break
        if value >= 5:
            break
        time.sleep(0.05)
    assert value >= 5, f"expected >=5 ping success calls, got {value}"


def test_basic_auth_rejects_without_credentials():
    cfg = McpHttpConfig(
        port=0,
        server_name="prom-auth",
        enable_prometheus=True,
        prometheus_basic_auth=("admin", "s3cret"),
    )
    reg = _make_registry()
    server = McpHttpServer(reg, cfg)
    handle = server.start()
    try:
        url = f"http://{handle.bind_addr}/metrics"

        # Skip if feature is not compiled in.
        status, _, _ = _get(url)
        if status == 404:
            pytest.skip("prometheus feature not enabled in wheel")

        # No auth header → 401
        assert status == 401, f"expected 401 without auth, got {status}"

        # Wrong password → 401
        wrong = base64.b64encode(b"admin:nope").decode()
        status, _, _ = _get(url, headers={"Authorization": f"Basic {wrong}"})
        assert status == 401

        # Correct password → 200
        good = base64.b64encode(b"admin:s3cret").decode()
        status, body, _ = _get(url, headers={"Authorization": f"Basic {good}"})
        assert status == 200
        assert "dcc_mcp_build_info" in body
    finally:
        handle.shutdown()


def test_metrics_endpoint_absent_when_flag_is_off():
    cfg = McpHttpConfig(port=0, server_name="prom-off", enable_prometheus=False)
    reg = _make_registry()
    server = McpHttpServer(reg, cfg)
    handle = server.start()
    try:
        status, _, _ = _get(f"http://{handle.bind_addr}/metrics")
        # 404 both when the feature was compiled but the flag is off,
        # and when the feature was never compiled in.
        assert status == 404
    finally:
        handle.shutdown()
