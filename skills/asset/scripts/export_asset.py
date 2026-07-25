"""export_asset — Export DCC selection/scene as a named asset in the catalog.

Composes dcc-mcp-cli list + search + call to:
  1. Find a ready DCC instance
  2. Discover the host-specific export tool
  3. Execute the export
  4. Report the result with file metadata
"""
from __future__ import annotations

import json
import os
import subprocess
from typing import Any


# Supported export formats across common DCC hosts
SUPPORTED_FORMATS = frozenset({"fbx", "usd", "usda", "usdc", "usdz", "abc", "obj", "gltf", "glb"})


def _run_cli(*args: str, timeout: int = 15) -> dict[str, Any] | None:
    """Run dcc-mcp-cli and return parsed JSON, or None on failure."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", *args, "--output", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if result.returncode != 0:
            return None
        return json.loads(result.stdout)
    except Exception:
        return None


def _find_ready_instance(dcc_type: str | None = None) -> str | None:
    """Find the first dispatch-ready DCC instance, optionally filtered by type."""
    inventory = _run_cli("list") or {}
    instances = inventory.get("instances", [])
    if not instances:
        return None

    for inst in instances:
        if dcc_type and inst.get("dcc_type") != dcc_type:
            continue
        dc = inst.get("direct_control", {})
        if dc.get("ready"):
            return inst.get("instance_short") or inst.get("instance_id")

    for inst in instances:
        if dcc_type and inst.get("dcc_type") != dcc_type:
            continue
        return inst.get("instance_short") or inst.get("instance_id")

    return None


def _resolve_tool(query: str, dcc_type: str | None = None) -> str | None:
    """Search for an export tool matching the query and return its slug."""
    args = ["search", "--query", query, "--limit", "5"]
    if dcc_type:
        args.extend(["--dcc-type", dcc_type])
    data = _run_cli(*args, timeout=15)
    if not data:
        return None

    tools = data.get("tools", data.get("results", []))
    if not tools:
        return None

    export_keywords = ("export", "save", "write", "publish")
    for t in tools:
        slug = t.get("slug", "")
        if any(kw in slug.lower() for kw in export_keywords):
            return slug
    return None


def export_asset(
    asset_name: str,
    asset_type: str | None = None,
    format: str = "fbx",
    selection_only: bool = True,
    output_dir: str | None = None,
    tags: list[str] | None = None,
    source_dcc: str | None = None,
) -> dict[str, Any]:
    """Export selection or full scene as a named asset in the catalog.

    Args:
        asset_name: Name for the exported asset.
        asset_type: Asset type — 'model', 'material', 'texture', 'animation'.
        format: Export format — 'fbx', 'usd', 'abc', 'obj', 'gltf'.
        selection_only: Export only current selection (default True).
        output_dir: Output directory. Uses catalog default when omitted.
        tags: Tags to attach to the asset entry.
        source_dcc: Source DCC type (auto-detected when omitted).

    Returns:
        Export result with file path, format, and size.
    """
    # --- Validate format ---
    fmt_lower = format.lower()
    if fmt_lower not in SUPPORTED_FORMATS:
        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "Unsupported format '{}'. Supported: {}.".format(
                format, ", ".join(sorted(SUPPORTED_FORMATS))
            ),
        }

    # --- Output directory ---
    output_dir = output_dir or os.path.join(os.getcwd(), "exports")
    try:
        os.makedirs(output_dir, exist_ok=True)
    except OSError as e:
        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "Cannot create output directory '{}': {}.".format(output_dir, e),
        }

    output_path = os.path.join(output_dir, "{}.{}".format(asset_name, fmt_lower))

    # --- Find ready instance ---
    instance = _find_ready_instance(source_dcc)
    if not instance:
        dcc_hint = " for {}".format(source_dcc) if source_dcc else ""
        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "No dispatch-ready DCC instance found{}. Start a DCC adapter first.".format(dcc_hint),
        }

    # --- Find export tool ---
    action = "selection" if selection_only else "scene"
    queries = [
        "export {}".format(action),
        "export file",
        "save {} as".format(action),
    ]
    tool_slug = None
    for q in queries:
        tool_slug = _resolve_tool(q, source_dcc)
        if tool_slug:
            break

    if not tool_slug:
        # Fallback: try a direct export call via tool name convention
        dcc_part = source_dcc or "dcc"
        tool_slug = "{}__export_file".format(dcc_part)

    # --- Execute export ---
    export_args: dict[str, Any] = {
        "file_path": output_path,
        "format": fmt_lower,
    }
    if selection_only:
        export_args["selection_only"] = True
    if asset_type:
        export_args["asset_type"] = asset_type
    if tags:
        export_args["tags"] = tags

    try:
        data = _run_cli("call", tool_slug, "--json", json.dumps(export_args), timeout=60)
        if data:
            file_size = os.path.getsize(output_path) if os.path.isfile(output_path) else 0
            return {
                "success": True,
                "exported": True,
                "asset_name": asset_name,
                "file_path": output_path,
                "format": fmt_lower,
                "file_size": file_size,
            }

        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "Export tool '{}' returned no data.".format(tool_slug),
        }
    except subprocess.TimeoutExpired:
        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "Export timed out after 60s — scene may be too large or DCC unresponsive.",
        }
    except Exception as e:
        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": str(e),
        }
