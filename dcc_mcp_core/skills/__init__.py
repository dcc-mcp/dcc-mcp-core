"""Skills package for DCC-MCP-Core.

This package provides the Skill system that enables zero-code registration of
scripts (Python, MEL, MaxScript, BAT, Shell, etc.) as MCP-discoverable tools.

It directly reuses the OpenClaw Skills ecosystem format (SKILL.md + scripts/).
"""

# Import local modules
from dcc_mcp_core.skills.loader import load_skill
from dcc_mcp_core.skills.loader import parse_skill_md
from dcc_mcp_core.skills.scanner import SkillScanner
from dcc_mcp_core.skills.scanner import scan_skill_paths
from dcc_mcp_core.skills.script_action import create_script_action

__all__ = [
    "SkillScanner",
    "create_script_action",
    "load_skill",
    "parse_skill_md",
    "scan_skill_paths",
]
