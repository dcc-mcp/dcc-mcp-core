"""Regression tests for adapter skill-load transform hooks (#1204)."""

from __future__ import annotations

import json
from pathlib import Path
import time
import urllib.request

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _write_skill(root: Path, name: str) -> None:
    skill_dir = root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(
        f"""---
name: {name}
description: Original skill description
metadata:
  dcc-mcp:
    dcc: python
    version: "1.0.0"
    layer: example
    tags: "test, hook"
    tools: tools.yaml
---

# {name}
""",
        encoding="utf-8",
    )
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: host_scene
    description: Original host scene tool
    input_schema:
      type: object
      properties: {}
""",
        encoding="utf-8",
    )


def _make_server(tmp_path: Path, skill_name: str) -> McpHttpServer:
    _write_skill(tmp_path, skill_name)
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name=f"hook-{skill_name}"))
    server.discover(extra_paths=[str(tmp_path)])
    return server


def _install_transform(server: McpHttpServer, label: str) -> list[tuple[str, list[str]]]:
    observed: list[tuple[str, list[str]]] = []

    def transform(skill):
        tools = list(skill.tools)
        tools[0].description = f"{skill.name} transformed through {label}"
        skill.tools = tools
        skill.description = f"{skill.name} metadata transformed"
        return None

    def after_load(skill, registered):
        observed.append((skill.name, list(registered)))

    assert server.set_skill_load_transform(transform) is None
    assert server.set_after_load_skill_hook(after_load) is None
    return observed


def _registered_description(server: McpHttpServer, action_name: str) -> str:
    action = server.registry.get_action(action_name)
    assert action is not None
    return action["description"]


def _post_json(url: str, body: dict) -> dict:
    data = json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def _remove_suffix(value: str, suffix: str) -> str:
    if suffix and value.endswith(suffix):
        return value[: -len(suffix)]
    return value


def test_programmatic_load_skill_uses_transform_and_after_hook(tmp_path: Path) -> None:
    server = _make_server(tmp_path, "programmatic-policy")
    observed = _install_transform(server, "programmatic")

    registered = server.load_skill("programmatic-policy")

    assert registered == ["programmatic_policy__host_scene"]
    assert _registered_description(server, "programmatic_policy__host_scene") == (
        "programmatic-policy transformed through programmatic"
    )
    info = server.get_skill_info("programmatic-policy")
    assert info["description"] == "programmatic-policy metadata transformed"
    assert observed == [("programmatic-policy", ["programmatic_policy__host_scene"])]


def test_mcp_load_skill_uses_same_transform(tmp_path: Path) -> None:
    server = _make_server(tmp_path, "mcp-policy")
    _install_transform(server, "mcp")
    handle = server.start()
    try:
        time.sleep(0.2)
        response = McpClient(handle.mcp_url()).post(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "load_skill",
                    "arguments": {"skill_name": "mcp-policy"},
                },
            }
        )[1]
        assert "error" not in response
        payload = json.loads(response["result"]["content"][0]["text"])
        tool = next(t for t in payload["tools"] if t["name"] == "mcp_policy__host_scene")
        assert tool["description"] == "mcp-policy transformed through mcp"
    finally:
        handle.shutdown()


def test_rest_load_skill_uses_same_transform(tmp_path: Path) -> None:
    server = _make_server(tmp_path, "rest-policy")
    _install_transform(server, "rest")
    handle = server.start()
    try:
        time.sleep(0.2)
        base_url = _remove_suffix(handle.mcp_url(), "/mcp")
        payload = _post_json(f"{base_url}/v1/load_skill", {"skill_name": "rest-policy"})
        assert payload["skill_name"] == "rest-policy"
        assert payload["actions"] == ["rest_policy__host_scene"]
        assert _registered_description(server, "rest_policy__host_scene") == ("rest-policy transformed through rest")
    finally:
        handle.shutdown()
