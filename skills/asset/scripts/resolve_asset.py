"""Resolve an asset by asset_id or search query into a full AssetDescriptor.

Delegates to the asset-source skill for catalog lookup. Returns the
descriptor with all variants and attribution metadata, ready for import.

If the asset_id is a direct catalog match it is returned immediately;
otherwise a fuzzy search is performed and the best match returned.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_error, skill_success
from dcc_mcp_core.asset_import import AssetDescriptor, AssetFileVariant, AssetFormat


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _format_from_extension(path):
    """Infer AssetFormat from a file extension."""
    ext = path.rsplit(".", 1)[-1].lower() if "." in path else ""
    mapping = {
        "obj": AssetFormat.OBJ,
        "fbx": AssetFormat.FBX,
        "gltf": AssetFormat.GLTF,
        "glb": AssetFormat.GLB,
        "usd": AssetFormat.USD,
        "usdz": AssetFormat.USDZ,
        "abc": AssetFormat.ABC,
        "blend": AssetFormat.BLEND,
    }
    return mapping.get(ext, AssetFormat.UNKNOWN)


def _build_descriptor_from_result(item, prefer_format=None):
    """Build an AssetDescriptor from an asset-source search result dict."""
    variants_raw = item.get("variants", [])
    variants = []
    for v in variants_raw:
        variant = AssetFileVariant.from_dict(v)
        variants.append(variant)

    attribution_raw = item.get("attribution")
    from dcc_mcp_core.asset_import import AssetAttribution
    attribution = AssetAttribution.from_dict(attribution_raw) if attribution_raw else None

    # Reorder variants so preferred format comes first
    if prefer_format and len(variants) > 1:
        preferred = [v for v in variants if v.format == prefer_format]
        others = [v for v in variants if v.format != prefer_format]
        variants = preferred + others

    desc = AssetDescriptor(
        asset_id=item.get("asset_id", "unknown"),
        variants=variants,
        attribution=attribution,
        preview=item.get("preview"),
        unit_hint=item.get("unit_hint", "unitless"),
        meters_per_unit=float(item.get("meters_per_unit", 1.0)),
        up_axis=item.get("up_axis", "y"),
        scale_hint=float(item["scale_hint"]) if item.get("scale_hint") is not None else None,
        source_bbox=item.get("source_bbox"),
        tags=list(item.get("tags", [])),
        extra=dict(item.get("extra", {})),
    )
    return desc


def _resolve_from_catalog(asset_id, prefer_format=None):
    """Query the asset-source skill's search_assets and return best match.

    Uses the gateway's tool search/call pattern via skills_helper.
    When running inside a DCC-MCP gateway, this will route through the
    gateway's dynamic capability surface. In standalone mode, falls back
    to the local catalog.
    """
    # Try gateway tool dispatch path first
    try:
        from dcc_mcp_core.skills_helper import search_skills as _search

        # Search for available DCC instances and the asset-source skill
        results = _search("asset-source search_assets")
        if results and len(results) > 0:
            # Call via gateway when available
            for entry in results:
                tool_slug = entry.get("tool_slug") or entry.get("name")
                if tool_slug and "search_assets" in tool_slug:
                    from dcc_mcp_core.skills_helper import call_tool as _call
                    response = _call(tool_slug, {"query": asset_id, "limit": 1})
                    if response.get("success") and response.get("context", {}).get("results"):
                        first = response["context"]["results"][0]
                        return _build_descriptor_from_result(first, prefer_format)
    except (ImportError, Exception):
        pass

    # Fallback: import local catalog directly
    return _resolve_from_local_catalog(asset_id, prefer_format)


def _resolve_from_local_catalog(asset_id, prefer_format=None):
    """Direct catalog lookup fallback when gateway is unavailable."""
    try:
        from skills.asset_source.scripts.search_assets import _DEMO_CATALOG
        from skills.asset_source.scripts.search_assets import _match_score

        query = asset_id.strip()
        scored = [(desc, _match_score(desc, query)) for desc in _DEMO_CATALOG]
        scored = [(desc, score) for desc, score in scored if score > 0]
        scored.sort(key=lambda x: x[1], reverse=True)

        if not scored:
            return None

        best_desc, best_score = scored[0]
        desc_dict = best_desc.to_dict()

        if prefer_format:
            variants = desc_dict.get("variants", [])
            preferred = [v for v in variants if v.get("format") == prefer_format]
            others = [v for v in variants if v.get("format") != prefer_format]
            desc_dict["variants"] = preferred + others

        return _build_descriptor_from_result(desc_dict, prefer_format)
    except ImportError:
        return None


# ---------------------------------------------------------------------------
# Tool entry point
# ---------------------------------------------------------------------------


@skill_entry
def main(asset_id, prefer_format=None, **kwargs):
    """Resolve an asset by asset_id or search query.

    Args:
        asset_id: Asset identifier (e.g. 'props/table-round') or search query.
        prefer_format: Optional preferred file format filter.

    Returns:
        Skill result dict with the resolved AssetDescriptor in context.

    """
    asset_id = asset_id.strip()
    if not asset_id:
        return skill_error("Empty asset_id", "asset_id must not be empty")

    descriptor = _resolve_from_catalog(asset_id, prefer_format)

    if descriptor is None:
        return skill_error(
            "Asset not found",
            "No matching asset for '%s'" % asset_id,
            prompt="Try a different asset_id or check the catalog with catalog_status.",
            query=asset_id,
        )

    try:
        descriptor.validate()
    except Exception as exc:
        return skill_error(
            "Asset descriptor validation failed",
            str(exc),
            query=asset_id,
        )

    desc_dict = descriptor.to_dict()
    variant_count = len(desc_dict.get("variants", []))
    preferred_path = ""
    for v in desc_dict.get("variants", []):
        if v.get("preferred"):
            preferred_path = v.get("local_path", "")
            break
    if not preferred_path and variant_count > 0:
        preferred_path = desc_dict["variants"][0].get("local_path", "")

    return skill_success(
        "Resolved asset '%s': %d variant(s)" % (descriptor.asset_id, variant_count),
        prompt="Pass the descriptor to import_asset to import into a DCC host.",
        descriptor=desc_dict,
        asset_id=descriptor.asset_id,
        variant_count=variant_count,
        preferred_path=preferred_path,
        query=asset_id,
    )


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
