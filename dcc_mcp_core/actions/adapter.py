"""Adapter layer for Action classes.

This module provides adapter functions to convert Action classes to callable functions,
making them compatible with function-based APIs.
"""

# Import built-in modules
from typing import Callable
from typing import Dict

# Import local modules
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel


def create_function_adapter(action_name: str) -> Callable:
    """Create a function adapter for an Action class.

    This function creates an adapter that converts a function call to an Action class instance,
    sets it up, and processes the input parameters.

    Args:
        action_name: Name of the Action to adapt

    Returns:
        Callable: Function adapter that takes the same parameters as the Action

    """

    def adapter_function(**kwargs) -> ActionResultModel:
        """Adapter function that forwards calls to the Action class.

        Args:
            **kwargs: Input parameters for the Action

        Returns:
            ActionResultModel: Result of the Action execution

        """
        registry = ActionRegistry()
        action_class = registry.get_action(action_name)
        if not action_class:
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} not found",
                error=f"Action {action_name} not found in registry",
                prompt="Please check the action name or register the action first",
                context={},
            )

        # Create Action instance, set it up, and process
        action = action_class().setup(**kwargs)
        return action.process()

    return adapter_function


def create_function_adapters() -> Dict[str, Callable]:
    """Create function adapters for all registered Actions.

    Returns:
        Dict[str, Callable]: Dictionary mapping action names to function adapters

    """
    registry = ActionRegistry()
    adapters = {}

    # Get all actions from the registry
    for action_info in registry.list_actions():
        name = action_info["name"]
        adapters[name] = create_function_adapter(name)

    return adapters
