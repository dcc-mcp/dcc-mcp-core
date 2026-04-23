"""Tests for issues #404-#411 implementations.

Covers the pure-Python modules added in this feature branch:
- batch.py (issue #406)
- elicitation.py (issue #407)
- auth.py (issue #408)
- rich_content.py (issue #409)
- plugin_manifest.py (issue #410)
- dcc_api_executor.py (issue #411)

These modules are pure-Python and have no dependency on the compiled _core
extension.  They import directly from dcc_mcp_core sub-modules after the
package has been built with maturin develop.
"""

from __future__ import annotations

import json
import pathlib

import pytest

import dcc_mcp_core.auth as _auth
import dcc_mcp_core.batch as _batch
import dcc_mcp_core.dcc_api_executor as _dcc_api_executor
import dcc_mcp_core.elicitation as _elicitation
import dcc_mcp_core.plugin_manifest as _plugin_manifest
import dcc_mcp_core.rich_content as _rich_content

# ── issue #406: batch_dispatch + EvalContext ─────────────────────────────────


class FakeDispatcher:
    """Minimal ToolDispatcher stand-in for testing."""

    def dispatch(self, name: str, json_str: str) -> dict:
        args = json.loads(json_str)
        return {"action": name, "output": {"success": True, "result": args}}


class TestBatchDispatch:
    def test_list_aggregate(self):
        batch_dispatch = _batch.batch_dispatch
        d = FakeDispatcher()
        result = batch_dispatch(d, [("tool_a", {"x": 1}), ("tool_b", {"y": 2})], aggregate="list")
        assert result["total"] == 2
        assert result["succeeded"] == 2
        assert len(result["results"]) == 2

    def test_merge_aggregate(self):
        batch_dispatch = _batch.batch_dispatch
        d = FakeDispatcher()
        result = batch_dispatch(d, [("tool_a", {"a": 1}), ("tool_b", {"b": 2})], aggregate="merge")
        merged = result["merged"]
        assert "success" in merged

    def test_last_aggregate(self):
        batch_dispatch = _batch.batch_dispatch
        d = FakeDispatcher()
        result = batch_dispatch(d, [("tool_a", {}), ("tool_b", {})], aggregate="last")
        assert result["last"]["action"] == "tool_b"

    def test_empty_calls(self):
        batch_dispatch = _batch.batch_dispatch
        d = FakeDispatcher()
        result = batch_dispatch(d, [])
        assert result["total"] == 0
        assert result["succeeded"] == 0

    def test_stop_on_error(self):
        batch_dispatch = _batch.batch_dispatch

        class FailDispatcher:
            def dispatch(self, name: str, json_str: str) -> dict:
                return {"action": name, "output": {"success": False, "message": "fail"}}

        result = batch_dispatch(
            FailDispatcher(),
            [("tool_a", {}), ("tool_b", {})],
            stop_on_error=True,
        )
        assert result["total"] == 2
        assert len(result["errors"]) >= 1


class TestEvalContext:
    def test_simple_return(self):
        EvalContext = _batch.EvalContext
        ctx = EvalContext(FakeDispatcher())
        result = ctx.run("return 42")
        assert result == 42

    def test_dispatch_in_script(self):
        EvalContext = _batch.EvalContext
        ctx = EvalContext(FakeDispatcher())
        result = ctx.run("r = dispatch('my_tool', {'val': 99})\nreturn r['action']")
        assert result == "my_tool"

    def test_no_return(self):
        EvalContext = _batch.EvalContext
        ctx = EvalContext(FakeDispatcher())
        result = ctx.run("x = 1 + 1")
        assert result is None

    def test_runtime_error(self):
        EvalContext = _batch.EvalContext
        ctx = EvalContext(FakeDispatcher())
        with pytest.raises(RuntimeError):
            ctx.run("raise ValueError('deliberate')")


# ── issue #407: elicitation ──────────────────────────────────────────────────


class TestElicitation:
    def test_elicit_form_returns_not_supported(self):
        import asyncio

        elicit_form = _elicitation.elicit_form
        resp = asyncio.run(elicit_form("Choose quality", {"type": "object", "properties": {}}))
        assert resp.accepted is False
        assert resp.message == "elicitation_not_supported"

    def test_elicit_url_returns_not_supported(self):
        import asyncio

        elicit_url = _elicitation.elicit_url
        resp = asyncio.run(elicit_url("Authorize", "https://example.com/oauth"))
        assert resp.accepted is False

    def test_elicit_form_sync_fallback(self):
        elicit_form_sync = _elicitation.elicit_form_sync
        resp = elicit_form_sync("Select", {}, fallback_values={"quality": "high"})
        assert resp.accepted is True
        assert resp.data == {"quality": "high"}

    def test_elicit_form_sync_no_fallback(self):
        elicit_form_sync = _elicitation.elicit_form_sync
        resp = elicit_form_sync("Select", {})
        assert resp.accepted is False

    def test_dataclass_fields(self):
        FormElicitation = _elicitation.FormElicitation
        ElicitationResponse = _elicitation.ElicitationResponse
        ElicitationMode = _elicitation.ElicitationMode
        fe = FormElicitation(message="test", schema={"type": "object"})
        assert fe.title is None
        resp = ElicitationResponse(accepted=True, data={"x": 1})
        assert resp.data == {"x": 1}
        assert ElicitationMode.FORM == "form"


