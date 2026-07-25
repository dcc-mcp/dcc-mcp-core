"""Export selected nodes from a DCC host scene to a file.

Discovers the host export tool at runtime via gateway tool search and
delegates the actual export. After export, registers the result in the
asset catalog via the asset-source skill.

Export flow:
1. Discover host export tools (by dcc name or auto-detect)
2. Call the discovered export tool with node list + output path
3. On success, build an AssetDescriptor from the output
4. Register in catalog (if gateway is available)
5. Return the descriptor
"""

from __future__ import annotations

import os as _os
import sys as _sys
from pathlib import Path as _Path

from dcc_mcp_core.skill import skill_entry, skill_error, skill_success
from dcc_mcp_core.asset_import import (
    AssetDescriptor,
    AssetFileVariant,
    AssetFormat,
    AssetAttribution,
)


# ---------------------------------------------------------------------------
# Format helpers
# ---------------------------------------------------------------------------

_FORMAT_EXTENSIONS = {
    "fbx": AssetFormat.FBX,
    "obj": AssetFormat.OBJ,
    "usd": AssetFormat.USD,
    "usdz": AssetFormat.USDZ,
    "gltf": AssetFormat.GLTF,
    "glb": AssetFormat.GLB,
    "abc": AssetFormat.ABC,
    "blend": AssetFormat.BLEND,
}

_FORMAT_MIMES = {
    "fbx": "model/fbx",
    "obj": "model/obj",
    "usd": "model/vnd.usd+zip",
    "usdz": "model/vnd.usdz+zip",
    "gltf": "model/gltf+json",
    "glb": "model/gltf-binary",
    "abc": "application/x-alembic",
    "blend": "application/x-blender",
}


def _infer_format(output_path, explicit_format=None):
    """Infer format from file extension, with explicit override."""
    if explicit_format:
        return explicit_format.lower()
    ext = output_path.rsplit(".", 1)[-1].lower() if "." in output_path else ""
    return _FORMAT_EXTENSIONS.get(ext, AssetFormat.UNKNOWN)


def _infer_mime(fmt):
    """Return MIME type for a format string."""
    return _FORMAT_MIMES.get(fmt, "application/octet-stream")


def _generate_asset_id(output_path, explicit_id=None):
    """Generate a catalog asset_id from the output path."""
    if explicit_id:
        return explicit_id
    path = _Path(output_path)
    name = path.stem
    # Use the parent directory's last component as category prefix
    category = path.parent.name
    if category and category not in (".", "..", ""):
        return "exports/%s/%s" % (category, name)
    return "exports/%s" % name


# ---------------------------------------------------------------------------
# Tool discovery helpers
# ---------------------------------------------------------------------------


def _find_export_tools(dcc=None):
    """Search gateway for available export tools."""
    export_tools = []
    try:
        from dcc_mcp_core.skills_helper import search_tools as _search

        raw = _search("export from scene")
        if not isinstance(raw, list):
            return export_tools

        for entry in raw:
            if not isinstance(entry, dict):
                continue
            tool_slug = entry.get("tool_slug") or entry.get("name", "")
            if "export" not in tool_slug.lower():
                continue
            if dcc and dcc.lower() not in tool_slug.lower():
                continue
            export_tools.append({
                "tool_slug": tool_slug,
                "dcc_type": entry.get("dcc_type", "unknown"),
                "display_name": entry.get("display_name", tool_slug),
            })
    except (ImportError, Exception):
        pass
    return export_tools


def _find_export_tools_fallback(dcc=None):
    """Fallback: list known export tool naming conventions."""
    known = {
        "maya": ["maya_geometry__export_selected", "maya_pipeline__export_selected"],
        "blender": ["blender_export__export_selected"],
        "houdini": ["houdini_geometry__export_selected"],
        "unreal": ["unreal_pipeline__export_selected"],
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


def _call_export_tool(tool_slug, params):
    """Call a discovered export tool."""
    try:
        from dcc_mcp_core.skills_helper import call_tool as _call
        return _call(tool_slug, params)
    except (ImportError, Exception):
        pass

    # Subprocess fallback
    import subprocess
    import json

    try:
        creationflags = 0
        if _sys.platform == "win32":
            creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)

        cmd = [
            _sys.executable,
            "-c",
            "import json, sys; "
            "from dcc_mcp_core.skills_helper import call_tool; "
            "result = call_tool(%s, %s); "
            "print(json.dumps(result))"
            % (json.dumps(tool_slug), json.dumps(params)),
        ]
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=300,
            creationflags=creationflags,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            return json.loads(proc.stdout)
        return {
            "success": False,
            "message": "Export tool call failed: %s" % (proc.stderr or "unknown error"),
        }
    except Exception as exc:
        return {"success": False, "message": "Export tool error: %s" % str(exc)}


