"""Actions package for DCC-MCP-Core.

This package contains modules related to action management, including action loading,
registration, and execution.
"""

# Import local modules
from dcc_mcp_core.actions.base import Action

# Function adapter imports
from dcc_mcp_core.actions.function_adapter import create_function_adapter
from dcc_mcp_core.actions.function_adapter import create_function_adapters
from dcc_mcp_core.actions.function_adapter import create_function_adapters_for_manager

# Class-based API imports
from dcc_mcp_core.actions.generator import generate_action_for_ai
from dcc_mcp_core.actions.manager import ActionManager
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.actions.registry import ActionRegistry

# Create global registry instance
registry = ActionRegistry()

__all__ = [
    # Basic classes
    "Action",
    "ActionManager",
    "ActionRegistry",
    
    # Manager related functions
    "create_action_manager",
    "get_action_manager",
    
    # Function adapter
    "create_function_adapter",
    "create_function_adapters",
    "create_function_adapters_for_manager",
    
    # Tool functions
    "generate_action_for_ai",
    
    # Global instance
    "registry",
]
