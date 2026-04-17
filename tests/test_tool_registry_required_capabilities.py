"""Tests for ToolRegistry.register(required_capabilities=…) storage path (#211)."""

from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import WebViewAdapter


@pytest.fixture
def registry() -> ToolRegistry:
    return ToolRegistry()


class TestRequiredCapabilitiesField:
    def test_default_empty_list(self, registry: ToolRegistry) -> None:
        """Empty requirements are omitted from the metadata dict for back-compat."""
        registry.register(name="no_caps_tool", dcc="python")
        meta = registry.get_action("no_caps_tool")
        assert meta is not None
        # Key is skipped via ``skip_serializing_if = "Vec::is_empty"`` — existing
        # services.json consumers keep byte-identical output when no caps apply.
        assert meta.get("required_capabilities", []) == []

    def test_passes_through_list(self, registry: ToolRegistry) -> None:
        registry.register(
            name="scene_tool",
            dcc="maya",
            required_capabilities=["scene", "selection"],
        )
        meta = registry.get_action("scene_tool")
        assert meta is not None
        assert meta["required_capabilities"] == ["scene", "selection"]

    def test_none_equivalent_to_empty(self, registry: ToolRegistry) -> None:
        registry.register(name="none_caps_tool", required_capabilities=None)
        meta = registry.get_action("none_caps_tool")
        assert meta is not None
        assert meta.get("required_capabilities", []) == []

    def test_preserves_order(self, registry: ToolRegistry) -> None:
        registry.register(
            name="ordered_caps",
            required_capabilities=["render", "scene", "undo"],
        )
        meta = registry.get_action("ordered_caps")
        assert meta is not None
        assert meta["required_capabilities"] == ["render", "scene", "undo"]

    def test_allows_custom_capability_keys(self, registry: ToolRegistry) -> None:
        """Registry is a dumb store — unknown keys pass through unfiltered."""
        registry.register(
            name="custom_caps",
            required_capabilities=["custom_host_feature"],
        )
        meta = registry.get_action("custom_caps")
        assert meta is not None
        assert meta["required_capabilities"] == ["custom_host_feature"]


class TestCapabilitiesWithWebviewAdapter:
    """Integration sanity: WebViewAdapter.matches_requirements + registry metadata."""

    def test_webview_hides_scene_tool(self, registry: ToolRegistry) -> None:
        registry.register(name="make_sphere", required_capabilities=["scene"])
        meta = registry.get_action("make_sphere")
        assert meta is not None
        assert not WebViewAdapter.matches_requirements(meta["required_capabilities"])

    def test_webview_shows_uncapped_tool(self, registry: ToolRegistry) -> None:
        registry.register(name="echo", required_capabilities=[])
        meta = registry.get_action("echo")
        assert meta is not None
        assert WebViewAdapter.matches_requirements(meta.get("required_capabilities", []))


class TestRequiredCapabilitiesSearchListRoundTrip:
    def test_field_present_in_list_actions(self, registry: ToolRegistry) -> None:
        registry.register(
            name="timeline_tool",
            dcc="maya",
            required_capabilities=["timeline"],
        )
        found = [m for m in registry.list_actions() if m["name"] == "timeline_tool"]
        assert len(found) == 1
        assert found[0]["required_capabilities"] == ["timeline"]

    def test_field_present_in_search_actions(self, registry: ToolRegistry) -> None:
        registry.register(
            name="select_tool",
            tags=["selection"],
            dcc="blender",
            required_capabilities=["selection"],
        )
        results = registry.search_actions(tags=["selection"])
        assert len(results) == 1
        assert results[0]["required_capabilities"] == ["selection"]


def test_capability_keys_exposed() -> None:
    """Sanity: predefined capability keys are importable from the package root."""
    assert "scene" in dcc_mcp_core.CAPABILITY_KEYS
    assert "timeline" in dcc_mcp_core.CAPABILITY_KEYS
    assert "selection" in dcc_mcp_core.CAPABILITY_KEYS
