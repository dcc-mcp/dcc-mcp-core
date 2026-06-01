"""MCP/REST coverage for agent-facing script materialization tools (#1222)."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
import urllib.request

from conftest import McpClient
import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import register_script_materialization_tools


def _rest_base(mcp_url: str) -> str:
    return mcp_url.rsplit("/mcp", 1)[0]


def _post_json(url: str, payload: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return resp.status, json.loads(resp.read().decode("utf-8"))


def _tool_payload(body: dict[str, Any]) -> dict[str, Any]:
    text = body["result"]["content"][0]["text"]
    payload = json.loads(text)
    assert isinstance(payload, dict)
    return payload


def test_register_script_materialization_tool_is_exported() -> None:
    assert dcc_mcp_core.register_script_materialization_tools is register_script_materialization_tools
    assert "register_script_materialization_tools" in dcc_mcp_core.__all__


def test_materialize_script_tool_list_entry_stays_compact(tmp_path: Path) -> None:
    registry = ToolRegistry()
    server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="materialize-script-size-test"))
    register_script_materialization_tools(
        server,
        dcc_name="custom",
        instance_id="inst-1",
        session_id="sess-1",
        root=tmp_path,
    )

    handle = server.start()
    try:
        client = McpClient(handle.mcp_url())
        code, body = client.post({"jsonrpc": "2.0", "id": "list-1", "method": "tools/list"})

        assert code == 200
        tool = next(tool for tool in body["result"]["tools"] if tool["name"] == "materialize_script")
        payload = json.dumps(tool, separators=(",", ":"), sort_keys=True).encode("utf-8")
        assert len(payload) < 2048
        assert tool["inputSchema"]["type"] == "object"
        assert "anyOf" not in tool["inputSchema"]
        assert "oneOf" not in tool["inputSchema"]
        assert "allOf" not in tool["inputSchema"]
        assert "not" not in tool["inputSchema"]
        assert set(tool["outputSchema"]["required"]) == {
            "file_ref",
            "file_path",
            "sha256",
            "bytes",
            "dcc_type",
            "instance_id",
            "session_id",
            "reused",
        }
    finally:
        handle.shutdown()


def test_materialize_script_tool_supports_mcp_and_rest(tmp_path: Path) -> None:
    registry = ToolRegistry()
    server = McpHttpServer(registry, McpHttpConfig(port=0, server_name="materialize-script-test"))
    assert (
        register_script_materialization_tools(
            server,
            dcc_name="custom",
            instance_id="inst-1",
            session_id="sess-1",
            root=tmp_path,
        )
        == 1
    )

    handle = server.start()
    try:
        client = McpClient(handle.mcp_url())
        source = "print('do-not-store-in-telemetry')"
        code, body = client.post(
            {
                "jsonrpc": "2.0",
                "id": "mat-1",
                "method": "tools/call",
                "params": {
                    "name": "materialize_script",
                    "arguments": {
                        "content": source,
                        "display_name": "agent-script",
                        "reuse": True,
                        "reuse_key": "agent-flow",
                        "ttl_secs": 3600,
                        "tool_call_id": "tool-call-1",
                        "correlation_id": "corr-1",
                    },
                },
            }
        )

        assert code == 200
        assert body["result"]["isError"] is False
        payload = _tool_payload(body)
        script_path = Path(payload["file_path"])
        assert script_path.is_file()
        assert script_path.read_text(encoding="utf-8") == source
        assert payload["file_ref"]["digest"] == f"sha256:{payload['sha256']}"
        assert payload["bytes"] == len(source.encode("utf-8"))
        assert payload["dcc_type"] == "custom"
        assert payload["instance_id"] == "inst-1"
        assert payload["session_id"] == "sess-1"
        assert payload["tool_call_id"] == "tool-call-1"
        assert payload["correlation_id"] == "corr-1"
        assert source not in json.dumps(payload)

        base = _rest_base(handle.mcp_url())
        code, search = _post_json(f"{base}/v1/search", {"query": "materialize script", "loaded_only": True})
        assert code == 200
        slugs = {hit["slug"] for hit in search["hits"]}
        assert "custom.core.materialize_script" in slugs

        code, called = _post_json(
            f"{base}/v1/call",
            {
                "tool_slug": "custom.core.materialize_script",
                "params": {
                    "code": "result = 1222",
                    "language": "python",
                    "suffix": ".py",
                    "session_id": "rest-session",
                },
            },
        )
        assert code == 200
        output = called["output"]
        assert Path(output["file_path"]).read_text(encoding="utf-8") == "result = 1222"
        assert output["session_id"] == "rest-session"
        assert "result = 1222" not in json.dumps(output)
    finally:
        handle.shutdown()
