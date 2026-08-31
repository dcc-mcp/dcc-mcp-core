"""Real MCP and REST regressions for portable recipe regex admission."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock
import urllib.error
import urllib.request

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core.recipes import register_recipes_tools


def _post_json(url: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={"Accept": "application/json", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read().decode("utf-8"))


def _mcp_output(body: dict[str, Any]) -> dict[str, Any]:
    result = body["result"]
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured
    return json.loads(result["content"][0]["text"])


def _rest_slug(base_url: str, action: str) -> str:
    status, body = _post_json(
        f"{base_url}/v1/search",
        {"query": action, "dcc_type": "python", "loaded_only": True},
    )
    assert status == 200
    matches = [hit["slug"] for hit in body["hits"] if hit.get("action") == action]
    assert len(matches) == 1, body
    return matches[0]


def _recipe_metadata(skill_dir: Path, recipe_path: Path) -> MagicMock:
    metadata = MagicMock()
    metadata.name = "regex-portability-route-skill"
    metadata.skill_path = str(skill_dir)
    metadata.metadata = {"dcc-mcp": {"recipes": str(recipe_path)}}
    return metadata


@pytest.mark.parametrize("transport", ["mcp", "rest"])
def test_non_boundary_assertion_is_rejected_through_real_routes(tmp_path: Path, transport: str) -> None:
    recipe_path = tmp_path / "recipes.yaml"
    recipe_path.write_text(
        json.dumps(
            {
                "recipes": [
                    {
                        "name": "pattern",
                        "inputs_schema": {
                            "type": "object",
                            "properties": {"value": {"type": "string", "pattern": r"\B"}},
                        },
                        "steps": [],
                    },
                    {
                        "name": "pattern-properties",
                        "inputs_schema": {
                            "type": "object",
                            "patternProperties": {r"\B": {"type": "string"}},
                        },
                        "steps": [],
                    },
                    {
                        "name": "literal-control",
                        "inputs_schema": {
                            "type": "object",
                            "properties": {"value": {"type": "string", "pattern": r"^\\B$"}},
                        },
                        "steps": [],
                    },
                    {
                        "name": "character-class-control",
                        "inputs_schema": {
                            "type": "object",
                            "properties": {"value": {"type": "string", "pattern": r"^[\\]B$"}},
                        },
                        "steps": [],
                    },
                ]
            }
        ),
        encoding="utf-8",
    )
    skill_dir = tmp_path / "regex-portability-route-skill"
    skill_dir.mkdir()
    metadata = _recipe_metadata(skill_dir, recipe_path)
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="recipe-regex-portability"))
    register_recipes_tools(server, skills=[metadata], dcc_name="python")
    handle = server.start()
    try:
        mcp_url = handle.mcp_url()
        cases = (
            ("pattern", {"value": ""}, False),
            ("pattern-properties", {"": "value"}, False),
            ("literal-control", {"value": r"\B"}, True),
            ("character-class-control", {"value": r"\B"}, True),
        )
        for recipe_name, inputs, expected_valid in cases:
            arguments = {
                "skill": "regex-portability-route-skill",
                "recipe": recipe_name,
                "inputs": inputs,
            }
            for action in ("recipes__validate", "recipes__apply"):
                if transport == "mcp":
                    status, body = McpClient(mcp_url).post(
                        {
                            "jsonrpc": "2.0",
                            "id": f"{action}:{recipe_name}",
                            "method": "tools/call",
                            "params": {"name": action, "arguments": arguments},
                        }
                    )
                    assert status == 200
                    output = _mcp_output(body)
                else:
                    base_url = mcp_url.rsplit("/mcp", 1)[0]
                    status, body = _post_json(
                        f"{base_url}/v1/call",
                        {
                            "tool_slug": _rest_slug(base_url, action),
                            "arguments": arguments,
                        },
                    )
                    assert status == 200
                    output = body["output"]

                if action == "recipes__validate":
                    assert output["success"] is True
                    assert output["context"]["valid"] is expected_valid
                else:
                    assert output["success"] is expected_valid
                if not expected_valid:
                    assert output["context"]["errors"] == ["$: Recipe input schema is invalid"]
    finally:
        handle.shutdown()
