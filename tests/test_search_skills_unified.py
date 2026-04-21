"""Integration tests for the unified ``search_skills`` MCP tool (issue #340).

The `search_skills` tool is now a superset of the deprecated `find_skills`:
it accepts `query`, `tags`, `dcc`, `scope`, and `limit`, and treats an empty
call as a "discovery" request that returns the top skills by scope
precedence.

`find_skills` still exists as a compat alias that forwards to
`search_skills`, logs a deprecation warning, and tags the response
`_meta["dcc.deprecation"]`.
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
import time
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS = REPO_ROOT / "examples" / "skills"


def _post(url: str, body: dict) -> dict:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _call_tool(url: str, name: str, arguments: dict | None = None, req_id: int = 1) -> dict:
    body = {
        "jsonrpc": "2.0",
        "id": req_id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments or {}},
    }
    return _post(url, body)


@pytest.fixture(scope="module")
def catalog_server():
    if not EXAMPLES_SKILLS.is_dir():
        pytest.skip("examples/skills directory not found")

    reg = ToolRegistry()
    config = McpHttpConfig(port=0, server_name="ci-search-skills-340")
    server = McpHttpServer(reg, config)
    server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
    handle = server.start()
    time.sleep(0.2)
    try:
        yield handle.mcp_url()
    finally:
        handle.shutdown()


# ── Unified signature ─────────────────────────────────────────────────────


class TestSearchSkillsUnifiedSignature:
    def test_query_only(self, catalog_server):
        resp = _call_tool(catalog_server, "search_skills", {"query": "hello"})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        assert payload["total"] >= 1
        names = [s["name"] for s in payload["skills"]]
        assert any("hello" in n for n in names)

    def test_empty_args_is_discovery(self, catalog_server):
        resp = _call_tool(catalog_server, "search_skills", {})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        assert payload["total"] >= 1
        # Every summary carries the new scope field.
        for s in payload["skills"]:
            assert "scope" in s

    def test_limit_caps_results(self, catalog_server):
        resp = _call_tool(catalog_server, "search_skills", {"limit": 1})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        assert payload["total"] == 1
        assert len(payload["skills"]) == 1

    def test_dcc_filter(self, catalog_server):
        resp = _call_tool(catalog_server, "search_skills", {"dcc": "maya"})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        for s in payload["skills"]:
            assert s["dcc"].lower() == "maya"

    def test_scope_filter_valid(self, catalog_server):
        # Example skills are discovered at Repo scope; this must not error.
        resp = _call_tool(catalog_server, "search_skills", {"scope": "repo"})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        for s in payload["skills"]:
            assert s["scope"] == "repo"

    def test_scope_filter_invalid_returns_error(self, catalog_server):
        resp = _call_tool(catalog_server, "search_skills", {"scope": "bogus"})
        assert resp["result"]["isError"] is True

    def test_combined_filters(self, catalog_server):
        resp = _call_tool(
            catalog_server,
            "search_skills",
            {"query": "hello", "dcc": "maya", "scope": "repo", "limit": 5},
        )
        assert resp["result"]["isError"] is False


# ── find_skills compatibility / deprecation ───────────────────────────────


class TestFindSkillsDeprecation:
    def test_find_skills_still_works(self, catalog_server):
        # The shim must return the same shape as the old call.
        resp = _call_tool(catalog_server, "find_skills", {"query": "hello"})
        assert resp["result"]["isError"] is False
        payload = json.loads(resp["result"]["content"][0]["text"])
        assert "skills" in payload
        assert payload["total"] >= 1

    def test_find_skills_attaches_deprecation_meta(self, catalog_server):
        resp = _call_tool(catalog_server, "find_skills", {"query": "hello"})
        meta = resp["result"].get("_meta", {})
        assert "dcc.deprecation" in meta, f"expected deprecation notice in _meta: {meta}"
        assert "search_skills" in meta["dcc.deprecation"]

    def test_find_skills_emits_tracing_warning(self, catalog_server, caplog):
        # `tracing` events bridge into the Python `logging` tree via `tracing-log`
        # / `env_logger`-style integrations when the Rust extension is built
        # with logging support. If no bridge is active the call still succeeds
        # and we assert the user-visible deprecation marker instead.
        with caplog.at_level(logging.WARNING):
            resp = _call_tool(catalog_server, "find_skills", {"query": "hello"})
        assert resp["result"]["isError"] is False
        messages = " ".join(r.getMessage() for r in caplog.records)
        if "find_skills" in messages:
            assert "deprecated" in messages.lower()
        # In either case the response-level deprecation marker is present.
        assert "dcc.deprecation" in resp["result"].get("_meta", {})

    def test_find_skills_tags_arg_is_forwarded(self, catalog_server):
        # `tags` was a `find_skills`-only arg; the shim must forward it.
        resp = _call_tool(catalog_server, "find_skills", {"tags": ["example"]})
        # Either matches or returns no-match text — must never isError.
        assert resp["result"]["isError"] is False


# ── Backward compatibility: same shape under both tool names ──────────────


class TestBackwardCompatSameShape:
    def test_find_and_search_return_equivalent_skills(self, catalog_server):
        old = _call_tool(catalog_server, "find_skills", {"query": "hello"})
        new = _call_tool(catalog_server, "search_skills", {"query": "hello"})

        old_payload = json.loads(old["result"]["content"][0]["text"])
        new_payload = json.loads(new["result"]["content"][0]["text"])

        old_names = sorted(s["name"] for s in old_payload["skills"])
        new_names = sorted(s["name"] for s in new_payload["skills"])

        # The search_skills set must be a superset of the find_skills set.
        for n in old_names:
            assert n in new_names, f"search_skills missing {n} returned by find_skills"


# ── Rust-level SkillCatalog binding ───────────────────────────────────────


class TestSkillCatalogPythonBinding:
    """The new `SkillCatalog.search_skills(...)` Python method (issue #340)."""

    def test_python_binding_accepts_all_args(self, tmp_path):
        from dcc_mcp_core import SkillCatalog
        from dcc_mcp_core import ToolRegistry

        reg = ToolRegistry()
        cat = SkillCatalog(reg)
        if not EXAMPLES_SKILLS.is_dir():
            pytest.skip("examples/skills directory not found")
        cat.discover([str(EXAMPLES_SKILLS)])

        results = cat.search_skills(
            query=None,
            tags=[],
            dcc=None,
            scope="repo",
            limit=3,
        )
        assert isinstance(results, list)
        assert len(results) <= 3
        for s in results:
            assert s.scope == "repo"

    def test_python_binding_rejects_invalid_scope(self, tmp_path):
        from dcc_mcp_core import SkillCatalog
        from dcc_mcp_core import ToolRegistry

        reg = ToolRegistry()
        cat = SkillCatalog(reg)
        with pytest.raises(ValueError):
            cat.search_skills(scope="bogus")
