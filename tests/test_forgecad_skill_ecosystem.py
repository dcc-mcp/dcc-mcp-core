"""Python-side ForgeCAD skill ecosystem acceptance coverage."""

from __future__ import annotations

import json
from pathlib import Path
import urllib.request

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import scan_and_load

FORGECAD_SKILL = "forgecad-make-a-model"
FORGECAD_TOOL = "create_model"


def _write_forgecad_skill(root: Path) -> None:
    skill_dir = root / FORGECAD_SKILL
    (skill_dir / "scripts").mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(
        """---
name: forgecad-make-a-model
description: Create new ForgeCAD (.forge.js) models in the active CAD project. Handles file placement, invokes the forgecad skill for API guidance, and validates the result.
dcc: forgecad
forgecad-public: true
tags: [forgecad, cad, third-party]
tools:
  - name: create_model
    description: Create a ForgeCAD model from a brief and return the generated file path.
    source_file: scripts/create_model.py
    input_schema:
      type: object
      properties:
        brief:
          type: string
      required: [brief]
---
# Make a Model

Create new ForgeCAD models in the user's active ForgeCAD project.
""",
        encoding="utf-8",
    )
    (skill_dir / "scripts" / "create_model.py").write_text(
        "# Python E2E uses in-process execution; this file proves source resolution.\n",
        encoding="utf-8",
    )


def _post_json(url: str, payload: dict) -> dict:
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        assert resp.status == 200
        return json.loads(resp.read())


def _mcp_post(mcp_url: str, method: str, params: dict | None = None, rpc_id: int = 1) -> dict:
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    return _post_json(mcp_url, body)


def _tool_text(response: dict) -> str:
    return response["result"]["content"][0]["text"]


def test_python_forgecad_skill_discovers_loads_and_calls_over_mcp_and_rest_http(tmp_path: Path) -> None:
    _write_forgecad_skill(tmp_path)

    skills, skipped = scan_and_load(extra_paths=[str(tmp_path)])
    assert skipped == []
    forgecad = next(skill for skill in skills if skill.name == FORGECAD_SKILL)
    assert forgecad.dcc == "forgecad"
    assert [tool.name for tool in forgecad.tools] == [FORGECAD_TOOL]

    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="forgecad-python-e2e"))

    def executor(script_path: str, params: dict, **context: object) -> dict:
        return {
            "success": True,
            "ecosystem": "forgecad",
            "script_path": script_path,
            "action_name": context["action_name"],
            "brief": params.get("brief", ""),
            "generated": "models/python-acceptance-test.forge.js",
        }

    server.set_in_process_executor(executor)
    assert server.discover(extra_paths=[str(tmp_path)]) == 1

    handle = server.start()
    try:
        mcp_url = handle.mcp_url()
        rest_url = mcp_url.removesuffix("/mcp")

        listed = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "list_skills", "arguments": {"status": "discovered"}},
            rpc_id=1,
        )
        assert FORGECAD_SKILL in _tool_text(listed)

        loaded = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": FORGECAD_SKILL}},
            rpc_id=2,
        )
        assert FORGECAD_TOOL in _tool_text(loaded)

        tools = _mcp_post(mcp_url, "tools/list", rpc_id=3)["result"]["tools"]
        assert FORGECAD_TOOL in {tool["name"] for tool in tools}

        called = _mcp_post(
            mcp_url,
            "tools/call",
            {"name": FORGECAD_TOOL, "arguments": {"brief": "python bracket"}},
            rpc_id=4,
        )
        assert "python bracket" in _tool_text(called)
        assert "python-acceptance-test.forge.js" in _tool_text(called)

        search = _post_json(f"{rest_url}/v1/search", {"query": "forgecad", "loaded_only": True})
        slug = next(hit["slug"] for hit in search["hits"] if hit["skill"] == FORGECAD_SKILL)
        rest_call = _post_json(
            f"{rest_url}/v1/call",
            {"tool_slug": slug, "params": {"brief": "rest python model"}},
        )
        assert rest_call["output"]["ecosystem"] == "forgecad"
        assert rest_call["output"]["brief"] == "rest python model"
    finally:
        handle.shutdown()
