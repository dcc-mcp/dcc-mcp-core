"""Function adapter for Action classes.

This module provides adapter functions to convert Action classes to callable functions,
making them compatible with function-based APIs. This is particularly useful when
integrating with systems that expect function-based interfaces rather than class-based ones.
"""

# Import built-in modules
from typing import Callable
from typing import Dict

# Import local modules
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel


def create_function_adapter(action_name: str, dcc_name: str = None) -> Callable:
    """Create a function adapter for an Action class.

    This function creates an adapter that converts a function call to an Action class instance,
    sets it up, and processes the input parameters. The adapter function has the same signature
    as the Action's setup method, making it compatible with function-based APIs.

    Args:
        action_name: Name of the Action to adapt
        dcc_name: Optional DCC name to get a DCC-specific action

    Returns:
        Callable: Function adapter that takes the same parameters as the Action
        
    Example:
        >>> create_sphere = create_function_adapter("create_sphere", "maya")
        >>> result = create_sphere(radius=1.0, segments=32)
    """
    from logging import getLogger
    logger = getLogger(__name__)

    def adapter_function(**kwargs) -> ActionResultModel:
        """Adapter function that forwards calls to the Action class.

        Args:
            **kwargs: Input parameters for the Action

        Returns:
            ActionResultModel: Result of the Action execution
        """
        # Get Action class
        registry = ActionRegistry()
        action_class = registry.get_action(action_name, dcc_name=dcc_name)
        
        if not action_class:
            logger.warning(f"Action {action_name} not found in registry")
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} not found",
                error=f"Action {action_name} not found in registry",
                prompt="Please check the action name or register the action first",
                context={},
            )

        try:
            # Create Action instance, setup, and process
            action = action_class()
            action.setup(**kwargs)
            return action.process()
        except Exception as e:
            logger.error(f"Error executing action {action_name}: {str(e)}")
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} execution failed",
                error=str(e),
                prompt="Please check the input parameters and try again",
                context={},
            )

    # Set function name and docstring
    adapter_function.__name__ = f"{action_name}_adapter"
    adapter_function.__doc__ = f"Function adapter for {action_name} action."
    
    return adapter_function


def create_function_adapters(dcc_name: str = None) -> Dict[str, Callable]:
    """Create function adapters for all registered Actions.

    This function creates adapter functions for all registered Actions,
    optionally filtering by DCC name. The returned dictionary maps action
    names to their corresponding adapter functions.
    
    Args:
        dcc_name: Optional DCC name to filter actions by

    Returns:
        Dict[str, Callable]: Dictionary mapping action names to function adapters
        
    Example:
        >>> maya_functions = create_function_adapters("maya")
        >>> result = maya_functions["create_sphere"](radius=1.0)
    """
    from logging import getLogger
    logger = getLogger(__name__)
    
    registry = ActionRegistry()
    adapters = {}

    # Get all action metadata
    action_list = registry.list_actions(dcc_name=dcc_name)
    logger.info(f"Creating function adapters for {len(action_list)} actions")
    
    # Create adapter function for each action
    for action_info in action_list:
        name = action_info["internal_name"]
        adapters[name] = create_function_adapter(name, dcc_name=dcc_name)
        logger.debug(f"Created function adapter for {name}")

    return adapters


def create_function_adapters_for_manager(manager_name: str, dcc_name: str) -> Dict[str, Callable]:
    """Create function adapters using an ActionManager instance.
    
    This function creates adapter functions for all actions registered with
    a specific ActionManager instance. This is useful when you need to use
    the context and middleware provided by the manager.
    
    Args:
        manager_name: Name of the ActionManager instance
        dcc_name: DCC name for the ActionManager
        
    Returns:
        Dict[str, Callable]: Dictionary mapping action names to function adapters
        
    Example:
        >>> maya_functions = create_function_adapters_for_manager("default", "maya")
        >>> result = maya_functions["create_sphere"](radius=1.0)
    """
    from logging import getLogger
    from dcc_mcp_core.actions.manager import get_action_manager
    
    logger = getLogger(__name__)
    
    # Get ActionManager instance
    manager = get_action_manager(dcc_name, name=manager_name)
    adapters = {}
    
    # Get available action names
    action_names = manager.list_available_actions()
    logger.info(f"Creating function adapters for {len(action_names)} actions using manager {manager_name}")
    
    # Create adapter functions
    for name in action_names:
        # Create adapter function using manager
        def create_manager_adapter(action_name):
            def adapter_function(**kwargs):
                return manager.call_action(action_name, **kwargs)
            return adapter_function
            
        adapters[name] = create_manager_adapter(name)
        adapters[name].__name__ = f"{name}_adapter"
        adapters[name].__doc__ = f"Function adapter for {name} action using {manager_name} manager."
        logger.debug(f"Created function adapter for {name} using manager {manager_name}")
    
    return adapters
