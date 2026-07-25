"""catalog_status — Report asset catalog health and statistics."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def catalog_status(catalog: str | None = None) -> dict[str, Any]:
    """Report catalog health and statistics.

    Args:
        catalog: Named catalog. Defaults to all.

    Returns:
        Catalog health statistics.
    """
    # Query the asset source for catalog status
    catalogs: list[dict[str, Any]] = []

    try:
        # Search with empty query to get catalog overview
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "asset_source__search_assets",
             "--json", json.dumps({"query": "", "limit": 1}), "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            total = data.get("total", 0)
            catalogs.append({
                "name": catalog or "default",
                "total_assets": total,
                "by_type": data.get("by_type", {}),
                "storage_bytes": data.get("storage_bytes", 0),
                "last_updated": data.get("last_updated", "unknown"),
                "healthy": True,
            })
        else:
            catalogs.append({
                "name": catalog or "default",
                "total_assets": 0,
                "healthy": False,
                "error": result.stderr,
            })
    except Exception as e:
        catalogs.append({
            "name": catalog or "default",
            "total_assets": 0,
            "healthy": False,
            "error": str(e),
        })

    return {
        "success": True,
        "catalogs": catalogs,
    }
