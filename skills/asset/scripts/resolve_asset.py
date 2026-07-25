"""resolve_asset — Resolve an asset name to a full AssetDescriptor with variants.

Composes asset_source__search_assets with variant filtering and format/LOD
preference resolution.  Returns the full descriptor ready for import_asset.
"""
from __future__ import annotations

import json
import subprocess
import sys
from typing import Any


_CREATION_FLAGS = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0


def _run_cli(*args: str, timeout: int = 15) -> dict[str, Any] | None:
    """Run dcc-mcp-cli and return parsed JSON, or None on failure."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", *args, "--output", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
            creationflags=_CREATION_FLAGS,
        )
        if result.returncode != 0:
            return None
        return json.loads(result.stdout)
    except Exception:
        return None


def _filter_variants(
    variants: list[dict[str, Any]],
    preferred_format: str | None,
    preferred_lod: str | None,
) -> list[dict[str, Any]]:
    """Sort variants so format/LOD-preferring entries come first.

    Does not remove non-matching variants — callers still have the full set,
    but the preferred ones are first so variant_index=0 picks correctly.
    """
    if not preferred_format and not preferred_lod:
        return variants

    def _score(v: dict[str, Any]) -> int:
        s = 0
        if preferred_format and v.get("format") == preferred_format:
            s += 2
        if preferred_lod and v.get("lod") == preferred_lod:
            s += 1
        return -s  # Higher score sorts first

    return sorted(variants, key=_score)


def resolve_asset(
    asset_name: str,
    preferred_format: str | None = None,
    preferred_lod: str | None = None,
) -> dict[str, Any]:
    """Resolve an asset name to a full AssetDescriptor.

    Searches the catalog for the asset, finds the exact match, filters
    variants by format/LOD preference, and returns a complete descriptor
    ready for import.

    Args:
        asset_name: Exact asset name from search_assets result.
        preferred_format: Preferred format (e.g. 'fbx', 'usd', 'abc').
            First available variant when omitted.
        preferred_lod: Preferred LOD — 'proxy', 'low', 'medium', 'high'.
            Closest available when omitted.

    Returns:
        Full AssetDescriptor with resolved variants, attribution, and metadata.
    """
    search_args: dict[str, Any] = {"query": asset_name, "limit": 5}

    data = _run_cli(
        "call", "asset_source__search_assets",
        "--json", json.dumps(search_args),
        timeout=15,
    )
    if data is None:
        return {
            "success": False,
            "resolved": False,
            "descriptor": None,
            "error": "Catalog search failed — asset_source not reachable. Check gateway health with verify_gateway.",
        }

    results = data.get("results", data.get("assets", []))
    if not results:
        return {
            "success": True,
            "resolved": False,
            "descriptor": None,
            "error": "Asset '{}' not found in any catalog. Try search_assets first to discover available names.".format(
                asset_name
            ),
        }

    # Prefer exact-match name, fall back to first result
    descriptor = None
    for r in results:
        if r.get("name") == asset_name:
            descriptor = r
            break
    if descriptor is None:
        descriptor = results[0]

    # Filter and sort variants by preference
    variants = descriptor.get("variants", [])
    variants = _filter_variants(variants, preferred_format, preferred_lod)

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
        "resolved": len(variants) > 0,
        "descriptor": resolved,
    }
