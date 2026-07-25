"""Import a resolved AssetDescriptor into a DCC host scene.

Discovers the appropriate DCC-specific import tool at runtime via gateway
tool search and delegates the actual import. The descriptor is expected
to be a full AssetDescriptor dict from resolve_asset or asset-source.

Import flow:
1. Validate the descriptor
2. Discover host import tools (by dcc name or auto-detect)
3. Build ImportToSceneRequest
4. Call the discovered import tool
5. Return ImportToSceneResult
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_error, skill_success, skill_warning
from dcc_mcp_core.asset_import import (
    AssetDescriptor,
    ImportToSceneRequest,
    MaterialMode,
    PlacementHint,
)


# ---------------------------------------------------------------------------
# Tool discovery helpers
# ---------------------------------------------------------------------------


def _find_import_tools(dcc=None):
    """Search gateway for available import-to-scene tools.

    Returns a list of dicts with tool_slug, dcc_type, and display_name.
    """
    import_tools = []
    try:
        from dcc_mcp_core.skills_helper import search_tools as _search

        raw = _search("import to scene")
        if not isinstance(raw, list):
            return import_tools

        for entry in raw:
            if not isinstance(entry, dict):
                continue
            tool_slug = entry.get("tool_slug") or entry.get("name", "")
            if "import" not in tool_slug.lower():
                continue
            if dcc and dcc.lower() not in tool_slug.lower():
                continue
            import_tools.append({
                "tool_slug": tool_slug,
                "dcc_type": entry.get("dcc_type", "unknown"),
                "display_name": entry.get("display_name", tool_slug),
            })
    except (ImportError, Exception):
        pass
    return import_tools


def _find_import_tools_fallback(dcc=None):
    """Fallback: list known import tool naming conventions."""
    known = {
        "maya": ["maya_geometry__import_to_scene", "maya_pipeline__import_to_scene"],
        "blender": ["blender_import__import_to_scene"],
        "houdini": ["houdini_geometry__import_to_scene"],
        "unreal": ["unreal_pipeline__import_to_scene"],
    }

    result = []
    targets = [dcc.lower()] if dcc else known.keys()
    for target in targets:
        for tool_slug in known.get(target, []):
            result.append({
                "tool_slug": tool_slug,
                "dcc_type": target,
                "display_name": tool_slug,
            })
    return result


def _call_import_tool(tool_slug, request_dict):
    """Call a discovered import tool with the import request."""
    try:
        from dcc_mcp_core.skills_helper import call_tool as _call
        return _call(tool_slug, request_dict)
    except (ImportError, Exception):
        pass

    # Direct subprocess fallback
    import subprocess
    import json
    import sys
    import os

    try:
        creationflags = 0
        if sys.platform == "win32":
            creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)

        cmd = [
            sys.executable,
            "-c",
            "import json, sys; "
            "from dcc_mcp_core.skills_helper import call_tool; "
            "result = call_tool(%s, %s); "
            "print(json.dumps(result))"
            % (json.dumps(tool_slug), json.dumps(request_dict)),
        ]
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=120,
            creationflags=creationflags,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            return json.loads(proc.stdout)
        return {
            "success": False,
            "message": "Import tool call failed: %s" % (proc.stderr or "unknown error"),
        }
    except Exception as exc:
        return {"success": False, "message": "Import tool error: %s" % str(exc)}


# ---------------------------------------------------------------------------
# Tool entry point
# ---------------------------------------------------------------------------


@skill_entry
def main(descriptor, dcc=None, material_mode="as_authored",
         target_collection=None, skip_existing=False, placement=None, **kwargs):
    """Import a resolved asset descriptor into a DCC host scene.

    Args:
        descriptor: AssetDescriptor dict from resolve_asset or asset-source.
        dcc: Target DCC host name. Auto-detected if omitted.
        material_mode: Material import strategy (as_authored, default_gray, skip).
        target_collection: Optional collection/layer name.
        skip_existing: Skip import if asset_id already present.
        placement: Optional placement hint dict with translate/rotate/scale/parent_name.

    Returns:
        Skill result dict with ImportToSceneResult in context.

    """
    # Validate descriptor
    try:
        asset_desc = AssetDescriptor.from_dict(descriptor)
        asset_desc.validate()
    except Exception as exc:
        return skill_error(
            "Invalid asset descriptor",
            str(exc),
            prompt="Pass a valid descriptor from resolve_asset or asset-source.",
        )

    # Discover import tools
    import_tools = _find_import_tools(dcc)
    if not import_tools:
        import_tools = _find_import_tools_fallback(dcc)

    if not import_tools:
        dcc_hint = " for '%s'" % dcc if dcc else ""
        return skill_error(
            "No import tools found%s" % dcc_hint,
            "No DCC import-to-scene tools discovered",
            prompt=(
                "Load a DCC import skill first (e.g. blender-import-to-scene, "
                "maya-geometry) or ensure a DCC gateway instance is running."
            ),
            dcc=dcc,
            descriptor_id=asset_desc.asset_id,
        )

    # Build import request
    placement_hint = PlacementHint.from_dict(placement) if placement else None
    request = ImportToSceneRequest(
        descriptor=asset_desc,
        material_mode=material_mode,
        placement=placement_hint,
        target_collection=target_collection,
        skip_existing=skip_existing,
    )
    request_dict = request.to_dict()

    # Try each available import tool
    results = []
    for tool_info in import_tools:
        tool_slug = tool_info["tool_slug"]
        response = _call_import_tool(tool_slug, request_dict)
        results.append({
            "tool": tool_slug,
            "dcc": tool_info["dcc_type"],
            "success": response.get("success", False),
            "message": response.get("message", ""),
            "context": response.get("context", {}),
        })

    succeeded = [r for r in results if r["success"]]
    failed = [r for r in results if not r["success"]]

    if succeeded:
        first = succeeded[0]
        imported_nodes = first.get("context", {}).get("imported_nodes", [])
        warnings = first.get("context", {}).get("warnings", [])

        msg = "Imported '%s' into %s: %d node(s)" % (
            asset_desc.asset_id,
            first["dcc"],
            len(imported_nodes),
        )

        warn_count = len(warnings)
        if warn_count > 0:
            return skill_warning(
                msg,
                warning="%d non-fatal warning(s) during import" % warn_count,
                prompt="Check imported nodes in the viewport.",
                imported_nodes=imported_nodes,
                warnings=warnings,
                tool=first["tool"],
                dcc=first["dcc"],
                all_results=results,
            )

        return skill_success(
            msg,
            prompt="Check imported nodes in the viewport.",
            imported_nodes=imported_nodes,
            warnings=warnings,
            tool=first["tool"],
            dcc=first["dcc"],
            all_results=results,
        )

    # All tools failed
    return skill_error(
        "Import failed for '%s': tried %d tool(s)" % (asset_desc.asset_id, len(results)),
        "All import attempts failed",
        prompt="Check DCC instances are running and import skills are loaded.",
        asset_id=asset_desc.asset_id,
        attempts=results,
    )


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
