"""read_only_query — replace with your domain skill query logic.

Read-only tools should never modify DCC state.
Mark them with read_only: true and idempotent: true in SKILL.md.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_success


@skill_entry
def read_only_query(filter: str = "", **kwargs) -> dict:
    """Query DCC state without modifying it.

    Replace this implementation with your actual read-only query.
    """
    # ---- your DCC query here ----
    # try:
    #     import maya.cmds as cmds
    #     items = cmds.ls(filter or "*", type="transform")
    # except ImportError:
    #     return skill_error("DCC not available", "This tool requires Maya.")

    items: list = []  # replace with actual query result

    return skill_success(
        f"Query returned {len(items)} items",
        items=items,
        filter=filter,
    )
