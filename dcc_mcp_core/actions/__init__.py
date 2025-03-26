"""Actions package for DCC-MCP-Core.

This package contains modules related to action management, including action loading,
registration, and execution.
"""

# Import local modules
from dcc_mcp_core.actions.adapter import create_function_adapter
from dcc_mcp_core.actions.adapter import create_function_adapters
from dcc_mcp_core.actions.base import Action

# Class-based API imports
from dcc_mcp_core.actions.generator import generate_action_for_ai
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.actions.registry import ActionRegistry

# Create global registry instance
registry = ActionRegistry()

__all__ = [
    "Action",
    "ActionManager",
    "ActionRegistry",
    "create_action_manager",
    "create_function_adapter",
    "create_function_adapters",
    "generate_action_for_ai",
    "get_action_manager",
    "registry",
]
