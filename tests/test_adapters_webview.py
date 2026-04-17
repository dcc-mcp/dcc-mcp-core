"""Tests for WebViewAdapter + ToolRegistry.required_capabilities (#211)."""

from __future__ import annotations

# Import built-in modules
from typing import Any

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import CAPABILITY_KEYS
from dcc_mcp_core import WEBVIEW_DEFAULT_CAPABILITIES
from dcc_mcp_core import WebViewAdapter
from dcc_mcp_core import WebViewContext


# ── Public API shape ──────────────────────────────────────────────────────────


class TestAdaptersPublicApi:
    def test_capability_keys_is_frozen(self) -> None:
        assert isinstance(CAPABILITY_KEYS, frozenset)
        assert {"scene", "timeline", "selection", "undo", "render"} == CAPABILITY_KEYS

    def test_default_capabilities_are_all_false(self) -> None:
        assert set(WEBVIEW_DEFAULT_CAPABILITIES) == CAPABILITY_KEYS
        assert all(v is False for v in WEBVIEW_DEFAULT_CAPABILITIES.values())

    def test_defaults_are_fresh_copies(self) -> None:
        """Mutating the default map should not leak into WebViewAdapter.capabilities."""
        local = dict(WEBVIEW_DEFAULT_CAPABILITIES)
        local["scene"] = True
        assert WEBVIEW_DEFAULT_CAPABILITIES["scene"] is False
        assert WebViewAdapter.capabilities["scene"] is False

    def test_adapters_reexport_from_top_level(self) -> None:
        assert dcc_mcp_core.WebViewAdapter is WebViewAdapter
        assert dcc_mcp_core.WebViewContext is WebViewContext


# ── WebViewAdapter contract ───────────────────────────────────────────────────


class TestWebViewAdapterContract:
    def test_default_capabilities_match_module_default(self) -> None:
        assert WebViewAdapter.capabilities == WEBVIEW_DEFAULT_CAPABILITIES

    def test_default_dcc_name_is_webview(self) -> None:
        assert WebViewAdapter.dcc_name == "webview"

    def test_abstract_methods_raise_not_implemented(self) -> None:
        adapter = WebViewAdapter()
        with pytest.raises(NotImplementedError):
            adapter.get_context()
        with pytest.raises(NotImplementedError):
            adapter.list_tools()
        with pytest.raises(NotImplementedError):
            adapter.execute("any_tool", {"x": 1})

    def test_get_audit_log_default_empty(self) -> None:
        assert WebViewAdapter().get_audit_log() == []

    def test_advertised_capabilities_returns_fresh_copy(self) -> None:
        first = WebViewAdapter.advertised_capabilities()
        first["scene"] = True
        second = WebViewAdapter.advertised_capabilities()
        assert second["scene"] is False

    def test_supports_false_for_default(self) -> None:
        for key in CAPABILITY_KEYS:
            assert WebViewAdapter.supports(key) is False

    def test_supports_returns_false_for_unknown_key(self) -> None:
        assert WebViewAdapter.supports("does_not_exist") is False

    def test_matches_requirements_empty_always_true(self) -> None:
        assert WebViewAdapter.matches_requirements([]) is True

    def test_matches_requirements_default_false_for_scene(self) -> None:
        assert WebViewAdapter.matches_requirements(["scene"]) is False


# ── Subclass overrides ────────────────────────────────────────────────────────


class AuroraLikeAdapter(WebViewAdapter):
    """Mimics AuroraView: advertises undo only, routes through a fake bridge."""

    dcc_name = "auroraview"
    capabilities = {**WEBVIEW_DEFAULT_CAPABILITIES, "undo": True}

    def __init__(self) -> None:
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def get_context(self) -> WebViewContext:
        return WebViewContext(
            window_title="AuroraView",
            url="http://localhost:3000",
            pid=12345,
            cdp_port=9222,
            host_dcc="maya",
        )

    def list_tools(self) -> list[dict[str, Any]]:
        return [{"name": "undo", "description": "Undo last action"}]

    def execute(self, tool: str, params: Any = None) -> dict[str, Any]:
        self.calls.append((tool, dict(params or {})))
        return {"success": True, "tool": tool}


class TestWebViewAdapterSubclass:
    def test_subclass_advertises_undo(self) -> None:
        assert AuroraLikeAdapter.supports("undo") is True
        assert AuroraLikeAdapter.supports("scene") is False

    def test_matches_requirements_on_subclass(self) -> None:
        assert AuroraLikeAdapter.matches_requirements(["undo"]) is True
        assert AuroraLikeAdapter.matches_requirements(["undo", "scene"]) is False

    def test_get_context_returns_webview_context(self) -> None:
        ctx = AuroraLikeAdapter().get_context()
        assert isinstance(ctx, WebViewContext)
        assert ctx["host_dcc"] == "maya"
        assert ctx["cdp_port"] == 9222

    def test_execute_records_call(self) -> None:
        adapter = AuroraLikeAdapter()
        result = adapter.execute("undo", {"count": 1})
        assert result == {"success": True, "tool": "undo"}
        assert adapter.calls == [("undo", {"count": 1})]

    def test_subclass_capabilities_do_not_leak_upward(self) -> None:
        """A subclass tweak must not rewrite the base class map."""
        assert AuroraLikeAdapter.capabilities["undo"] is True
        assert WebViewAdapter.capabilities["undo"] is False
