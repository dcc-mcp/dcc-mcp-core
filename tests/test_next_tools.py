"""Tests for the `next-tools` wiring from SKILL.md / tools.yaml to
``CallToolResult._meta["dcc.next_tools"]`` — issue #342.

These tests exercise the full pipeline:

1. Sibling-file parsing: ``tools.yaml`` with per-tool ``next-tools``
   surfaces on ``SkillMetadata.tools[i].next_tools``.
2. Legacy deprecation: a top-level ``next-tools:`` on SKILL.md parses
   but flags the skill as non-spec-compliant and lists ``next-tools``
   in ``legacy_extension_fields``.
3. End-to-end: a running ``McpHttpServer`` attaches
   ``_meta["dcc.next_tools"]["on_success"]`` after success and
   ``_meta["dcc.next_tools"]["on_failure"]`` after an error, and
   omits the key entirely when the tool declared no next-tools.
"""

from __future__ import annotations

import json
from pathlib import Path
import socket
import sys
import time
from typing import Any
import urllib.request

import pytest

import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

# ── Unit: sibling tools.yaml parses next-tools per tool ────────────────────


def _write_skill_md(skill_dir: Path, body: str) -> None:
    skill_dir.mkdir(parents=True, exist_ok=True)
    (skill_dir / "SKILL.md").write_text(body, encoding="utf-8")


def test_sibling_tools_yaml_parses_next_tools(tmp_path: Path) -> None:
    skill_dir = tmp_path / "nt-skill"
    skill_dir.mkdir()
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: create_sphere
    description: Create a polygon sphere
    next-tools:
      on-success:
        - maya_geometry__bevel_edges
        - maya_geometry__assign_material
      on-failure:
        - diagnostics__screenshot
""",
        encoding="utf-8",
    )
    _write_skill_md(
        skill_dir,
        """---
name: nt-skill
description: next-tools sibling-file test
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: tools.yaml
---
# body
""",
    )

    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
    assert meta is not None
    assert meta.is_spec_compliant()
    assert len(meta.tools) == 1
    tool = meta.tools[0]
    nt = tool.next_tools
    assert nt is not None, "next_tools dict must be exposed"
    assert nt["on_success"] == [
        "maya_geometry__bevel_edges",
        "maya_geometry__assign_material",
    ]
    assert nt["on_failure"] == ["diagnostics__screenshot"]


# ── Unit: top-level next-tools is legacy + deprecation ─────────────────────


def test_top_level_next_tools_is_legacy(tmp_path: Path) -> None:
    skill_dir = tmp_path / "legacy-nt"
    _write_skill_md(
        skill_dir,
        """---
name: legacy-nt
dcc: maya
next-tools:
  on-success: [foo]
---
""",
    )
    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
    assert meta is not None
    assert not meta.is_spec_compliant(), "A top-level next-tools: key must flag the skill as non-compliant"
    assert "next-tools" in meta.legacy_extension_fields, (
        f"legacy_extension_fields must name next-tools; got {meta.legacy_extension_fields!r}"
    )


# ── E2E: CallToolResult._meta["dcc.next_tools"] wiring ─────────────────────


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _post(url: str, body: dict[str, Any]) -> dict[str, Any]:
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
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _write_success_skill(root: Path) -> Path:
    """Write a minimal skill whose single tool succeeds and declares
    both on-success and on-failure next-tools. The script echoes its
    arguments back as a success dict.
    """
    skill_dir = root / "nt-success"
    skill_dir.mkdir()
    scripts = skill_dir / "scripts"
    scripts.mkdir()
    (scripts / "ping.py").write_text(
        """from __future__ import annotations
import json, sys
print(json.dumps({"success": True, "message": "pong"}))
""",
        encoding="utf-8",
    )
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: ping
    description: Returns pong
    source_file: scripts/ping.py
    next-tools:
      on-success: [nt_success__followup_a, nt_success__followup_b]
      on-failure: [diagnostics__screenshot]
""",
        encoding="utf-8",
    )
    _write_skill_md(
        skill_dir,
        """---
name: nt-success
description: Success skill for #342 end-to-end test
metadata:
  dcc-mcp.dcc: test
  dcc-mcp.tools: tools.yaml
---
""",
    )
    return skill_dir