# ── issue #408: auth ──────────────────────────────────────────────────────────


class TestAuth:
    def test_validate_bearer_token_ok(self):
        validate_bearer_token = _auth.validate_bearer_token
        assert validate_bearer_token({"Authorization": "Bearer secret"}, expected_token="secret") is True

    def test_validate_bearer_token_wrong(self):
        validate_bearer_token = _auth.validate_bearer_token
        assert validate_bearer_token({"Authorization": "Bearer wrong"}, expected_token="secret") is False

    def test_validate_bearer_token_missing_header(self):
        validate_bearer_token = _auth.validate_bearer_token
        assert validate_bearer_token({}, expected_token="secret") is False

    def test_validate_bearer_token_no_auth(self):
        validate_bearer_token = _auth.validate_bearer_token
        assert validate_bearer_token({}, expected_token=None) is True

    def test_generate_api_key(self):
        generate_api_key = _auth.generate_api_key
        key1 = generate_api_key()
        key2 = generate_api_key()
        assert len(key1) > 10
        assert key1 != key2

    def test_api_key_config_resolve(self, monkeypatch):
        ApiKeyConfig = _auth.ApiKeyConfig
        cfg = ApiKeyConfig(api_key="direct-key")
        assert cfg.resolve() == "direct-key"

        cfg2 = ApiKeyConfig(env_var="MY_TEST_KEY")
        monkeypatch.setenv("MY_TEST_KEY", "env-key")
        assert cfg2.resolve() == "env-key"

    def test_cimd_document(self):
        CimdDocument = _auth.CimdDocument
        doc = CimdDocument(client_name="test-mcp", redirect_uris=["http://localhost/cb"])
        d = doc.to_dict()
        assert d["client_name"] == "test-mcp"
        assert "redirect_uris" in d

    def test_oauth_config_cimd(self):
        OAuthConfig = _auth.OAuthConfig
        cfg = OAuthConfig(
            provider_url="https://auth.example.com",
            scopes=["read", "write"],
            client_name="test",
        )
        cimd = cfg.to_cimd_document(redirect_uri="http://localhost:8765/cb")
        assert cimd.client_name == "test"
        assert cimd.scope == "read write"
        assert "http://localhost:8765/cb" in cimd.redirect_uris


# ── issue #409: rich_content ──────────────────────────────────────────────────


class TestRichContent:
    def test_chart(self):
        RichContent = _rich_content.RichContent
        RichContentKind = _rich_content.RichContentKind
        spec = {"mark": "bar", "data": {"values": []}}
        rc = RichContent.chart(spec)
        assert rc.kind == RichContentKind.CHART
        d = rc.to_dict()
        assert d["kind"] == "chart"
        assert d["spec"] == spec

    def test_table(self):
        RichContent = _rich_content.RichContent
        rc = RichContent.table(["Name", "Value"], [["a", 1], ["b", 2]], title="My Table")
        d = rc.to_dict()
        assert d["headers"] == ["Name", "Value"]
        assert d["title"] == "My Table"

    def test_image(self):
        RichContent = _rich_content.RichContent
        rc = RichContent.image(b"\x89PNG", "image/png", alt="test")
        d = rc.to_dict()
        assert d["kind"] == "image"
        assert d["mime"] == "image/png"
        assert d["alt"] == "test"

    def test_image_from_file(self, tmp_path):
        RichContent = _rich_content.RichContent
        p = tmp_path / "test.png"
        p.write_bytes(b"\x89PNG\r\n\x1a\n")
        rc = RichContent.image_from_file(p)
        assert rc.kind.value == "image"

    def test_dashboard(self):
        RichContent = _rich_content.RichContent
        children = [RichContent.chart({"mark": "point"}), RichContent.table(["A"], [["a"]])]
        dash = RichContent.dashboard(children)
        d = dash.to_dict()
        assert len(d["components"]) == 2

    def test_skill_success_with_chart(self):
        skill_success_with_chart = _rich_content.skill_success_with_chart
        result = skill_success_with_chart("Done", {"mark": "bar"}, total=5)
        assert result["success"] is True
        assert "__rich__" in result["context"]
        assert result["context"]["__rich__"]["kind"] == "chart"

    def test_skill_success_with_table(self):
        skill_success_with_table = _rich_content.skill_success_with_table
        result = skill_success_with_table("Listed", ["A", "B"], [["a", "b"]])
        assert result["context"]["__rich__"]["kind"] == "table"

    def test_skill_success_with_image(self):
        skill_success_with_image = _rich_content.skill_success_with_image
        result = skill_success_with_image("Captured", image_data=b"\x89PNG")
        assert result["context"]["__rich__"]["kind"] == "image"

    def test_skill_success_with_image_no_data_raises(self):
        skill_success_with_image = _rich_content.skill_success_with_image
        with pytest.raises(ValueError):
            skill_success_with_image("Captured")


