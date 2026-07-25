"""resolve_asset — Resolve asset name to full AssetDescriptor with variants."""
from __future__ import annotations

import json
import os
import subprocess
from typing import Any


def resolve_asset(
    asset_name: str,
    format: str | None = None,
    lod: str | None = None,
) -> dict[str, Any]:
    """Resolve an asset name to a full AssetDescriptor.

    Args:
        asset_name: Exact asset name from search_assets.
        format: Preferred format (e.g. 'fbx', 'usd').
        lod: Level of detail: proxy, low, medium, high.

    Returns:
        Full AssetDescriptor ready for import.
    """
    # First search to find the asset
    search_args: dict[str, Any] = {"query": asset_name, "limit": 5}
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "asset_source__search_assets",
             "--json", json.dumps(search_args), "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode != 0:
            return {"success": False, "resolved": False, "error": "Search failed: {}".format(result.stderr)}

        data = json.loads(result.stdout)
        results = data.get("results", data.get("assets", []))
        if not results:
            return {"success": True, "resolved": False, "error": "Asset '{}' not found.".format(asset_name)}

        # Find exact match
        descriptor = None
        for r in results:
            if r.get("name") == asset_name:
                descriptor = r
                break
        if not descriptor:
            descriptor = results[0]  # Fallback to first result

        # Filter variants by format/LOD preference
        variants = descriptor.get("variants", [])
        if variants:
            if format:
                variants = [v for v in variants if v.get("format") == format] + \
                          [v for v in variants if v.get("format") != format]
            if lod:
                variants = [v for v in variants if v.get("lod") == lod] + \
                          [v for v in variants if v.get("lod") != lod]

        resolved = {
            "name": descriptor.get("name", asset_name),
            "asset_type": descriptor.get("asset_type", "unknown"),
            "display_name": descriptor.get("display_name", asset_name),
            "variants": variants,
            "attribution": descriptor.get("attribution", {}),
            "metadata": descriptor.get("metadata", {}),
        }

        return {
            "success": True,
            "resolved": bool(variants),
            "descriptor": resolved,
        }
    except Exception as e:
        return {"success": False, "resolved": False, "error": str(e)}
