"""Regression: exclude progressive stubs from tools/list (#174 / #238)."""

from __future__ import annotations

import json
from pathlib import Path
import time

import pytest

from conftest import McpClient
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS = REPO_ROOT / "examples" / "skills"


def _tools_list(url: str) -> list[dict]:
    client = McpClient(url)
    _, resp = client.post({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    return resp["result"]["tools"]


@pytest.fixture(scope="module")
def catalog_server_exclude_stubs():
    if not EXAMPLES_SKILLS.is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ToolRegistry()
    config = McpHttpConfig(port=0, server_name="ci-exclude-stubs")
    config.exclude_skill_stubs_from_tools_list = True
    config.exclude_group_stubs_from_tools_list = True
    server = McpHttpServer(reg, config)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    yield handle
    handle.shutdown()


def test_tools_list_omits_skill_stubs_when_configured(catalog_server_exclude_stubs):
    tools = _tools_list(catalog_server_exclude_stubs.mcp_url())
    names = [t["name"] for t in tools]
    stubs = [n for n in names if n.startswith("__skill__")]
    assert stubs == [], f"Expected no __skill__ stubs, got: {stubs}"


def test_search_tools_still_finds_unloaded_skills(catalog_server_exclude_stubs):
    url = catalog_server_exclude_stubs.mcp_url()
    # Use the exact skill name so the layer=example filter is bypassed for
    # skill_candidates (PR #1398 hides example-layer skills from partial
    # queries; exact-name matches are always surfaced regardless of layer).
    body = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "search_tools",
            "arguments": {"query": "hello-world", "include_unloaded_skills": True},
        },
    }
    resp = McpClient(url).post(body)[1]
    text = resp["result"]["content"][0]["text"]
    payload = json.loads(text)
    candidates = payload.get("skill_candidates") or []
    assert candidates, f"Expected skill_candidates in search_tools payload: {payload}"
