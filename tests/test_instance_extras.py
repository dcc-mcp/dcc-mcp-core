"""Tests for the ServiceEntry.extras Python write path (issue #2500).

`extras` is the type-preserving counterpart of `instance_metadata`: it lets an
adapter publish adapter-specific values that are not strings — for example the
WebView / bridge fields `cdp_port` (int), `url`, `window_title` and `host_dcc`.

These tests cover the configuration surface end to end. The heartbeat that
copies extras onto the FileRegistry row is exercised by the Rust tests in
`dcc-mcp-transport` and `dcc-mcp-gateway`.
"""

from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig


class TestInstanceExtrasConfigSurface:
    """`McpHttpConfig.instance_extras` seeds the registry row at registration."""

    def test_defaults_to_empty_mapping(self) -> None:
        assert McpHttpConfig(port=0).instance_extras == {}

    def test_round_trips_json_scalar_types(self) -> None:
        config = McpHttpConfig(port=0)
        config.instance_extras = {
            "url": "http://localhost:3000",
            "cdp_port": 9222,
            "window_title": "My Tool",
            "host_dcc": "maya-2024",
        }

        extras = config.instance_extras
        assert extras["url"] == "http://localhost:3000"
        # The whole point of `extras`: an int stays an int instead of being
        # coerced to a string the way `instance_metadata` would.
        assert extras["cdp_port"] == 9222
        assert isinstance(extras["cdp_port"], int)
        assert extras["window_title"] == "My Tool"
        assert extras["host_dcc"] == "maya-2024"

    def test_round_trips_nested_containers(self) -> None:
        config = McpHttpConfig(port=0)
        config.instance_extras = {
            "enabled": True,
            "viewport": {"width": 1920, "scale": 1.5},
            "layers": ["base", "over"],
        }

        extras = config.instance_extras
        assert extras["enabled"] is True
        assert extras["viewport"] == {"width": 1920, "scale": 1.5}
        assert extras["layers"] == ["base", "over"]

    def test_replaces_rather_than_merges(self) -> None:
        """The property is a replace; the handle API is the merge surface."""
        config = McpHttpConfig(port=0)
        config.instance_extras = {"a": 1, "b": 2}
        config.instance_extras = {"b": 3}

        assert config.instance_extras == {"b": 3}

    def test_nulls_are_tombstones_and_hidden_on_read(self) -> None:
        """A None value marks a key for removal and is not a stored value."""
        config = McpHttpConfig(port=0)
        config.instance_extras = {"keep": 1, "drop_me": None}

        assert config.instance_extras == {"keep": 1}

    def test_rejects_non_mapping(self) -> None:
        config = McpHttpConfig(port=0)
        with pytest.raises(TypeError):
            config.instance_extras = "not-a-mapping"  # type: ignore[assignment]