# ---------------------------------------------------------------------------
# Catalog registration
# ---------------------------------------------------------------------------


def _register_asset(descriptor_dict):
    """Register an exported asset in the catalog via asset-source."""
    try:
        from dcc_mcp_core.skills_helper import call_tool as _call
        from dcc_mcp_core.skills_helper import search_tools as _search

        tools = _search("asset source register")
        for entry in (tools if isinstance(tools, list) else []):
            tool_slug = entry.get("tool_slug") or entry.get("name", "")
            if "register" in tool_slug:
                result = _call(tool_slug, {"descriptor": descriptor_dict})
                return result.get("success", False)
    except (ImportError, Exception):
        pass
    return False


# ---------------------------------------------------------------------------
# Tool entry point
# ---------------------------------------------------------------------------


@skill_entry
def main(output_path, node_names, format=None, dcc=None,
         asset_id=None, tags=None, **kwargs):
    """Export nodes from a DCC host scene to a file.

    Args:
        output_path: Target file path for the exported asset.
        node_names: List of node names or paths to export.
        format: Export format hint. Inferred from extension if omitted.
        dcc: Source DCC host. Auto-detected if omitted.
        asset_id: Catalog asset_id. Auto-generated from filename if omitted.
        tags: Optional list of tags for catalog registration.

    Returns:
        Skill result dict with the exported AssetDescriptor in context.

    """
    output_path = output_path.strip()
    if not output_path:
        return skill_error("Empty output_path", "output_path must not be empty")

    if not node_names:
        return skill_error("Empty node_names", "At least one node name is required")

    fmt = _infer_format(output_path, format)
    if fmt == AssetFormat.UNKNOWN:
        return skill_error(
            "Unknown export format",
            "Cannot determine format from '%s'" % output_path,
            prompt="Specify --format explicitly (e.g. 'fbx', 'usd', 'obj').",
            output_path=output_path,
        )

    # Discover export tools
    export_tools = _find_export_tools(dcc)
    if not export_tools:
        export_tools = _find_export_tools_fallback(dcc)

    if not export_tools:
        dcc_hint = " for '%s'" % dcc if dcc else ""
        return skill_error(
            "No export tools found%s" % dcc_hint,
            "No DCC export tools discovered",
            prompt=(
                "Load a DCC export skill first or ensure a DCC gateway "
                "instance is running."
            ),
            dcc=dcc,
            output_path=output_path,
        )

    # Build export params
    export_params = {
        "output_path": output_path,
        "node_names": node_names,
        "format": fmt,
    }

    # Try each available export tool
    results = []
    for tool_info in export_tools:
        tool_slug = tool_info["tool_slug"]
        response = _call_export_tool(tool_slug, export_params)
        results.append({
            "tool": tool_slug,
            "dcc": tool_info["dcc_type"],
            "success": response.get("success", False),
            "message": response.get("message", ""),
            "context": response.get("context", {}),
        })

    succeeded = [r for r in results if r["success"]]

    if not succeeded:
        return skill_error(
            "Export failed: tried %d tool(s)" % len(results),
            "All export attempts failed",
            prompt="Check DCC instances are running and export skills are loaded.",
            output_path=output_path,
            nodes=node_names,
            attempts=results,
        )

    first = succeeded[0]
    generated_id = _generate_asset_id(output_path, asset_id)
    mime = _infer_mime(fmt)

    # Build the exported asset descriptor
    variant = AssetFileVariant(
        local_path=output_path,
        format=fmt,
        preferred=True,
        mime=mime,
    )

    # Check file size
    file_size = None
    try:
        file_size = _Path(output_path).stat().st_size
    except OSError:
        pass

    tags_list = list(tags) if tags else []

    desc = AssetDescriptor(
        asset_id=generated_id,
        variants=[variant],
        unit_hint="unitless",
        meters_per_unit=1.0,
        up_axis="y",
        tags=tags_list,
        extra={
            "exported_from": first["dcc"],
            "export_tool": first["tool"],
            "node_count": len(node_names),
        },
    )
    desc_dict = desc.to_dict()

    # Attempt catalog registration
    registered = _register_asset(desc_dict)

    return skill_success(
        "Exported %d node(s) to '%s' via %s" % (
            len(node_names), output_path, first["dcc"],
        ),
        prompt="Pass the descriptor to import_asset to re-import later.",
        descriptor=desc_dict,
        asset_id=generated_id,
        output_path=output_path,
        file_size=file_size,
        format=fmt,
        dcc=first["dcc"],
        node_count=len(node_names),
        registered=registered,
        all_results=results,
    )


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
