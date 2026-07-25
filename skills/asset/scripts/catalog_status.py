"""Report catalog statistics and health.

Aggregates statistics from the asset-source catalog:
- Total asset count
- Breakdown by file format
- Tag frequency distribution
- Attribution coverage (how many assets have license/author info)
- Format health (how many assets per format)

Useful for catalog health checks, gap analysis, and overview.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry, skill_error, skill_success


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _fetch_catalog():
    """Return the full catalog as a list of AssetDescriptor dicts.

    Tries the gateway path first, then falls back to local catalog.
    """
    # Try gateway tool dispatch first
    try:
        from dcc_mcp_core.skills_helper import call_tool as _call
        from dcc_mcp_core.skills_helper import search_tools as _search

        tools = _search("search_assets")
        if tools and isinstance(tools, list):
            for entry in tools:
                tool_slug = entry.get("tool_slug") or entry.get("name", "")
                if "search_assets" in tool_slug:
                    # Search with empty wildcard to get all results
                    response = _call(tool_slug, {
                        "query": "",
                        "limit": 50,
                    })
                    if response.get("success") and response.get("context", {}).get("results"):
                        return response["context"]["results"]
    except (ImportError, Exception):
        pass

    # Fallback: local catalog
    try:
        from skills.asset_source.scripts.search_assets import _DEMO_CATALOG
        return [desc.to_dict() for desc in _DEMO_CATALOG]
    except ImportError:
        pass

    return []


def _compute_format_stats(catalog, format_filter=None):
    """Compute breakdown by file format."""
    counts = {}
    for item in catalog:
        variants = item.get("variants", [])
        for v in variants:
            fmt = v.get("format", "unknown")
            if format_filter and fmt != format_filter:
                continue
            counts[fmt] = counts.get(fmt, 0) + 1
    return counts


def _compute_tag_stats(catalog, tag_filter=None):
    """Compute tag frequency distribution."""
    counts = {}
    for item in catalog:
        tags = item.get("tags", [])
        for tag in tags:
            if tag_filter and tag != tag_filter:
                continue
            counts[tag] = counts.get(tag, 0) + 1
    # Sort by frequency descending
    return dict(sorted(counts.items(), key=lambda x: x[1], reverse=True))


def _compute_attribution_coverage(catalog):
    """Compute attribution metadata coverage."""
    total = len(catalog)
    if total == 0:
        return {
            "total": 0,
            "with_attribution": 0,
            "with_license": 0,
            "with_author": 0,
            "with_title": 0,
            "coverage_pct": 0.0,
        }

    with_attr = 0
    with_license = 0
    with_author = 0
    with_title = 0

    for item in catalog:
        attr = item.get("attribution")
        if attr:
            with_attr += 1
            if attr.get("license_spdx") or attr.get("license_text"):
                with_license += 1
            if attr.get("author"):
                with_author += 1
            if attr.get("title"):
                with_title += 1

    return {
        "total": total,
        "with_attribution": with_attr,
        "with_license": with_license,
        "with_author": with_author,
        "with_title": with_title,
        "coverage_pct": round(with_attr * 100.0 / total, 1) if total > 0 else 0.0,
    }


def _compute_unit_stats(catalog):
    """Compute breakdown by unit system."""
    counts = {}
    for item in catalog:
        unit = item.get("unit_hint", "unitless")
        counts[unit] = counts.get(unit, 0) + 1
    return counts


# ---------------------------------------------------------------------------
# Tool entry point
# ---------------------------------------------------------------------------


@skill_entry
def main(format_filter=None, tag_filter=None, include_empty=False, **kwargs):
    """Report catalog statistics.

    Args:
        format_filter: Optional format to restrict statistics to.
        tag_filter: Optional tag to restrict statistics to.
        include_empty: Include entries with zero counts.

    Returns:
        Skill result dict with catalog statistics in context.

    """
    catalog = _fetch_catalog()
    total = len(catalog)

    if total == 0:
        return skill_error(
            "Catalog is empty",
            "No assets found in the catalog",
            prompt="Populate the catalog first or check the asset-source configuration.",
        )

    # Compute all breakdowns
    format_stats = _compute_format_stats(catalog, format_filter)
    tag_stats = _compute_tag_stats(catalog, tag_filter)
    attr_coverage = _compute_attribution_coverage(catalog)
    unit_stats = _compute_unit_stats(catalog)

    # Filter out zeros unless include_empty is set
    if not include_empty:
        format_stats = {k: v for k, v in format_stats.items() if v > 0}
        tag_stats = {k: v for k, v in tag_stats.items() if v > 0}
        unit_stats = {k: v for k, v in unit_stats.items() if v > 0}

    # Determine unique format count
    unique_formats = len(format_stats)
    top_format = max(format_stats.items(), key=lambda x: x[1]) if format_stats else ("none", 0)

    # Asset IDs for reference
    asset_ids = [item.get("asset_id", "") for item in catalog]

    filter_desc = []
    if format_filter:
        filter_desc.append("format=%s" % format_filter)
    if tag_filter:
        filter_desc.append("tag=%s" % tag_filter)
    filter_str = (" (filtered: %s)" % ", ".join(filter_desc)) if filter_desc else ""

    return skill_success(
        "Catalog has %d asset(s)%s across %d format(s)" % (
            total, filter_str, unique_formats,
        ),
        prompt=(
            "Use resolve_asset to look up individual assets, or "
            "asset-source search_assets to search by keyword."
        ),
        total=total,
        unique_formats=unique_formats,
        top_format=top_format[0],
        top_format_count=top_format[1],
        format_breakdown=format_stats,
        tag_breakdown=tag_stats,
        unit_breakdown=unit_stats,
        attribution_coverage=attr_coverage,
        asset_ids=asset_ids,
    )


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
