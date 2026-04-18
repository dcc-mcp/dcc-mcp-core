"""DCC-specific skill script template.

This script runs inside a DCC application's Python environment.
It receives parameters as JSON on stdin and must print a JSON result to stdout.

Replace the body with actual DCC API calls (e.g. maya.cmds, bpy, hou).
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def main(params: dict) -> dict:
    """Execute a command in the target DCC."""
    command = params.get("command")
    if not command:
        return skill_error("Missing required parameter: command")

    # ── Replace this block with your DCC logic ──────────────────────
    # Example for Maya:
    #   import maya.cmds as cmds
    #   result = cmds.eval(command)
    #
    # Example for Blender:
    #   import bpy
    #   exec(command)
    # ────────────────────────────────────────────────────────────────

    return skill_success(
        f"Executed: {command}",
        command=command,
    )


if __name__ == "__main__":
    skill_entry(main)
