"""primary_action — replace with your domain skill logic.

This script is the entry point for the `primary_action` tool declared in SKILL.md.
The dispatcher calls `main(**kwargs)` and captures the return value as the tool result.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


@skill_entry
def primary_action(required_param: str = "", optional_param: float = 1.0, **kwargs) -> dict:
    """Execute the primary domain action.

    Replace this implementation with your actual DCC logic.
    Import DCC-specific modules (maya.cmds, bpy, hou, etc.) inside the
    function body so the skill loads in non-DCC environments too.
    """
    if not required_param:
        return skill_error("required_param is missing", "Provide a non-empty required_param value.")

    # ---- your DCC logic here ----
    # try:
    #     import maya.cmds as cmds
    #     result = cmds.something(required_param)
    # except ImportError:
    #     return skill_error("DCC not available", "This tool requires Maya.")

    return skill_success(
        f"primary_action completed for {required_param!r}",
        # Extra kwargs become the tool's context output and are available
        # to downstream tools via next-tools chaining.
        required_param=required_param,
        optional_param=optional_param,
    )
