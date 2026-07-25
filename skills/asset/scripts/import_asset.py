"""import_asset — Import an AssetDescriptor into the active DCC scene.

Composes dcc-mcp-cli list + search + call to orchestrate a cross-DCC import:
  1. Validate the AssetDescriptor and select the target variant
  2. Discover a ready DCC instance
  3. Find the host-specific import tool
  4. Execute the import and return scene objects
"""
from __future__ import annotations

import json
import os
import subprocess
import time
from typing import Any


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

    # Fallback: first instance of matching type
    for inst in instances:
        if dcc_type and inst.get("dcc_type") != dcc_type:
            continue
        return inst.get("instance_short") or inst.get("instance_id")

    return None


def _resolve_tool(query: str, dcc_type: str | None = None) -> str | None:
    """Search for a tool matching the query and return its slug."""
    args = ["search", "--query", query, "--limit", "5"]
    if dcc_type:
        args.extend(["--dcc-type", dcc_type])
    data = _run_cli(*args, timeout=15)
    if not data:
        return None

    tools = data.get("tools", data.get("results", []))
    if not tools:
        return None

    slug = tools[0].get("slug", "")
    import_keywords = ("import", "load", "open", "file")
    if not any(kw in slug.lower() for kw in import_keywords):
        # Try the next tool
        for t in tools[1:]:
            s = t.get("slug", "")
            if any(kw in s.lower() for kw in import_keywords):
                return s
        return None
    return slug


def import_asset(
    descriptor: dict[str, Any],
    target_dcc: str | None = None,
    variant_index: int = 0,
    import_options: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Import a resolved AssetDescriptor into the active DCC scene.

    Args:
        descriptor: AssetDescriptor from resolve_asset.
        target_dcc: Target DCC type (auto-detected when omitted).
        variant_index: Which variant to import (0 = first, default).
        import_options: DCC-specific import options.

    Returns:
        Import result with scene objects and timing.
    """
    asset_name = descriptor.get("name", "unknown")
    variants = descriptor.get("variants", [])
    asset_type = descriptor.get("asset_type", "")

    # --- Validate descriptor ---
    if not variants:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": "No variants in AssetDescriptor. Run resolve_asset first to get file variants.",
        }

    if variant_index >= len(variants):
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": "variant_index {} out of range ({} variants available).".format(
                variant_index, len(variants)
            ),
        }

    variant = variants[variant_index]
    file_path = variant.get("path", "")
    asset_format = variant.get("format", "unknown")

    if not file_path:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": "Variant at index {} has no path.".format(variant_index),
        }

    if not os.path.exists(file_path):
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "file_path": file_path,
            "error": "Asset file not found: {}".format(file_path),
        }

    # --- Find ready instance ---
    instance = _find_ready_instance(target_dcc)
    if not instance:
        dcc_hint = " for {}".format(target_dcc) if target_dcc else ""
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": "No dispatch-ready DCC instance found{}. Start a DCC adapter first.".format(dcc_hint),
        }

    # --- Find import tool ---
    queries = [
        "import {}".format(asset_type) if asset_type else "import file",
        "import file",
        "import scene",
        "load file",
    ]
    tool_slug = None
    for q in queries:
        tool_slug = _resolve_tool(q, target_dcc)
        if tool_slug:
            break

    if not tool_slug:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "file_path": file_path,
            "format": asset_format,
            "error": "No import tool found{} — check adapter install.".format(
                " for {}".format(target_dcc) if target_dcc else ""
            ),
        }

    # --- Execute import ---
    call_args: dict[str, Any] = {"file_path": file_path}
    if import_options:
        call_args.update(import_options)

    start_time = time.time()
    try:
        import_data = _run_cli("call", tool_slug, "--json", json.dumps(call_args), timeout=60)
        duration_ms = (time.time() - start_time) * 1000

        if import_data:
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
            "error": "Import tool '{}' returned no data.".format(tool_slug),
        }
    except subprocess.TimeoutExpired:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "error": "Import timed out after 60s — file may be too large or DCC unresponsive.",
        }
    except Exception as e:
        return {
            "success": False,
            "imported": False,
            "asset_name": asset_name,
            "file_path": file_path,
            "error": str(e),
        }
