"""Tests for adapter context/resources/policy helpers (#608-#613)."""

from __future__ import annotations

import json
from typing import Any

import dcc_mcp_core
from dcc_mcp_core import AdapterInstructionSet
from dcc_mcp_core import DccApiDocEntry
from dcc_mcp_core import DccApiDocIndex
from dcc_mcp_core import DccContextSnapshot
from dcc_mcp_core import DccToolsetProfile
from dcc_mcp_core import ResponseShapePolicy
from dcc_mcp_core import ToolsetProfileRegistry
from dcc_mcp_core import VisualFeedbackPolicy
from dcc_mcp_core import append_context_snapshot
from dcc_mcp_core import build_visual_feedback_context
from dcc_mcp_core import register_adapter_instruction_resources
from dcc_mcp_core import register_dcc_api_docs
from dcc_mcp_core import shape_response
from dcc_mcp_core.server_base import DccServerBase


class _FakeServer:
    def __init__(self) -> None:
        self.resources: dict[str, dict[str, Any]] = {}

    def add_docs_resource(self, **kwargs: Any) -> None:
        self.resources[kwargs["uri"]] = kwargs


def test_adapter_context_symbols_exported() -> None:
    for name in (
        "AdapterInstructionSet",
        "DccContextSnapshot",
        "VisualFeedbackPolicy",
        "ResponseShapePolicy",
        "DccToolsetProfile",
        "ToolsetProfileRegistry",
        "DccApiDocEntry",
        "DccApiDocIndex",
        "register_adapter_instruction_resources",
        "register_dcc_api_docs",
    ):
        assert hasattr(dcc_mcp_core, name)
        assert name in dcc_mcp_core.__all__


def test_register_adapter_instruction_resources() -> None:
    server = _FakeServer()
    uris = register_adapter_instruction_resources(
        server,
        AdapterInstructionSet(
            dcc="maya",
            instructions="Use screenshots after visual changes.",
            capabilities={"screenshots": True},
            troubleshooting="Check the Script Editor.",
            adapter_version="1.2.3",
        ),
    )

    assert uris == [
        "docs://adapter/maya/instructions",
        "docs://adapter/maya/capabilities",
        "docs://adapter/maya/troubleshooting",
    ]
    assert server.resources["docs://adapter/maya/instructions"]["content"].startswith("Use screenshots")
    capabilities = json.loads(server.resources["docs://adapter/maya/capabilities"]["content"])
    assert capabilities["adapter_version"] == "1.2.3"
    assert capabilities["capabilities"]["screenshots"] is True


def test_append_context_snapshot_shapes_snapshot() -> None:
    result = {"success": True, "message": "ok"}
    snapshot = DccContextSnapshot(
        dcc="photoshop",
        document={"name": "hero.psd"},
        selection={"kind": "layer"},
        counts={"layers": 250},
    )

    enriched = append_context_snapshot(result, snapshot, policy=ResponseShapePolicy(max_items=3))

    assert enriched["context"]["snapshot"]["dcc"] == "photoshop"
    assert enriched["context"]["snapshot"]["document"]["name"] == "hero.psd"
    assert result.get("context") is None


def test_visual_feedback_context_clamps_dimensions() -> None:
    payload = build_visual_feedback_context(
        resource="output://preview.png",
        width=1200,
        height=600,
        policy=VisualFeedbackPolicy(mode="after_mutation", max_size=800, format="png"),
    )

    feedback = payload["visual_feedback"]
    assert feedback["resource"] == "output://preview.png"
    assert feedback["width"] == 800
    assert feedback["height"] == 600
    assert feedback["mode"] == "after_mutation"


def test_shape_response_truncates_lists_with_metadata() -> None:
    shaped = shape_response({"objects": list(range(5))}, ResponseShapePolicy(max_items=3))

    assert shaped["truncated"] is True
    assert shaped["data"]["objects"] == [0, 1, 2]
    assert shaped["_meta"]["dcc.response_shape"]["omitted"][0]["omitted_items"] == 2


def test_toolset_profile_registry_tracks_active_profiles() -> None:
    registry = ToolsetProfileRegistry(
        [
            DccToolsetProfile("modeling-basic", tools=("create_cube",), default=True),
            DccToolsetProfile("rendering", tools=("render",)),
        ]
    )

    assert [profile.name for profile in registry.active_profiles()] == ["modeling-basic"]
    registry.activate("rendering")
    names = [profile["name"] for profile in registry.list_profiles() if profile["active"]]
    assert names == ["modeling-basic", "rendering"]
    registry.deactivate("modeling-basic")
    assert [profile.name for profile in registry.active_profiles()] == ["rendering"]


def test_api_docs_index_search_and_resource_registration() -> None:
    server = _FakeServer()
    index = DccApiDocIndex(
        "blender",
        [
            DccApiDocEntry("bpy.ops.mesh.primitive_cube_add", "Add a cube", tags=("mesh",)),
            DccApiDocEntry("bpy.ops.wm.quit_blender", "Quit Blender", tags=("dangerous",)),
        ],
        version="4.0",
    )

    results = index.search("cube")
    assert results[0]["symbol"] == "bpy.ops.mesh.primitive_cube_add"
    uris = register_dcc_api_docs(server, index)
    assert "docs://adapter/blender/api/index" in uris
    assert "docs://adapter/blender/api/bpy.ops.mesh.primitive_cube_add" in uris


def test_dcc_server_base_snapshot_and_instruction_wrappers() -> None:
    server = object.__new__(DccServerBase)
    fake = _FakeServer()
    server._server = fake
    server._snapshot_provider = lambda: DccContextSnapshot(dcc="maya", counts={"objects": 2})

    enriched = server.append_context_snapshot({"success": True})
    assert enriched["context"]["snapshot"]["counts"]["objects"] == 2

    uris = server.register_adapter_instructions(AdapterInstructionSet(dcc="maya", instructions="Use tools/list first."))
    assert uris == ["docs://adapter/maya/instructions", "docs://adapter/maya/capabilities"]
