"""Plan a deterministic cross-DCC spatial conversion."""

from __future__ import annotations

from typing import Any
from typing import Dict
from typing import List
from typing import Optional

from dcc_mcp_core import plan_spatial_conversion
from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


@skill_entry
def main(
    source: Dict[str, Any],
    target: Dict[str, Any],
    sample_point: Optional[List[float]] = None,
    **kwargs: Any,
) -> Dict[str, Any]:
    """Return a conversion plan without touching a DCC scene."""
    try:
        plan = plan_spatial_conversion(source, target, sample_point)
    except (KeyError, TypeError, ValueError) as exc:
        return skill_error(
            "Spatial conversion could not be planned",
            "invalid_spatial_conversion",
            prompt="Inspect the live source and target axes and units, then provide three distinct signed axes.",
            _meta={
                "dcc.error": {
                    "type": type(exc).__name__,
                    "message": str(exc),
                }
            },
        )
    return skill_success("Spatial conversion planned", **plan)


if __name__ == "__main__":
    run_main(main)
