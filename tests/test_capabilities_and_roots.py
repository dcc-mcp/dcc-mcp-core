"""Tests for issue #354 — capability declaration + workspace path handshake."""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import ToolDeclaration
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import WorkspaceRoots
from dcc_mcp_core import scan_and_load

# ── WorkspaceRoots ─────────────────────────────────────────────────────────


def test_workspace_roots_resolves_workspace_scheme(tmp_path):
    roots = WorkspaceRoots([str(tmp_path)])
    resolved = roots.resolve("workspace://scenes/hero.usd")
    assert resolved.endswith("scenes/hero.usd") or resolved.endswith("scenes\\hero.usd")
    assert str(tmp_path).replace("\\", "/") in resolved.replace("\\", "/")


def test_workspace_roots_absolute_passthrough(tmp_path):
    roots = WorkspaceRoots([str(tmp_path)])
    other = tmp_path / "other.txt"
    resolved = roots.resolve(str(other))
    assert resolved == str(other)


def test_workspace_roots_relative_joined_against_first_root(tmp_path):
    roots = WorkspaceRoots([str(tmp_path)])
    resolved = roots.resolve("scenes/a.usd")
    norm = resolved.replace("\\", "/")
    assert norm.endswith("scenes/a.usd")
    assert str(tmp_path).replace("\\", "/") in norm


def test_workspace_roots_no_roots_errors_on_workspace_scheme():
    roots = WorkspaceRoots()
    with pytest.raises(ValueError, match="no workspace roots"):
        roots.resolve("workspace://anything")


def test_workspace_roots_empty_init_returns_empty_roots_list():
    roots = WorkspaceRoots()
    assert roots.roots == []


def test_workspace_roots_uri_and_path_mixed():
    roots = WorkspaceRoots(["file:///a", "/b", "custom://x"])
    # All three are preserved verbatim for diagnostics.
    assert "file:///a" in roots.roots
    assert "custom://x" in roots.roots
    # Only file:// entries participate in resolve() (custom scheme is ignored).
    assert roots.resolve("workspace://rel").replace("\\", "/").startswith("/a/")


# ── SkillMetadata aggregation ──────────────────────────────────────────────


def test_skill_metadata_aggregates_required_capabilities():
    md = SkillMetadata("example")
    t1 = ToolDeclaration("a")
    t1.required_capabilities = ["usd", "scene.read"]
    t2 = ToolDeclaration("b")
    t2.required_capabilities = ["usd", "scene.mutate"]
    md.tools = [t1, t2]
    # Deduplicated + sorted.
    assert md.required_capabilities() == ["scene.mutate", "scene.read", "usd"]


def test_skill_metadata_no_caps_returns_empty():
    md = SkillMetadata("empty")
    md.tools = [ToolDeclaration("a")]
    assert md.required_capabilities() == []


# ── tools.yaml parsing ─────────────────────────────────────────────────────


def test_sibling_tools_yaml_parses_required_capabilities(tmp_path):
    skill_dir = tmp_path / "cap-skill"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text(
        "---\nname: cap-skill\ndescription: test\nmetadata:\n  dcc-mcp.tools: tools.yaml\n---\n",
        encoding="utf-8",
    )
    (skill_dir / "tools.yaml").write_text(
        "tools:\n"
        "  - name: import_usd\n"
        "    description: Import a USD file\n"
        "    required_capabilities: [usd, scene.mutate, filesystem.read]\n"
        "  - name: read_scene\n"
        "    required_capabilities: [scene.read]\n",
        encoding="utf-8",
    )

    skills, skipped = scan_and_load(extra_paths=[str(tmp_path)])
    assert not skipped
    cap_skill = next(s for s in skills if s.name == "cap-skill")
    tool_names = {t.name: t for t in cap_skill.tools}
    assert tool_names["import_usd"].required_capabilities == [
        "usd",
        "scene.mutate",
        "filesystem.read",
    ]
    assert tool_names["read_scene"].required_capabilities == ["scene.read"]
    # Skill-level aggregation.
    assert set(cap_skill.required_capabilities()) == {
        "usd",
        "scene.mutate",
        "filesystem.read",
        "scene.read",
    }


