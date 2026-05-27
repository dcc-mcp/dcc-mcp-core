"""Tests for the capability graph (issue #1336)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import CapabilityEdge
from dcc_mcp_core import CapabilityGraph
from dcc_mcp_core import EdgeKind


class TestEdgeKind:
    def test_parse_accepts_enum_and_snake_kebab(self) -> None:
        assert EdgeKind.parse(EdgeKind.REQUIRES) is EdgeKind.REQUIRES
        assert EdgeKind.parse("requires") is EdgeKind.REQUIRES
        assert EdgeKind.parse("DEPENDS_ON") is EdgeKind.DEPENDS_ON
        assert EdgeKind.parse("fallback-for") is EdgeKind.FALLBACK_FOR

    def test_parse_rejects_unknown(self) -> None:
        with pytest.raises(ValueError):
            EdgeKind.parse("loves")


class TestCapabilityEdge:
    def test_rejects_self_loop(self) -> None:
        with pytest.raises(ValueError):
            CapabilityEdge("a", "a", EdgeKind.REQUIRES)

    def test_rejects_empty_endpoints(self) -> None:
        with pytest.raises(ValueError):
            CapabilityEdge("", "b", EdgeKind.REQUIRES)

    def test_rejects_out_of_range_weight(self) -> None:
        with pytest.raises(ValueError):
            CapabilityEdge("a", "b", EdgeKind.REQUIRES, weight=2.0)


class TestCapabilityGraph:
    def test_add_edge_is_idempotent(self) -> None:
        g = CapabilityGraph()
        e = CapabilityEdge("usd_import", "scene_node", EdgeKind.PRODUCES)
        assert g.add_edge(e) is True
        assert g.add_edge(e) is False
        assert g.edge_count() == 1

    def test_register_skill_returns_added_count(self) -> None:
        g = CapabilityGraph()
        added = g.register_skill(
            "usd_import",
            requires=["scene_open"],
            produces=["scene_node:transform", "file:usd"],
            fallback_for=["fbx_import"],
        )
        assert added == 4
        # Repeat is idempotent
        assert g.register_skill("usd_import", requires=["scene_open"]) == 0

    def test_neighbors_filters_by_kind(self) -> None:
        g = CapabilityGraph()
        g.register_skill("usd_import", requires=["scene_open"], produces=["scene_node"])
        produces = g.neighbors("usd_import", kinds=[EdgeKind.PRODUCES])
        assert {e.target for e in produces} == {"scene_node"}

    def test_neighbors_unknown_node_returns_empty(self) -> None:
        g = CapabilityGraph()
        assert g.neighbors("ghost") == ()

    def test_neighbors_invalid_direction_raises(self) -> None:
        g = CapabilityGraph()
        g.add_node("a")
        with pytest.raises(ValueError):
            g.neighbors("a", direction="sideways")

    def test_expand_is_depth_bounded(self) -> None:
        g = CapabilityGraph()
        # chain a -> b -> c -> d
        g.add_edge(CapabilityEdge("a", "b", EdgeKind.PRODUCES))
        g.add_edge(CapabilityEdge("b", "c", EdgeKind.PRODUCES))
        g.add_edge(CapabilityEdge("c", "d", EdgeKind.PRODUCES))

        depth1 = g.expand(["a"], max_depth=1)
        depth2 = g.expand(["a"], max_depth=2)
        assert depth1 == ("b",)
        assert depth2 == ("b", "c")

    def test_expand_zero_depth_returns_empty(self) -> None:
        g = CapabilityGraph()
        g.add_edge(CapabilityEdge("a", "b", EdgeKind.PRODUCES))
        assert g.expand(["a"], max_depth=0) == ()

    def test_expand_filters_by_kind(self) -> None:
        g = CapabilityGraph()
        g.add_edge(CapabilityEdge("a", "b", EdgeKind.PRODUCES))
        g.add_edge(CapabilityEdge("a", "c", EdgeKind.REQUIRES))
        # Expanding only along PRODUCES does not visit c via REQUIRES
        assert g.expand(["a"], kinds=[EdgeKind.PRODUCES], max_depth=2) == ("b",)

    def test_expand_reverse_direction(self) -> None:
        g = CapabilityGraph()
        # b PRODUCES a -> expanding from a "inward" reaches b
        g.add_edge(CapabilityEdge("b", "a", EdgeKind.PRODUCES))
        assert g.expand(["a"], direction="in", max_depth=1) == ("b",)

    def test_json_round_trip_preserves_edges(self) -> None:
        g = CapabilityGraph()
        g.register_skill(
            "usd_import",
            requires=["scene_open"],
            produces=["scene_node"],
            fallback_for=["fbx_import"],
        )
        payload = g.to_json()
        back = CapabilityGraph.from_json(payload)
        assert back.edge_count() == g.edge_count()
        assert set(back.nodes()) == set(g.nodes())

    def test_from_json_normalises_edge_kinds_in_kebab_case(self) -> None:
        payload = {
            "nodes": ["a", "b"],
            "edges": [{"source": "a", "target": "b", "kind": "fallback-for"}],
        }
        g = CapabilityGraph.from_json(payload)
        edges = g.neighbors("a")
        assert len(edges) == 1
        assert edges[0].kind is EdgeKind.FALLBACK_FOR

    def test_len_and_edge_count(self) -> None:
        g = CapabilityGraph()
        assert len(g) == 0
        g.add_edge(CapabilityEdge("a", "b", EdgeKind.PRODUCES))
        assert len(g) == 2
        assert g.edge_count() == 1
