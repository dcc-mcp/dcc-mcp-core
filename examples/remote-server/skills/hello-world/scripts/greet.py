"""Read-only smoke tool for the standalone MCP service example."""

from __future__ import annotations

from dcc_mcp_core.skills_helper import run_main
from dcc_mcp_core.skills_helper import skill_entry
from dcc_mcp_core.skills_helper import skill_error
from dcc_mcp_core.skills_helper import skill_success


@skill_entry
def main(name: str = "World"):
    """Return a bounded greeting for connectivity validation."""
    if not isinstance(name, str) or not name.strip():
        return skill_error("Invalid name", "Name must be a non-empty string.")
    return skill_success(
        f"Hello, {name.strip()}! (from the internal MCP service)",
        name=name.strip(),
    )


if __name__ == "__main__":
    run_main(main)
