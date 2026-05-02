"""Verifier skill stub — import an asset and return SceneStats.

This is a *template* script. It exists so the surrounding SKILL.md can
be loaded by ``parse_skill_md`` during core contract tests without a
real DCC dependency. Downstream DCC repos (``dcc-mcp-blender``,
``dcc-mcp-maya``, ``dcc-mcp-unreal``, ...) copy this file and replace
the body with their native import + inspection logic.

The output MUST match the :class:`dcc_mcp_core.SceneStats` contract:

    {
        "object_count": int,
        "vertex_count": int,
        "has_mesh": bool,
        "extra": dict,  # optional DCC-specific enrichments
    }
"""

from __future__ import annotations

from dcc_mcp_core import SceneStats
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def main(params: dict) -> dict:
    """Import ``file_path`` into the host DCC and return SceneStats."""
    file_path = params.get("file_path")
    if not file_path:
        return skill_error("Missing required parameter: file_path")

    # ── Replace this block with your DCC's native import + inspection ──
    # Example for Blender:
    #   import bpy
    #   bpy.ops.wm.read_factory_settings(use_empty=True)
    #   bpy.ops.import_scene.fbx(filepath=file_path)
    #   meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    #   vertex_count = sum(len(m.data.vertices) for m in meshes)
    #   stats = SceneStats(
    #       object_count=len(bpy.context.scene.objects),
    #       vertex_count=vertex_count,
    #       has_mesh=bool(meshes),
    #   )
    #
    # Example for Maya:
    #   import maya.cmds as cmds
    #   before = set(cmds.ls(assemblies=True) or [])
    #   cmds.file(file_path, i=True, type="FBX")
    #   imported = set(cmds.ls(assemblies=True) or []) - before
    #   meshes = cmds.ls(type="mesh") or []
    #   stats = SceneStats(
    #       object_count=len(imported),
    #       vertex_count=sum(cmds.polyEvaluate(m, v=True) for m in meshes),
    #       has_mesh=bool(meshes),
    #   )
    # ────────────────────────────────────────────────────────────────

    stats = SceneStats(object_count=0, vertex_count=0, has_mesh=False)
    payload = stats.to_dict()
    payload["note"] = "verifier-harness template stub — replace before use"

    return skill_success(
        f"Template stub inspected: {file_path}",
        **payload,
    )


if __name__ == "__main__":
    skill_entry(main)