def _write_failure_skill(root: Path) -> Path:
    """Tool exits with an error payload so the server marks the result
    as ``isError: true`` and surfaces the on-failure list.
    """
    skill_dir = root / "nt-failure"
    skill_dir.mkdir()
    scripts = skill_dir / "scripts"
    scripts.mkdir()
    (scripts / "boom.py").write_text(
        """from __future__ import annotations
import json, sys
sys.stderr.write("boom!\\n")
sys.exit(1)
""",
        encoding="utf-8",
    )
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: boom
    description: Always fails
    source_file: scripts/boom.py
    next-tools:
      on-failure: [diagnostics__screenshot, diagnostics__audit_log]
""",
        encoding="utf-8",
    )
    _write_skill_md(
        skill_dir,
        """---
name: nt-failure
description: Failure skill for #342 end-to-end test
metadata:
  dcc-mcp.dcc: test
  dcc-mcp.tools: tools.yaml
---
""",
    )
    return skill_dir


def _write_plain_skill(root: Path) -> Path:
    """Skill with no next-tools declared — baseline for "no _meta" case."""
    skill_dir = root / "nt-plain"
    skill_dir.mkdir()
    scripts = skill_dir / "scripts"
    scripts.mkdir()
    (scripts / "noop.py").write_text(
        """from __future__ import annotations
import json
print(json.dumps({"success": True, "message": "ok"}))
""",
        encoding="utf-8",
    )
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: noop
    description: Does nothing
    source_file: scripts/noop.py
""",
        encoding="utf-8",
    )
    _write_skill_md(
        skill_dir,
        """---
name: nt-plain
description: No next-tools declared
metadata:
  dcc-mcp.dcc: test
  dcc-mcp.tools: tools.yaml
---
""",
    )
    return skill_dir


@pytest.fixture(scope="module")
def e2e_server(tmp_path_factory):
    root = tmp_path_factory.mktemp("nt-skills")
    _write_success_skill(root)
    _write_failure_skill(root)
    _write_plain_skill(root)

    reg = ToolRegistry()
    port = _free_port()
    config = McpHttpConfig(port=port, server_name="nt-test-server")
    server = McpHttpServer(reg, config)
    server.discover(extra_paths=[str(root)])
    handle = server.start()
    # Wait briefly for the listener to be ready.
    time.sleep(0.25)
    try:
        url = handle.mcp_url()
        # Activate every declared skill so its tools are callable.
        for skill in ("nt-success", "nt-failure", "nt-plain"):
            _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 100,
                    "method": "tools/call",
                    "params": {"name": "load_skill", "arguments": {"skill_name": skill}},
                },
            )
        yield url
    finally:
        handle.shutdown()


def _call_tool(url: str, name: str, rid: int = 1) -> dict[str, Any]:
    return _post(
        url,
        {
            "jsonrpc": "2.0",
            "id": rid,
            "method": "tools/call",
            "params": {"name": name, "arguments": {}},
        },
    )


def test_tools_call_success_attaches_on_success(e2e_server: str) -> None:
    body = _call_tool(e2e_server, "nt_success__ping", rid=1)
    result = body["result"]
    assert result["isError"] is False, f"unexpected error: {result}"
    meta = result.get("_meta")
    assert meta is not None, f"expected _meta on success, got: {result}"
    nt = meta.get("dcc.next_tools")
    assert nt is not None, f"expected dcc.next_tools, got: {meta}"
    assert nt["on_success"] == [
        "nt_success__followup_a",
        "nt_success__followup_b",
    ]
    assert "on_failure" not in nt, "on_failure must be absent on a success result"


def test_tools_call_failure_attaches_on_failure(e2e_server: str) -> None:
    body = _call_tool(e2e_server, "nt_failure__boom", rid=2)
    result = body["result"]
    assert result["isError"] is True, f"expected error: {result}"
    meta = result.get("_meta")
    assert meta is not None, f"expected _meta on failure, got: {result}"
    nt = meta.get("dcc.next_tools")
    assert nt is not None, f"expected dcc.next_tools, got: {meta}"
    assert nt["on_failure"] == [
        "diagnostics__screenshot",
        "diagnostics__audit_log",
    ]
    assert "on_success" not in nt, "on_success must be absent on an error result"


def test_tools_call_no_next_tools_omits_meta(e2e_server: str) -> None:
    body = _call_tool(e2e_server, "nt_plain__noop", rid=3)
    result = body["result"]
    assert result["isError"] is False, f"unexpected error: {result}"
    # When no next-tools declared: either no _meta at all or a _meta
    # without the dcc.next_tools key. The former is preferred.
    meta = result.get("_meta")
    if meta is not None:
        assert "dcc.next_tools" not in meta, f"next_tools must be absent for a tool with no declaration, got: {meta}"
