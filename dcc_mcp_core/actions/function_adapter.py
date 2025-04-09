"""Function adapter for Action classes.

This module provides adapter functions to convert Action classes to callable functions,
making them compatible with function-based APIs. This is particularly useful when
integrating with systems that expect function-based interfaces rather than class-based ones.

The module offers three main approaches to creating function adapters:
1. Creating a single function adapter for a specific action
2. Creating multiple function adapters for all actions of a specific DCC
3. Creating function adapters that use a specific ActionManager instance

Using an ActionManager-based adapter is recommended when you need middleware,
context management, or other features provided by the ActionManager.
"""

# Import built-in modules
import logging
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel

# Module logger
logger = logging.getLogger(__name__)


def create_function_adapter(
    action_name: str, dcc_name: Optional[str] = None, manager=None, context: Optional[Dict] = None
) -> Callable:
    """Create a function adapter for an Action class.

    This function creates an adapter that converts a function call to an Action class instance,
    sets it up, and processes the input parameters. The adapter function has the same signature
    as the Action's setup method, making it compatible with function-based APIs.

    There are two modes of operation:
    1. Using an ActionManager (recommended): If manager is provided, the adapter will use
       the manager's call_action method, which includes middleware processing and context management.
    2. Direct Action instantiation: If manager is None, the adapter will directly instantiate
       the Action class and call its process method.

    Args:
        action_name: Name of the Action to adapt
        dcc_name: Optional DCC name to get a DCC-specific action
        manager: Optional ActionManager instance to use for calling the action
        context: Optional context dictionary to use when creating the Action instance

    Returns:
        Callable: Function adapter that takes the same parameters as the Action

    Example:
        >>> # Direct instantiation
        >>> create_sphere = create_function_adapter("create_sphere", "maya")
        >>> result = create_sphere(radius=1.0, segments=32)
        >>>
        >>> # Using an ActionManager
        >>> from dcc_mcp_core.actions.manager import get_action_manager
        >>> manager = get_action_manager("maya")
        >>> create_sphere = create_function_adapter("create_sphere", manager=manager)
        >>> result = create_sphere(radius=1.0, segments=32)

    """
    # Import built-in modules
    from logging import getLogger

    logger = getLogger(__name__)

    # If manager is provided, use it to call the action
    if manager is not None:

        def manager_adapter_function(**kwargs) -> ActionResultModel:
            """Adapter function that uses an ActionManager to call the action.

            Args:
                **kwargs: Input parameters for the Action

            Returns:
                ActionResultModel: Result of the Action execution

            """
            return manager.call_action(action_name, context=context, **kwargs)

        # Set function name and docstring
        manager_adapter_function.__name__ = f"{action_name}_adapter"
        manager_adapter_function.__doc__ = f"Function adapter for {action_name} action using {manager.name} manager."

        return manager_adapter_function

    # Otherwise, directly instantiate the Action class
    def direct_adapter_function(**kwargs) -> ActionResultModel:
        """Adapter function that directly instantiates and processes the Action.

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
            action = action_class(context=context)
            action.setup(**kwargs)
            return action.process()
        except Exception as e:
            logger.error(f"Error executing action {action_name}: {e!s}")
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} execution failed",
                error=str(e),
                prompt="Please check the input parameters and try again",
                context={},
            )

    # Set function name and docstring
    direct_adapter_function.__name__ = f"{action_name}_adapter"
    direct_adapter_function.__doc__ = f"Function adapter for {action_name} action."

    return direct_adapter_function


def create_function_adapters(
    dcc_name: Optional[str] = None,
    manager=None,
    context: Optional[Dict] = None,
    action_names: Optional[List[str]] = None,
) -> Dict[str, Callable]:
    """Create function adapters for multiple Actions.

    This function creates adapter functions for multiple Actions, optionally filtering
    by DCC name or using a specific list of action names. The returned dictionary maps
    action names to their corresponding adapter functions.

    There are two modes of operation:
    1. Using an ActionManager (recommended): If manager is provided, the adapters will use
       the manager's call_action method, which includes middleware processing and context management.
    2. Direct Action instantiation: If manager is None, the adapters will directly instantiate
       the Action classes and call their process methods.

    Args:
        dcc_name: Optional DCC name to filter actions by
        manager: Optional ActionManager instance to use for calling the actions
        context: Optional context dictionary to use when creating the Action instances
        action_names: Optional list of specific action names to create adapters for

    Returns:
        Dict[str, Callable]: Dictionary mapping action names to function adapters

    Example:
        >>> # Direct instantiation
        >>> maya_functions = create_function_adapters("maya")
        >>> result = maya_functions["create_sphere"](radius=1.0)
        >>>
        >>> # Using an ActionManager
        >>> from dcc_mcp_core.actions.manager import get_action_manager
        >>> manager = get_action_manager("maya")
        >>> maya_functions = create_function_adapters(manager=manager)
        >>> result = maya_functions["create_sphere"](radius=1.0)

    """
    # Import built-in modules
    from logging import getLogger

    logger = getLogger(__name__)

    adapters = {}

    # If specific action names are provided, use those
    if action_names:
        logger.info(f"Creating function adapters for {len(action_names)} specified actions")
        for name in action_names:
            adapters[name] = create_function_adapter(
                action_name=name, dcc_name=dcc_name, manager=manager, context=context
            )
            logger.debug(f"Created function adapter for {name}")
        return adapters

    # Otherwise, get all available actions
    if manager:
        # If manager is provided, use it to get available actions
        available_actions = manager.list_available_actions()
        logger.info(f"Creating function adapters for {len(available_actions)} actions using manager {manager.name}")

        for name in available_actions:
            adapters[name] = create_function_adapter(action_name=name, manager=manager, context=context)
            logger.debug(f"Created function adapter for {name} using manager {manager.name}")
    else:
        # Otherwise, use the registry directly
        registry = ActionRegistry()
        action_list = registry.list_actions(dcc_name=dcc_name)
        logger.info(f"Creating function adapters for {len(action_list)} actions")

        for action_info in action_list:
            name = action_info["internal_name"]
            adapters[name] = create_function_adapter(action_name=name, dcc_name=dcc_name, context=context)
            logger.debug(f"Created function adapter for {name}")

    return adapters


# The create_function_adapters_for_manager function has been removed
# as part of the refactoring to simplify the API.
