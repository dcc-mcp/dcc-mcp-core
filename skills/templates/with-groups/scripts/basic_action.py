"""Basic tool — active by default.

This tool is in the 'basic' group and is visible immediately when the skill
is loaded. Replace with your default-active tool logic.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_success


def main(params: dict) -> dict:
    """Process input with the basic action."""
    value = params.get("input", "default")
    return skill_success(f"Basic action processed: {value}", input=value)


if __name__ == "__main__":
    skill_entry(main)
