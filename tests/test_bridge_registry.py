"""Tests for the BridgeRegistry Python API.

Covers: BridgeContext, BridgeRegistry, get_bridge_context, register_bridge,
DccCapabilities.uses_bridge / http_bridge / websocket_bridge.

Run:  pytest tests/test_bridge_registry.py -v
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import BridgeContext
from dcc_mcp_core import BridgeRegistry
from dcc_mcp_core import DccCapabilities

# ── BridgeContext ────────────────────────────────────────────────────────


class TestBridgeContext:
    """Tests for the BridgeContext pyclass."""

    def test_bridge_context_attributes(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        ctx = registry.get("photoshop")
        assert ctx is not None
        assert ctx.dcc_type == "photoshop"
        assert ctx.bridge_url == "ws://localhost:9001"
        assert ctx.connected is True

    def test_bridge_context_repr(self) -> None:
        registry = BridgeRegistry()
        registry.register("zbrush", "http://localhost:8765")
        ctx = registry.get("zbrush")
        assert ctx is not None
        r = repr(ctx)
        assert "zbrush" in r
        assert "http://localhost:8765" in r
        assert "connected=" in r and "true" in r.lower()


# ── BridgeRegistry ──────────────────────────────────────────────────────


class TestBridgeRegistry:
    """Tests for the BridgeRegistry pyclass (local instance)."""

    def test_new_registry_is_empty(self) -> None:
        registry = BridgeRegistry()
        assert registry.is_empty()
        assert len(registry) == 0

    def test_register_and_get(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        assert not registry.is_empty()
        assert len(registry) == 1

        ctx = registry.get("photoshop")
        assert ctx is not None
        assert ctx.dcc_type == "photoshop"
        assert ctx.bridge_url == "ws://localhost:9001"
        assert ctx.connected is True

    def test_get_missing_returns_none(self) -> None:
        registry = BridgeRegistry()
        assert registry.get("nonexistent") is None

    def test_get_url(self) -> None:
        registry = BridgeRegistry()
        registry.register("zbrush", "http://localhost:8765")
        assert registry.get_url("zbrush") == "http://localhost:8765"
        assert registry.get_url("nonexistent") is None

    def test_register_multiple_bridges(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        registry.register("zbrush", "http://localhost:8765")
        assert len(registry) == 2
        assert registry.contains("photoshop")
        assert registry.contains("zbrush")

    def test_list_all(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        registry.register("zbrush", "http://localhost:8765")
        all_bridges = registry.list_all()
        assert len(all_bridges) == 2
        names = {ctx.dcc_type for ctx in all_bridges}
        assert names == {"photoshop", "zbrush"}

    def test_register_update_overwrites(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        registry.register("photoshop", "ws://localhost:9999")
        ctx = registry.get("photoshop")
        assert ctx is not None
        assert ctx.bridge_url == "ws://localhost:9999"
        assert len(registry) == 1

    def test_set_disconnected(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        assert registry.get("photoshop").connected is True

        registry.set_disconnected("photoshop")
        assert registry.get("photoshop").connected is False
        assert len(registry) == 1  # Still in registry

    def test_unregister(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        assert registry.contains("photoshop")

        registry.unregister("photoshop")
        assert not registry.contains("photoshop")
        assert registry.is_empty()

    def test_clear(self) -> None:
        registry = BridgeRegistry()
        registry.register("photoshop", "ws://localhost:9001")
        registry.register("zbrush", "http://localhost:8765")
        assert len(registry) == 2

        registry.clear()
        assert registry.is_empty()

    def test_contains(self) -> None:
        registry = BridgeRegistry()
        assert not registry.contains("photoshop")
        registry.register("photoshop", "ws://localhost:9001")
        assert registry.contains("photoshop")

    def test_len(self) -> None:
        registry = BridgeRegistry()
        assert len(registry) == 0
        registry.register("a", "ws://a")
        assert len(registry) == 1
        registry.register("b", "ws://b")
        assert len(registry) == 2

    def test_repr(self) -> None:
        registry = BridgeRegistry()
        r = repr(registry)
        assert "BridgeRegistry" in r
        assert "count=0" in r

    def test_register_empty_dcc_type_raises(self) -> None:
        registry = BridgeRegistry()
        with pytest.raises(ValueError, match="dcc_type"):
            registry.register("", "ws://localhost:9001")

    def test_register_empty_url_raises(self) -> None:
        registry = BridgeRegistry()
        with pytest.raises(ValueError, match="url"):
            registry.register("photoshop", "")

    def test_unregister_missing_raises(self) -> None:
        registry = BridgeRegistry()
        with pytest.raises(ValueError, match="not found"):
            registry.unregister("nonexistent")

    def test_set_disconnected_missing_raises(self) -> None:
        registry = BridgeRegistry()
        with pytest.raises(ValueError, match="not found"):
            registry.set_disconnected("nonexistent")


# ── Global bridge functions ─────────────────────────────────────────────


class TestGlobalBridgeFunctions:
    """Tests for get_bridge_context() and register_bridge() on the global singleton."""

    def test_register_and_get_global(self) -> None:
        """Register and query via the global singleton functions."""
        # Use a unique DCC type to avoid collisions with other tests
        dcc_mcp_core.register_bridge("_test_photoshop", "ws://localhost:9001")
        ctx = dcc_mcp_core.get_bridge_context("_test_photoshop")
        assert ctx is not None
        assert ctx.dcc_type == "_test_photoshop"
        assert ctx.bridge_url == "ws://localhost:9001"
        assert ctx.connected is True

    def test_get_bridge_context_missing(self) -> None:
        assert dcc_mcp_core.get_bridge_context("_nonexistent_dcc") is None

    def test_register_bridge_empty_raises(self) -> None:
        with pytest.raises(ValueError):
            dcc_mcp_core.register_bridge("", "ws://localhost:9001")

    def test_get_bridge_context_returns_context_not_string(self) -> None:
        """Verify get_bridge_context returns BridgeContext, not just a URL string."""
        dcc_mcp_core.register_bridge("_test_zbrush", "http://localhost:8765")
        ctx = dcc_mcp_core.get_bridge_context("_test_zbrush")
        assert isinstance(ctx, BridgeContext)
        assert hasattr(ctx, "connected")


# ── DccCapabilities bridge methods ──────────────────────────────────────


class TestDccCapabilitiesBridge:
    """Tests for DccCapabilities.uses_bridge / http_bridge / websocket_bridge."""

    def test_uses_bridge_false_by_default(self) -> None:
        caps = DccCapabilities()
        assert not caps.uses_bridge()

    def test_uses_bridge_true_when_bridge_kind_set(self) -> None:
        caps = DccCapabilities(bridge_kind="http")
        assert caps.uses_bridge()

    def test_http_bridge_factory(self) -> None:
        caps = DccCapabilities.http_bridge("http://localhost:8765")
        assert caps.bridge_kind == "http"
        assert caps.bridge_endpoint == "http://localhost:8765"
        assert caps.uses_bridge()
        assert not caps.has_embedded_python

    def test_websocket_bridge_factory(self) -> None:
        caps = DccCapabilities.websocket_bridge("ws://localhost:9001")
        assert caps.bridge_kind == "websocket"
        assert caps.bridge_endpoint == "ws://localhost:9001"
        assert caps.uses_bridge()
        assert not caps.has_embedded_python

    def test_bridge_dcc_no_embedded_python(self) -> None:
        """Bridge-based DCCs should not have embedded Python."""
        caps = DccCapabilities.websocket_bridge("ws://localhost:9001")
        assert not caps.has_embedded_python

    def test_embedded_python_dcc_no_bridge(self) -> None:
        """DCCs with embedded Python should not use a bridge by default."""
        caps = DccCapabilities(has_embedded_python=True)
        assert not caps.uses_bridge()
        assert caps.bridge_kind is None
