"""catalog_status — Report asset catalog health and statistics.

Composes dcc-mcp-cli calls to asset_source to enumerate catalogs and
gather per-catalog statistics: total assets, type breakdown, storage,
and health indicators.
"""
from __future__ import annotations

import json
import subprocess
from datetime import datetime, timezone
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


def _query_catalog(name: str, query: str = "", limit: int = 1) -> dict[str, Any] | None:
    """Query a specific catalog and return raw response."""
    search_args: dict[str, Any] = {"query": query, "limit": limit}
    if name and name != "all":
        search_args["catalog"] = name
    return _run_cli(
        "call", "asset_source__search_assets",
        "--json", json.dumps(search_args),
        timeout=10,
    )


def _list_catalogs() -> list[str]:
    """Discover available catalog names."""
    # Try a dedicated catalog list endpoint first
    data = _run_cli("call", "asset_source__list_catalogs", timeout=10)
    if data:
        return data.get("catalogs", data.get("names", []))

    # Fallback: query the default catalog and see if per-catalog info is returned
    data = _run_cli(
        "call", "asset_source__search_assets",
        "--json", json.dumps({"query": "", "limit": 1}),
        timeout=10,
    )
    if data:
        catalogs = data.get("catalogs", [])
        if catalogs:
            return [c.get("name", "default") for c in catalogs]
    return ["default"]


def catalog_status(catalog: str | None = None) -> dict[str, Any]:
    """Report asset catalog health and statistics.

    Enumerates all available catalogs (or a specific one) and gathers
    statistics: total assets, breakdown by type, storage usage, last
    update timestamp, and health status.

    Args:
        catalog: Specific catalog name. All catalogs when omitted.

    Returns:
        Per-catalog statistics and health report.
    """
    now_utc = datetime.now(timezone.utc).isoformat()
    catalog_list = [catalog] if catalog else _list_catalogs()

    catalogs: list[dict[str, Any]] = []
    overall_healthy = True
    total_assets = 0

    for cat_name in catalog_list:
        data = _query_catalog(cat_name)
        if data is None:
            catalogs.append({
                "name": cat_name,
                "total_assets": 0,
                "by_type": {},
                "storage_bytes": 0,
                "last_updated": now_utc,
                "healthy": False,
                "error": "Catalog unreachable — check gateway and network.",
            })
            overall_healthy = False
            continue

        cat_total = data.get("total", 0)
        total_assets += cat_total

        catalogs.append({
            "name": cat_name,
            "total_assets": cat_total,
            "by_type": data.get("by_type", {}),
            "storage_bytes": data.get("storage_bytes", data.get("storage", 0)),
            "last_updated": data.get("last_updated", now_utc),
            "healthy": True,
        })

    return {
        "success": True,
        "catalogs": catalogs,
        "total_assets": total_assets,
        "all_healthy": overall_healthy,
    }
