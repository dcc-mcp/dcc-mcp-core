"""Actions package for DCC-MCP-Core.

This package contains modules related to action management, including action loading,
registration, and execution.
"""

# Import local modules
from dcc_mcp_core.actions.generator import generate_action_for_ai
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.actions.metadata import extract_action_metadata

__all__ = [
    "ActionManager",
    "create_action_manager",
    "extract_action_metadata",
    "generate_action_for_ai",
    "get_action_manager",
]