# ── issue #410: plugin_manifest ───────────────────────────────────────────────


class TestPluginManifest:
    def test_build_basic(self):
        build_plugin_manifest = _plugin_manifest.build_plugin_manifest
        m = build_plugin_manifest("maya", "http://localhost:8765/mcp", version="1.0.0")
        assert m["name"] == "maya-mcp"
        assert m["version"] == "1.0.0"
        assert len(m["mcp_servers"]) == 1
        assert m["mcp_servers"][0]["url"] == "http://localhost:8765/mcp"

    def test_build_with_api_key(self):
        build_plugin_manifest = _plugin_manifest.build_plugin_manifest
        m = build_plugin_manifest("maya", "http://localhost:8765/mcp", api_key="secret")
        assert "Authorization" in m["mcp_servers"][0].get("headers", {})

    def test_build_no_mcp_url(self):
        build_plugin_manifest = _plugin_manifest.build_plugin_manifest
        m = build_plugin_manifest("maya", None)
        assert len(m["mcp_servers"]) == 0

    def test_build_filters_nonexistent_paths(self, tmp_path):
        build_plugin_manifest = _plugin_manifest.build_plugin_manifest
        real_dir = tmp_path / "skills"
        real_dir.mkdir()
        fake_dir = "/nonexistent/skills/dir"

        m = build_plugin_manifest("maya", None, [str(real_dir), fake_dir])
        assert str(real_dir) in m["skills"]
        assert fake_dir not in m["skills"]

    def test_export(self, tmp_path):
        build_plugin_manifest = _plugin_manifest.build_plugin_manifest
        export_plugin_manifest = _plugin_manifest.export_plugin_manifest
        m = build_plugin_manifest("maya", "http://localhost:8765/mcp")
        out = export_plugin_manifest(m, tmp_path / "claude_plugin.json")
        assert out.exists()
        loaded = json.loads(out.read_text())
        assert loaded["name"] == "maya-mcp"

    def test_plugin_manifest_dataclass(self):
        PluginManifest = _plugin_manifest.PluginManifest
        pm = PluginManifest(
            name="test",
            version="1.0",
            description="test",
            mcp_servers=[],
            skills=[],
        )
        d = pm.to_dict()
        assert "sub_agents" not in d  # empty list omitted


# ── issue #411: DccApiExecutor ────────────────────────────────────────────────


class TestDccApiCatalog:
    def test_empty_catalog(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        cat = DccApiCatalog("maya")
        assert len(cat) == 0

    def test_search_finds_match(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        cat = DccApiCatalog(
            "maya",
            commands=[
                {"name": "polySphere", "description": "Create a polygon sphere"},
                {"name": "polyCube", "description": "Create a polygon cube"},
                {"name": "xform", "description": "Transform objects"},
            ],
        )
        results = cat.search("polygon sphere")
        assert any("polySphere" in r["name"] for r in results)

    def test_search_no_results(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        cat = DccApiCatalog("maya", commands=[{"name": "polySphere", "description": "sphere"}])
        results = cat.search("totally unrelated xyz")
        assert results == []

    def test_parse_catalog_text(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        cat = DccApiCatalog("maya", catalog_text="polySphere - Create polygon sphere\nxform - Transform")
        assert len(cat) == 2
        results = cat.search("transform")
        assert any("xform" in r["name"] for r in results)

    def test_search_limit(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        commands = [{"name": f"tool_{i}", "description": "polygon mesh"} for i in range(20)]
        cat = DccApiCatalog("maya", commands=commands)
        results = cat.search("polygon", limit=5)
        assert len(results) <= 5


class TestDccApiExecutor:
    def test_search(self):
        DccApiCatalog = _dcc_api_executor.DccApiCatalog
        DccApiExecutor = _dcc_api_executor.DccApiExecutor
        cat = DccApiCatalog(
            "maya",
            commands=[
                {"name": "polySphere", "description": "Create sphere"},
            ],
        )
        ex = DccApiExecutor("maya", catalog=cat)
        result = ex.search("sphere")
        assert result["success"] is True
        assert len(result["results"]) >= 1

    def test_search_no_results(self):
        DccApiExecutor = _dcc_api_executor.DccApiExecutor
        ex = DccApiExecutor("maya")
        result = ex.search("completely unrelated xyz abc")
        assert result["success"] is True
        assert result["results"] == []
        assert "hint" in result

    def test_execute_simple(self):
        DccApiExecutor = _dcc_api_executor.DccApiExecutor
        ex = DccApiExecutor("maya", dispatcher=FakeDispatcher())
        result = ex.execute("return 1 + 1")
        assert result["success"] is True
        assert result["output"] == 2

    def test_execute_error(self):
        DccApiExecutor = _dcc_api_executor.DccApiExecutor
        ex = DccApiExecutor("maya")
        result = ex.execute("raise ValueError('oops')")
        assert result["success"] is False
        assert "oops" in result["error"]
