"""Minimal skill script template.

This script is invoked when the AI agent calls `my_skill__hello`.
It receives parameters as JSON on stdin and must print a JSON result to stdout.
"""

from __future__ import annotations

from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_success


def main(params: dict) -> dict:
    """Greet the user by name."""
    name = params.get("name", "World")
    return skill_success(f"Hello, {name}!")


if __name__ == "__main__":
    skill_entry(main)
