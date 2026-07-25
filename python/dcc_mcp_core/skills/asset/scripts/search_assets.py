"""search_assets — Search asset catalog and return matching AssetDescriptors."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def search_assets(
    query: str,
    asset_type: str | None = None,
    tags: list[str] | None = None,
    limit: int = 20,
    catalog: str | None = None,
) -> dict[str, Any]:
    """Search the asset catalog.

    Args:
        query: Search query.
        asset_type: Filter by asset type.
        tags: Filter by tags (AND).
        limit: Max results.
        catalog: Named catalog to search.

    Returns:
        Search results with AssetDescriptor summaries.
    """
    search_args: dict[str, Any] = {"query": query, "limit": limit}
    if asset_type:
        search_args["asset_type"] = asset_type
    if tags:
        search_args["tags"] = tags

    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "asset_source__search_assets",
             "--json", json.dumps(search_args), "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            return {
                "success": True,
                "total": data.get("total", len(data.get("results", []))),
                "results": data.get("results", data.get("assets", [])),
            }
        return {"success": False, "total": 0, "results": [], "error": result.stderr}
    except Exception as e:
        return {"success": False, "total": 0, "results": [], "error": str(e)}
