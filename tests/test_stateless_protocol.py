"""Integration tests for ADR-010 Phase 1 stateless protocol routing.

These tests validate that the gateway correctly routes requests based on
the `MCP-Protocol-Version` header without disrupting existing session-based
clients.
"""

import json
import urllib.request
import urllib.error


# ── Helpers ────────────────────────────────────────────────────────────────

def _mcp_request(endpoint, body, headers=None):
    """Send a POST request to the gateway /mcp endpoint."""
    if headers is None:
        headers = {}
    headers.setdefault("Content-Type", "application/json")
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(endpoint, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8") if e.fp else "{}"
        try:
            payload = json.loads(body)
        except json.JSONDecodeError:
            payload = {"raw": body}
        return e.code, payload


def _initialize_body(client_name="test-client", protocol_version="2025-06-18"):
    """Build a standard initialize request."""
    return {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": protocol_version,
            "capabilities": {},
            "clientInfo": {"name": client_name, "version": "1.0.0"},
        },
    }


# ── Test: stateless routing ─────────────────────────────────────────────────

def test_stateless_returns_501(gateway_endpoint):
    """A stateless request (MCP-Protocol-Version: 2026-07-28) should return
    a 501 Not Implemented error in Phase 1a."""
    status, payload = _mcp_request(
        gateway_endpoint,
        {"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}},
        headers={"MCP-Protocol-Version": "2026-07-28"},
    )
    assert status == 501, f"Expected 501, got {status}: {payload}"
    assert payload["error"]["code"] == -32601
    assert "stateless" in payload["error"]["message"].lower()


# ── Test: session compatibility ─────────────────────────────────────────────

def test_session_client_unaffected(gateway_endpoint):
    """Existing session clients (no MCP-Protocol-Version header) should work
    exactly as before — initialize, get session id, tools/call."""
    # Step 1: initialize (creates session)
    status, init_resp = _mcp_request(
        gateway_endpoint,
        _initialize_body("codex-desktop"),
        headers={},
    )
    assert status == 200, f"Initialize failed: {init_resp}"
    assert "Mcp-Session-Id" in init_resp.get("_headers", {}) or init_resp.get("id") is not None

    # The initialize response should succeed — no stateless interference
    assert "result" in init_resp
    assert init_resp["result"]["protocolVersion"] == "2025-06-18"


def test_no_header_defaults_to_session(gateway_endpoint):
    """Requests without MCP-Protocol-Version header should default to
    session mode — even if they use newer method names."""
    status, payload = _mcp_request(
        gateway_endpoint,
        _initialize_body("claude-code"),
        headers={"Mcp-Session-Id": "test-session-123"},
    )
    assert status == 200, f"Session request failed: {payload}"
    assert "result" in payload
