"""E2E test: Python McpHttpServer prompts registration (issue #792).

Validates the acceptance criterion:

> register a prompt from Python → ``prompts/list`` → ``prompts/get``
> returns the expected rendered content.

The test
1. Creates a :class:`~dcc_mcp_core.McpHttpServer` with
   ``enable_prompts = True``.
2. Registers a prompt via ``server.prompts().register_prompt(...)``
3. Starts the server and calls ``prompts/list`` — the prompt MUST
   appear in the result.
4. Calls ``prompts/get`` with the required arguments — the returned
   ``messages[0].content.text`` MUST be the rendered template.
5. Cleans up via ``handle.shutdown()``.

No external DCC or gateway is required — the test exercises the
in-process MCP HTTP server directly.
"""

from __future__ import annotations

import contextlib
import json
import socket
import time
import urllib.request

from conftest import McpClient
from conftest import allocate_gateway_port
from conftest import wait_tcp_reachable
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import PromptHandle
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import create_skill_server

# ── helpers ──────────────────────────────────────────────────────────


def _post_mcp(url: str, method: str, params: dict | None = None, rpc_id: int = 1, timeout: float = 10.0) -> dict:
    client = McpClient(url)
    body = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        body["params"] = params
    _, resp = client.post(body)
    return resp


# ── test ─────────────────────────────────────────────────────────────────


def test_python_prompt_registration_e2e():
    """Register prompt from Python → list → get → rendered."""
    port = allocate_gateway_port()
    cfg = McpHttpConfig(port=port)
    cfg.enable_prompts = True

    registry = ToolRegistry()
    server = McpHttpServer(registry, cfg)

    assert PromptHandle is not None

    # Register a prompt before start()
    handle = server.prompts()
    assert isinstance(handle, PromptHandle)
    handle.register_prompt(
        name="bake_animation",
        description="Bake animation across frame range",
        template="Bake from {{start}} to {{end}}",
        arguments=[
            {"name": "start", "description": "Start frame", "required": True},
            {"name": "end", "description": "End frame", "required": True},
        ],
    )

    # Start the server
    server_handle = server.start()
    assert wait_tcp_reachable("127.0.0.1", server_handle.port, budget=3.0), (
        f"server port {server_handle.port} unreachable"
    )

    mcp_url = f"http://127.0.0.1:{server_handle.port}/mcp"

    try:
        # 1. prompts/list must include our prompt
        list_resp = _post_mcp(mcp_url, "prompts/list")
        assert "result" in list_resp, f"prompts/list failed: {list_resp}"
        prompts = list_resp["result"].get("prompts", [])
        by_name = {p["name"]: p for p in prompts}
        assert "bake_animation" in by_name, f"prompts/list missing 'bake_animation', got {list(by_name)}"
        args = by_name["bake_animation"].get("arguments", [])
        assert args[0].get("description") == "Start frame"

        # 2. prompts/get with correct args must render template
        get_resp = _post_mcp(
            mcp_url,
            "prompts/get",
            {
                "name": "bake_animation",
                "arguments": {"start": "1", "end": "100"},
            },
        )
        assert "result" in get_resp, f"prompts/get failed: {get_resp}"
        result = get_resp["result"]
        assert "messages" in result
        assert len(result["messages"]) > 0
        content = result["messages"][0]["content"]
        # content is a dict {"type": "text", "text": "..."} per MCP spec
        rendered = content if isinstance(content, str) else content.get("text", "")
        assert "Bake from 1 to 100" in rendered, f"prompt not rendered correctly, got: {content}"

        # 3. prompts/get with missing required arg must error
        err_resp = _post_mcp(
            mcp_url,
            "prompts/get",
            {
                "name": "bake_animation",
                "arguments": {"start": "1"},
            },
        )
        assert "error" in err_resp, f"expected error for missing 'end', got {err_resp}"

    finally:
        with contextlib.suppress(Exception):
            server_handle.shutdown()


def test_skill_examples_metadata_derives_prompt_e2e(tmp_path):
    """Loaded skills can derive prompts from metadata.dcc-mcp.examples."""
    skill_parent = tmp_path / "skills"
    skill_root = skill_parent / "scene-review"
    references = skill_root / "references"
    references.mkdir(parents=True)
    (skill_root / "SKILL.md").write_text(
        """---
name: scene-review
description: "Scene review examples. Use when validating scene state."
license: MIT
compatibility: Python 3.7+
metadata:
  dcc-mcp:
    dcc: maya
    layer: example
    examples: references/EXAMPLES.md
---

# Scene review
""",
        encoding="utf-8",
    )
    (references / "EXAMPLES.md").write_text(
        "Example: call `scene_review__inspect_scene` before export.",
        encoding="utf-8",
    )

    port = allocate_gateway_port()
    cfg = McpHttpConfig(port=port)
    cfg.enable_prompts = True
    server = create_skill_server("maya", cfg, extra_paths=[str(skill_parent)], accumulated=False)
    server_handle = server.start()
    assert wait_tcp_reachable("127.0.0.1", server_handle.port, budget=3.0), (
        f"server port {server_handle.port} unreachable"
    )

    mcp_url = f"http://127.0.0.1:{server_handle.port}/mcp"
    try:
        load_resp = _post_mcp(
            mcp_url,
            "tools/call",
            {"name": "load_skill", "arguments": {"skill_name": "scene-review"}},
        )
        assert "error" not in load_resp, f"load_skill failed: {load_resp.get('error')}"

        list_resp = _post_mcp(mcp_url, "prompts/list")
        assert "result" in list_resp, f"prompts/list failed: {list_resp}"
        prompts = {p["name"]: p for p in list_resp["result"]["prompts"]}
        prompt = prompts["scene-review.examples"]
        source = prompt["_meta"]["dcc.prompt_source"]
        assert source == {"skill": "scene-review", "source": "examples"}

        get_resp = _post_mcp(mcp_url, "prompts/get", {"name": "scene-review.examples"})
        assert "result" in get_resp, f"prompts/get failed: {get_resp}"
        text = get_resp["result"]["messages"][0]["content"]["text"]
        assert "scene_review__inspect_scene" in text
    finally:
        with contextlib.suppress(Exception):
            server_handle.shutdown()
