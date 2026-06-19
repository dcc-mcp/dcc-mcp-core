"""Search the asset catalog and return matching AssetDescriptor entries.

This is a demo/mock source that returns static catalog entries. Production
sources should replace the catalog with a real asset database or download
helper while keeping the same AssetDescriptor contract.
"""

from __future__ import annotations

from dcc_mcp_core.asset_import import AssetAttribution
from dcc_mcp_core.asset_import import AssetDescriptor
from dcc_mcp_core.asset_import import AssetFileVariant
from dcc_mcp_core.asset_import import AssetFormat
from dcc_mcp_core.asset_import import AxisHint
from dcc_mcp_core.asset_import import UnitHint
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

# ---------------------------------------------------------------------------
# Static demo catalog — replace with a real asset registry in production
# ---------------------------------------------------------------------------

_DEMO_CATALOG: list[AssetDescriptor] = [
    AssetDescriptor(
        asset_id="props/table-round",
        variants=[
            AssetFileVariant(
                local_path="/data/assets/props/table_round.obj",
                format=AssetFormat.OBJ,
                preferred=True,
                mime="model/obj",
            ),
        ],
        attribution=AssetAttribution(
            source_url="https://example.com/assets/table-round",
            license_spdx="CC-BY-4.0",
            author="Example Studio",
            title="Round Table",
        ),
        unit_hint=UnitHint.CENTIMETER,
        meters_per_unit=0.01,
        up_axis=AxisHint.Y,
        tags=["furniture", "indoor", "table"],
    ),
    AssetDescriptor(
        asset_id="arch/city-bank/desk",
        variants=[
            AssetFileVariant(
                local_path="/data/assets/arch/desk.fbx",
                format=AssetFormat.FBX,
                preferred=True,
                mime="model/fbx",
            ),
        ],
        attribution=AssetAttribution(
            source_url="https://example.com/assets/desk",
            license_spdx="CC-BY-4.0",
            author="City Bank Studio",
            title="Bank Desk",
        ),
        unit_hint=UnitHint.CENTIMETER,
        meters_per_unit=0.01,
        up_axis=AxisHint.Y,
        scale_hint=1.0,
        source_bbox={"min": [-60.0, 0.0, -40.0], "max": [60.0, 80.0, 40.0]},
        tags=["furniture", "indoor", "desk"],
    ),
    AssetDescriptor(
        asset_id="characters/robot-v2",
        variants=[
            AssetFileVariant(
                local_path="/data/assets/chars/robot_v2.fbx",
                format=AssetFormat.FBX,
                preferred=True,
                mime="model/fbx",
            ),
        ],
        attribution=AssetAttribution(
            source_url="https://example.com/assets/robot-v2",
            license_spdx="CC-BY-4.0",
            author="Robot Corp",
            title="Robot V2",
        ),
        preview="/data/assets/chars/robot_thumb.png",
        unit_hint=UnitHint.METER,
        meters_per_unit=1.0,
        up_axis=AxisHint.Y,
        tags=["character", "robot", "scifi"],
    ),
    AssetDescriptor(
        asset_id="sets/sci-fi-corridor",
        variants=[
            AssetFileVariant(
                local_path="/data/assets/sets/corridor.usd",
                format=AssetFormat.USD,
                preferred=True,
                mime="model/vnd.usd+zip",
            ),
            AssetFileVariant(
                local_path="/data/assets/sets/corridor.usdz",
                format=AssetFormat.USDZ,
                preferred=False,
                mime="model/vnd.usdz+zip",
            ),
        ],
        attribution=AssetAttribution(
            source_url="https://example.com/assets/corridor",
            license_spdx="CC-BY-4.0",
            author="Set Builders Inc",
            title="Sci-Fi Corridor",
        ),
        unit_hint=UnitHint.METER,
        meters_per_unit=1.0,
        up_axis=AxisHint.Z,
        scale_hint=0.01,
        tags=["environment", "scifi", "corridor"],
    ),
    AssetDescriptor(
        asset_id="props/chair-modern",
        variants=[
            AssetFileVariant(
                local_path="/data/assets/props/chair_modern.fbx",
                format=AssetFormat.FBX,
                preferred=True,
                mime="model/fbx",
            ),
        ],
        attribution=AssetAttribution(
            source_url="https://example.com/assets/chair-modern",
            license_spdx="CC0-1.0",
            author="Free Assets Org",
            title="Modern Chair",
        ),
        unit_hint=UnitHint.CENTIMETER,
        meters_per_unit=0.01,
        up_axis=AxisHint.Y,
        tags=["furniture", "indoor", "chair"],
    ),
]


def _match_score(desc: AssetDescriptor, query: str) -> float:
    """Compute a simple relevance score for a descriptor against a query.

    Higher score = better match. Case-insensitive substring match on asset_id
    and tags.
    """
    q = query.lower()
    score = 0.0

    # Exact asset_id match
    if q == desc.asset_id.lower():
        score += 100.0
    # Substring in asset_id
    elif q in desc.asset_id.lower():
        score += 50.0

    # Tag matches
    for tag in desc.tags:
        tag_lower = tag.lower()
        if q == tag_lower:
            score += 30.0
        elif q in tag_lower:
            score += 10.0

    # Attribution title/author match
    if desc.attribution:
        if desc.attribution.title and q in desc.attribution.title.lower():
            score += 15.0
        if desc.attribution.author and q in desc.attribution.author.lower():
            score += 5.0

    return score


def search_assets(query: str, limit: int = 10) -> dict:
    """Search the asset catalog for matching descriptors.

    Args:
        query: Search term (asset name, id, or keyword).
        limit: Maximum number of results (default 10, max 50).

    Returns:
        ActionResultModel dict with matched descriptors.
    """
    query = query.strip()
    if not query:
        return skill_error("Empty query", "query must not be empty")

    limit = max(1, min(limit, 50))

    # Score and sort
    scored = [(desc, _match_score(desc, query)) for desc in _DEMO_CATALOG]
    scored = [(desc, score) for desc, score in scored if score > 0]
    scored.sort(key=lambda x: x[1], reverse=True)

    results = scored[:limit]
    if not results:
        return skill_success(
            f"No assets found for '{query}'",
            results=[],
            total=0,
            query=query,
            prompt="Try a different search term or check the asset catalog.",
        )

    descriptors = [desc.to_dict() for desc, _score in results]
    scores = [score for _desc, score in results]

    return skill_success(
        f"Found {len(results)} asset(s) for '{query}'",
        results=descriptors,
        scores=scores,
        total=len(results),
        query=query,
        prompt="Select a descriptor and pass it to an import-to-scene skill.",
    )


@skill_entry
def main(**kwargs) -> dict:
    """Entry point; delegates to :func:`search_assets`."""
    return search_assets(**kwargs)


if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main

    run_main(main)