# ── McpHttpConfig.declared_capabilities ────────────────────────────────────


def test_mcp_http_config_declared_capabilities_default_empty():
    cfg = McpHttpConfig(port=0)
    assert cfg.declared_capabilities == []


def test_mcp_http_config_declared_capabilities_setter():
    cfg = McpHttpConfig(port=0)
    cfg.declared_capabilities = ["usd", "scene.mutate"]
    assert cfg.declared_capabilities == ["usd", "scene.mutate"]


# ── End-to-end: capability gate blocks tools/call ──────────────────────────


def _start_server_with_cap_tool(port: int, declared_caps: list[str], required_caps: list[str]):
    """Spin up an McpHttpServer exposing a single tool that declares
    `required_caps`. Returns (server, handle, tool_name).
    """
    registry = ToolRegistry()
    registry.register(
        name="import_usd",
        description="Import USD",
        dcc="test",
        required_capabilities=required_caps,
    )
    cfg = McpHttpConfig(port=port)
    cfg.declared_capabilities = declared_caps
    server = McpHttpServer(registry, cfg)
    handle = server.start()
    return server, handle, "import_usd"


def _mcp_call(handle, method: str, params: dict | None = None) -> dict:
    import urllib.request

    body = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params or {},
        }
    ).encode("utf-8")
    url = handle.mcp_url()
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        data = resp.read().decode("utf-8")
    # Strip SSE framing if present.
    for line in data.splitlines():
        if line.startswith("data:"):
            return json.loads(line[5:].strip())
    return json.loads(data)


def test_tools_call_blocks_when_capability_missing():
    _server, handle, tool_name = _start_server_with_cap_tool(
        port=0,
        declared_caps=[],  # nothing declared
        required_caps=["usd"],
    )
    try:
        # initialize
        _mcp_call(
            handle,
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest", "version": "0"},
            },
        )
        resp = _mcp_call(
            handle,
            "tools/call",
            {"name": tool_name, "arguments": {}},
        )
        assert "error" in resp, resp
        assert resp["error"]["code"] == -32001
        data = resp["error"].get("data", {})
        assert "usd" in data.get("missing_capabilities", [])
    finally:
        handle.shutdown()


def test_tools_call_succeeds_when_capability_declared():
    _server, handle, tool_name = _start_server_with_cap_tool(
        port=0,
        declared_caps=["usd", "scene.mutate"],
        required_caps=["usd"],
    )
    try:
        _mcp_call(
            handle,
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest", "version": "0"},
            },
        )
        resp = _mcp_call(
            handle,
            "tools/call",
            {"name": tool_name, "arguments": {}},
        )
        # Without a handler the dispatcher returns an error, but NOT the
        # capability-gate error. Anything that is not -32001 proves the
        # capability check passed.
        if "error" in resp:
            assert resp["error"]["code"] != -32001, resp
    finally:
        handle.shutdown()


def test_tools_list_meta_includes_missing_capabilities():
    _server, handle, tool_name = _start_server_with_cap_tool(
        port=0,
        declared_caps=["usd"],
        required_caps=["usd", "fluid"],
    )
    try:
        _mcp_call(
            handle,
            "initialize",
            {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest", "version": "0"},
            },
        )
        resp = _mcp_call(handle, "tools/list", {})
        tools = resp["result"]["tools"]
        entry = next(t for t in tools if t["name"] == tool_name)
        meta = entry.get("_meta", {}).get("dcc", {})
        assert meta.get("required_capabilities") == ["usd", "fluid"]
        assert meta.get("missing_capabilities") == ["fluid"]
    finally:
        handle.shutdown()
