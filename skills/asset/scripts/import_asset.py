"""import_asset — Import an AssetDescriptor into the active DCC scene."""
from __future__ import annotations

import json
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


def _search_import_tool(instance: str, dcc_type: str, asset_type: str) -> str | None:
    """Find the best import tool for this asset type."""
    queries = [
        "import {}".format(asset_type),
        "import file",
        "import scene",
    ]
    for q in queries:
        try:
            result = subprocess.run(
                ["dcc-mcp-cli", "search", "--query", q, "--dcc-type", dcc_type, "--limit", "5", "--output", "json"],
                capture_output=True, text=True, timeout=10,
            )
            if result.returncode == 0:
                data = json.loads(result.stdout)
                tools = data.get("tools", data.get("results", []))
                if tools:
                    slug = tools[0].get("slug", "")
                    # Verify tool looks like an import tool
                    if any(kw in slug.lower() for kw in ["import", "load", "open"]):
                        return slug.replace("." + instance + ".", ".{}.".format(instance))
        except Exception:
            continue
    return None


def import_asset(
    descriptor: dict[str, Any],
    target_dcc: str | None = None,
    variant_index: int = 0,
    import_options: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Import an asset into a DCC scene.

    Args:
        descriptor: AssetDescriptor from resolve_asset.
        target_dcc: Target DCC type.
        variant_index: Variant to import.
        import_options: DCC-specific options.

    Returns:
        Import result.
    """
    asset_name = descriptor.get("name", "unknown")
    variants = descriptor.get("variants", [])

    if not variants or variant_index >= len(variants):
        return {
            "success": False,
            "imported": False,
            "error": "No valid variant found for import (variants={}, index={})".format(len(variants), variant_index),
        }

    variant = variants[variant_index]
    file_path = variant.get("path", "")
    asset_format = variant.get("format", "unknown")

    if not file_path or not os.path.exists(file_path):
        return {
            "success": False,
            "imported": False,
            "error": "Asset file not found: {}".format(file_path),
        }

    instance = _get_first_instance(target_dcc)
    if not instance:
        return {"success": False, "imported": False, "error": "No ready DCC instance found."}

    # Try to find and call the DCC's import tool
    # Since we can't know the exact tool name, we use the workflow chain pattern
    start_time = time.time()

    # Build a generic import call — in practice this uses the DCC's tool
    call_args: dict[str, Any] = {
        "file_path": file_path,
    }
    if import_options:
        call_args.update(import_options)

    try:
        # Use a multi-step call: search for import tool → call it
        # For the demo implementation, we report the import parameters
        result = subprocess.run(
            ["dcc-mcp-cli", "search", "--query", "import file", "--dcc-type", target_dcc or "",
             "--limit", "5", "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        tools = []
        if result.returncode == 0:
            data = json.loads(result.stdout)
            tools = data.get("tools", data.get("results", []))

        if tools:
            # Found import tools — use the first one
            tool_slug = tools[0].get("slug", "")
            call_result = subprocess.run(
                ["dcc-mcp-cli", "call", tool_slug, "--json", json.dumps(call_args), "--output", "json"],
                capture_output=True, text=True, timeout=60,
            )
            if call_result.returncode == 0:
                import_data = json.loads(call_result.stdout)
                duration_ms = (time.time() - start_time) * 1000
                return {
                    "success": True,
                    "imported": True,
                    "asset_name": asset_name,
                    "file_path": file_path,
                    "format": asset_format,
                    "scene_objects": import_data.get("scene_objects", import_data.get("objects", [])),
                    "duration_ms": round(duration_ms, 1),
                }

        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "file_path": file_path,
            "format": asset_format,
            "error": "Import failed — no import tool found or call failed.",
        }
    except Exception as e:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": str(e),
        }
