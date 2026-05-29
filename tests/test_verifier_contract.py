"""Contract tests for the cross-DCC verifier shape (issue #688).

These tests freeze the :class:`dcc_mcp_core.SceneStats` contract that
downstream DCC repos (``dcc-mcp-blender``, ``dcc-mcp-maya``,
``dcc-mcp-unreal``, ``dcc-mcp-photoshop``) build their verifier skills
against. They are pure-Python and require no DCC binary — the verifier
*implementations* are tested in the respective downstream repositories.

Run:  pytest tests/test_verifier_contract.py -v
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core import SceneStats


class TestSceneStatsContract:
    """The SceneStats dataclass is the single source of truth for verifier output shape."""

    def test_scene_stats_roundtrip_dict(self) -> None:
        """SceneStats ↔ dict round-trip preserves every contract field."""
        original = SceneStats(
            object_count=3,
            vertex_count=482,
            has_mesh=True,
            extra={"bbox_max_z": 1.23},
        )
        restored = SceneStats.from_dict(original.to_dict())
        assert restored == original

    def test_scene_stats_matches_tolerance(self) -> None:
        """matches() is strict on object_count / has_mesh and fuzzy on vertex_count."""
        produced = SceneStats(object_count=1, vertex_count=100, has_mesh=True)
        close = SceneStats(object_count=1, vertex_count=104, has_mesh=True)  # +4 %
        drifted = SceneStats(object_count=1, vertex_count=130, has_mesh=True)  # +30 %

        assert produced.matches(close, vertex_tolerance=0.05)
        assert not produced.matches(drifted, vertex_tolerance=0.05)

    def test_scene_stats_extra_preserved(self) -> None:
        """Unknown fields in ``extra`` survive the serialisation round-trip."""
        stats = SceneStats(
            object_count=2,
            vertex_count=64,
            has_mesh=True,
            extra={"material_count": 4, "dcc_note": "blender-3.6"},
        )
        payload = stats.to_dict()
        assert payload["extra"]["material_count"] == 4
        restored = SceneStats.from_dict(payload)
        assert restored.extra == {"material_count": 4, "dcc_note": "blender-3.6"}

    def test_scene_stats_matches_rejects_has_mesh_divergence(self) -> None:
        """has_mesh mismatch fails even when vertex counts happen to coincide."""
        produced = SceneStats(object_count=1, vertex_count=0, has_mesh=True)
        empty = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        assert not produced.matches(empty)

    def test_scene_stats_matches_rejects_object_count_divergence(self) -> None:
        """object_count mismatch always fails — structural invariant."""
        produced = SceneStats(object_count=1, vertex_count=100, has_mesh=True)
        merged = SceneStats(object_count=2, vertex_count=100, has_mesh=True)
        assert not produced.matches(merged)

    def test_scene_stats_from_dict_requires_core_fields(self) -> None:
        """A payload missing any of the 3 core fields is a KeyError."""
        with pytest.raises(KeyError):
            SceneStats.from_dict({"object_count": 1, "vertex_count": 10})

    def test_scene_stats_from_dict_rejects_malformed_extra(self) -> None:
        """Extra must be a mapping — guards against list/str smuggling."""
        with pytest.raises(TypeError):
            SceneStats.from_dict(
                {
                    "object_count": 1,
                    "vertex_count": 10,
                    "has_mesh": True,
                    "extra": ["not", "a", "dict"],
                }
            )

    def test_scene_stats_matches_rejects_negative_tolerance(self) -> None:
        """vertex_tolerance<0 is a programming error, not a silent pass."""
        a = SceneStats(object_count=1, vertex_count=10, has_mesh=True)
        with pytest.raises(ValueError):
            a.matches(a, vertex_tolerance=-0.01)

    def test_scene_stats_matches_handles_zero_vertex_baseline(self) -> None:
        """Zero-vertex asset (e.g. camera-only scene) compares strictly."""
        empty_a = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        empty_b = SceneStats(object_count=1, vertex_count=0, has_mesh=False)
        different = SceneStats(object_count=1, vertex_count=5, has_mesh=False)
        assert empty_a.matches(empty_b)
        assert not empty_a.matches(different)

    def test_scene_stats_is_top_level_exported(self) -> None:
        """``dcc_mcp_core.SceneStats`` is part of the documented public API."""
        assert hasattr(dcc_mcp_core, "SceneStats")
        assert "SceneStats" in dcc_mcp_core.__all__

    def test_scene_stats_stub_payload_is_json_safe(self) -> None:
        """A zeroed SceneStats payload is JSON-safe and shape-complete."""
        zeroed = SceneStats(object_count=0, vertex_count=0, has_mesh=False).to_dict()
        decoded = json.loads(json.dumps(zeroed))
        assert set(decoded) == {"object_count", "vertex_count", "has_mesh", "extra"}
