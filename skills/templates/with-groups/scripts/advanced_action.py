"""Advanced tool — hidden until group is activated.

This tool is in the 'advanced' group (default-active: false). The AI agent
must call activate_tool_group("my-grouped-skill", "advanced") before this
tool appears in tools/list. Replace with your power-user tool logic.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def main(params: dict) -> dict:
    """Process input with the advanced action."""
    value = params.get("input")
    if not value:
        return skill_error("Missing required parameter: input")

    mode = params.get("mode", "balanced")
    return skill_success(
        f"Advanced action processed: {value} (mode={mode})",
        input=value,
        mode=mode,
    )


if __name__ == "__main__":
    skill_entry(main)
