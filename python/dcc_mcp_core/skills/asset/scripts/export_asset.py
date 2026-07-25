"""export_asset — Export selection/scene as a named asset in the catalog."""
from __future__ import annotations

import json
import os
import subprocess
import time
from typing import Any


def _get_first_instance(dcc_type: str | None = None) -> str | None:
    """Get first ready instance matching dcc_type."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "list", "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        for inst in data.get("instances", []):
            if dcc_type and inst.get("dcc_type") != dcc_type:
                continue
            if inst.get("direct_control", {}).get("ready"):
                return inst.get("instance_short") or inst.get("instance_id")
        if data.get("instances"):
            return data["instances"][0].get("instance_short")
    except Exception:
        pass
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
    """Export selection/scene as a named asset.

    Args:
        asset_name: Name for the exported asset.
        asset_type: Asset type.
        format: Export format.
        selection_only: Export selection only.
        output_dir: Output directory.
        tags: Tags to attach.
        source_dcc: Source DCC type.

    Returns:
        Export result.
    """
    output_dir = output_dir or os.path.join(os.getcwd(), "exports")
    os.makedirs(output_dir, exist_ok=True)

    output_path = os.path.join(output_dir, "{}.{}".format(asset_name, format))

    instance = _get_first_instance(source_dcc)
    if not instance:
        return {"success": False, "exported": False, "error": "No ready DCC instance found."}

    # Search for export tools
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "search", "--query", "export {}".format("selection" if selection_only else "scene"),
             "--dcc-type", source_dcc or "", "--limit", "5", "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        tools = []
        if result.returncode == 0:
            data = json.loads(result.stdout)
            tools = data.get("tools", data.get("results", []))

        if tools:
            tool_slug = tools[0].get("slug", "")
            export_args: dict[str, Any] = {
                "file_path": output_path,
                "format": format,
            }
            call_result = subprocess.run(
                ["dcc-mcp-cli", "call", tool_slug, "--json", json.dumps(export_args), "--output", "json"],
                capture_output=True, text=True, timeout=60,
            )
            if call_result.returncode == 0:
                file_size = os.path.getsize(output_path) if os.path.isfile(output_path) else 0
                return {
                    "success": True,
                    "exported": True,
                    "asset_name": asset_name,
                    "file_path": output_path,
                    "format": format,
                    "file_size": file_size,
                }

        return {
            "success": False,
            "exported": False,
            "asset_name": asset_name,
            "error": "No export tool found for format '{}'.".format(format),
        }
    except Exception as e:
        return {"success": False, "exported": False, "asset_name": asset_name, "error": str(e)}
