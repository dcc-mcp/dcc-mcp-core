"""Tests for the structured error envelope (DccMcpError) in tools/call responses.

When tools/call hits an error path the server returns a JSON-serialised
DccMcpError envelope inside the CallToolResult text content.  This test
file verifies the envelope shape for the key error paths:

  - Unknown tool (ACTION_NOT_FOUND)
  - Skill stub (__skill__<name> → SKILL_NOT_LOADED)
  - No handler registered (NO_HANDLER)

See: GitHub issue #237
"""

from __future__ import annotations

import json
from typing import Any

# ── helpers ──────────────────────────────────────────────────────────────
import urllib.error
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _post_json(
    url: str, body: dict[str, Any] | list, headers: dict[str, str] | None = None
) -> tuple[int, dict[str, Any]]:
    """POST a JSON-RPC message and return (status_code, response_body)."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            **(headers or {}),
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _parse_error_envelope(body: dict[str, Any]) -> dict[str, Any]:
    """Extract and parse the DccMcpError envelope from a tools/call response."""
    result = body["result"]
    assert result["isError"] is True, "Expected isError=true"
    content = result["content"]
    assert len(content) >= 1
    assert content[0]["type"] == "text"
    text = content[0]["text"]
    envelope = json.loads(text)
    return envelope


def _assert_envelope_shape(envelope: dict[str, Any]) -> None:
    """Assert that the envelope has the required DccMcpError fields."""
    assert "layer" in envelope, "Missing 'layer' field"
    assert "code" in envelope, "Missing 'code' field"
    assert "message" in envelope, "Missing 'message' field"
    # layer must be one of the well-known values
    assert envelope["layer"] in (
        "gateway",
        "registry",
        "instance",
        "subprocess",
        "dcc",
    ), f"Unknown layer: {envelope['layer']}"
    # code must be a non-empty UPPER_SNAKE_CASE string
    assert isinstance(envelope["code"], str)
    assert len(envelope["code"]) > 0
    assert envelope["code"] == envelope["code"].upper()
    # message must be a non-empty string
    assert isinstance(envelope["message"], str)
    assert len(envelope["message"]) > 0
    # hint is optional but must be a string if present
    if "hint" in envelope and envelope["hint"] is not None:
        assert isinstance(envelope["hint"], str)
    # trace_id is optional but must be a string if present
    if "trace_id" in envelope and envelope["trace_id"] is not None:
        assert isinstance(envelope["trace_id"], str)


# ── fixtures ─────────────────────────────────────────────────────────────


@pytest.fixture(scope="module")
def error_envelope_server():
    """Start a minimal McpHttpServer for error envelope testing."""
    reg = ToolRegistry()
    # Register a tool WITHOUT a handler to test NO_HANDLER path
    reg.register(
        "no_handler_tool",
        description="Tool registered without handler",
        category="test",
        tags=[],
        dcc="test",
        version="1.0.0",
    )
    config = McpHttpConfig(port=0, server_name="error-envelope-test")
    server = McpHttpServer(reg, config)
    # Intentionally do NOT register a handler for "no_handler_tool"
    handle = server.start()
    url = handle.mcp_url()
    yield url
    handle.shutdown()


# ── tests ────────────────────────────────────────────────────────────────


class TestDccMcpErrorEnvelope:
    """Verify that tools/call error responses contain a structured DccMcpError envelope."""

    def test_unknown_tool_returns_action_not_found(self, error_envelope_server):
        """Calling a completely unknown tool returns ACTION_NOT_FOUND."""
        url = error_envelope_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "totally_unknown_tool", "arguments": {}},
            },
        )
        assert code == 200
        envelope = _parse_error_envelope(body)
        _assert_envelope_shape(envelope)

        assert envelope["layer"] == "registry"
        assert envelope["code"] == "ACTION_NOT_FOUND"
        assert "totally_unknown_tool" in envelope["message"]
        # Should have a hint about how to find tools
        assert envelope.get("hint") is not None
        assert len(envelope["hint"]) > 0

    def test_skill_stub_returns_skill_not_loaded(self, error_envelope_server):
        """Calling __skill__<name> returns SKILL_NOT_LOADED with load hint."""
        url = error_envelope_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": "__skill__my-cool-skill", "arguments": {}},
            },
        )
        assert code == 200
        envelope = _parse_error_envelope(body)
        _assert_envelope_shape(envelope)

        assert envelope["layer"] == "gateway"
        assert envelope["code"] == "SKILL_NOT_LOADED"
        assert "my-cool-skill" in envelope["message"]
        # Hint should mention load_skill
        assert envelope.get("hint") is not None
        assert "load_skill" in envelope["hint"]

    def test_group_stub_returns_group_not_activated(self, error_envelope_server):
        """Calling __group__<name> returns GROUP_NOT_ACTIVATED with activation hint."""
        url = error_envelope_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "__group__advanced", "arguments": {}},
            },
        )
        assert code == 200
        envelope = _parse_error_envelope(body)
        _assert_envelope_shape(envelope)

        assert envelope["layer"] == "gateway"
        assert envelope["code"] == "GROUP_NOT_ACTIVATED"
        assert "advanced" in envelope["message"]
        # Hint should mention activate_tool_group
        assert envelope.get("hint") is not None
        assert "activate_tool_group" in envelope["hint"]

    def test_no_handler_returns_structured_error(self, error_envelope_server):
        """Calling a tool registered without a handler returns NO_HANDLER."""
        url = error_envelope_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "no_handler_tool", "arguments": {}},
            },
        )
        assert code == 200
        envelope = _parse_error_envelope(body)
        _assert_envelope_shape(envelope)

        assert envelope["layer"] == "instance"
        assert envelope["code"] == "NO_HANDLER"
        assert "no_handler_tool" in envelope["message"]
        # Hint should mention register_handler
        assert envelope.get("hint") is not None
        assert "register_handler" in envelope["hint"]

    def test_trace_id_present_and_unique(self, error_envelope_server):
        """Each error envelope should have a unique trace_id for log correlation."""
        url = error_envelope_server
        trace_ids = []
        for i in range(3):
            code, body = _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 100 + i,
                    "method": "tools/call",
                    "params": {"name": f"nonexistent_{i}", "arguments": {}},
                },
            )
            assert code == 200
            envelope = _parse_error_envelope(body)
            tid = envelope.get("trace_id")
            assert tid is not None, "trace_id should be present"
            assert isinstance(tid, str)
            assert len(tid) > 0
            trace_ids.append(tid)

        # All trace IDs should be unique
        assert len(set(trace_ids)) == 3, f"trace_ids should be unique, got: {trace_ids}"

    def test_envelope_is_valid_json(self, error_envelope_server):
        """The error text content must be valid JSON parseable as DccMcpError."""
        url = error_envelope_server
        code, body = _post_json(
            url,
            {
                "jsonrpc": "2.0",
                "id": 200,
                "method": "tools/call",
                "params": {"name": "ghost_tool", "arguments": {}},
            },
        )
        assert code == 200
        result = body["result"]
        assert result["isError"] is True
        text = result["content"][0]["text"]

        # Must be valid JSON
        try:
            parsed = json.loads(text)
        except json.JSONDecodeError:
            pytest.fail(f"Error text is not valid JSON: {text!r}")

        # Must have the envelope structure
        assert isinstance(parsed, dict)
        assert set(parsed.keys()) >= {"layer", "code", "message"}
