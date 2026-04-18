"""Tests for MCP protocol version negotiation (issue #239).

Verifies that the server negotiates the protocol version correctly:
- Client requests a supported version -> server echoes it back.
- Client requests an unsupported version -> server picks latest supported.
- No version in params -> server picks latest supported.
"""

from __future__ import annotations

import json
from typing import Any
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── Helpers ──────────────────────────────────────────────────────────────────


def _post_json(url: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    """POST a JSON-RPC message and return (status_code, response_body)."""
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
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _initialize(url: str, protocol_version: str | None = None) -> dict[str, Any]:
    """Send an initialize request and return the result dict."""
    params: dict[str, Any] = {
        "capabilities": {},
        "clientInfo": {"name": "pytest-negotiation", "version": "1.0"},
    }
    if protocol_version is not None:
        params["protocolVersion"] = protocol_version

    code, body = _post_json(
        url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": params,
        },
    )
    assert code == 200, f"Expected 200, got {code}"
    assert "result" in body, f"No result in response: {body}"
    return body["result"]


# ── Fixtures ─────────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def server_url():
    """Start a McpHttpServer on a random port; yield the MCP endpoint URL."""
    reg = ToolRegistry()
    reg.register(
        "noop",
        description="No-op tool for version negotiation tests",
        category="test",
        dcc="test",
        version="1.0.0",
    )
    config = McpHttpConfig(port=0, server_name="version-negotiation-test")
    server = McpHttpServer(reg, config)
    handle = server.start()
    url = handle.mcp_url()
    yield url
    handle.shutdown()


# ── Tests ────────────────────────────────────────────────────────────────────


class TestProtocolVersionNegotiation:
    """Verify MCP protocol version negotiation."""

    def test_client_requests_2025_03_26(self, server_url):
        """Client sends '2025-03-26' -> server echoes '2025-03-26'."""
        result = _initialize(server_url, "2025-03-26")
        assert result["protocolVersion"] == "2025-03-26"

    def test_client_requests_2025_06_18(self, server_url):
        """Client sends '2025-06-18' -> server echoes '2025-06-18'."""
        result = _initialize(server_url, "2025-06-18")
        assert result["protocolVersion"] == "2025-06-18"

    def test_client_requests_unknown_version(self, server_url):
        """Client sends an unknown version -> server falls back to latest."""
        result = _initialize(server_url, "2099-01-01")
        # Server should pick its latest supported version
        assert result["protocolVersion"] == "2025-06-18"

    def test_client_omits_version(self, server_url):
        """Client omits protocolVersion entirely -> server uses latest."""
        result = _initialize(server_url, None)
        assert result["protocolVersion"] == "2025-06-18"
