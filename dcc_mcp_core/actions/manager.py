"""Action manager module for DCC-MCP-Core.

This module provides functionality for managing the lifecycle of Action instances,
including their creation, setup, execution, and result handling. It serves as the
central coordinator for the Action system, working with the ActionRegistry to
discover and register Action classes, and with the middleware system to process
Actions during execution.

The ActionManager class follows a clear separation of concerns:
- ActionRegistry: Discovers and registers Action classes
- ActionManager: Creates, sets up, and executes Action instances
- Middleware: Processes Actions during execution
- EventBus: Facilitates communication between components
"""

# Import built-in modules
import datetime
import logging
import os
import platform
import threading
import time
import traceback
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.actions.events import event_bus
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.middleware import MiddlewareChain
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.decorators import error_handler

logger = logging.getLogger(__name__)


class ActionManager:
    """Manager for Action lifecycle.

    This class is responsible for creating, setting up, and executing Action instances.
    It focuses on the lifecycle management of Actions, while the discovery and registration
    of Action classes is handled by the ActionRegistry.

    The ActionManager follows a clear separation of concerns:
    - ActionRegistry: Discovers and registers Action classes
    - ActionManager: Creates and executes Action instances

    Attributes:
        name (str): Unique name for this action manager instance
        dcc_name (str): Name of the DCC this action manager is for
        context (Dict[str, Any]): Context data to inject into actions
        registry (ActionRegistry): Registry for Action classes
        middleware_chain (MiddlewareChain): Chain of middleware for processing actions
        middleware (Callable): Built middleware chain for processing actions
        event_bus (EventBus): Event bus for publishing events
        _last_refresh_time (float): Time of last action refresh
        _refresh_interval (int): Interval between automatic refreshes in seconds

    """

    def __init__(
        self,
        name: str,
        dcc_name: str,
        context: Optional[Dict[str, Any]] = None,
        auto_refresh: bool = False,
        refresh_interval: int = 60,
        registry: Optional[ActionRegistry] = None,
    ) -> None:
        """Initialize a new ActionManager instance.

        Creates a new ActionManager instance with the specified name and DCC.
        The ActionManager is responsible for the lifecycle of Action instances,
        including their creation, setup, and execution.

        Args:
            name: Unique name for this action manager instance
            dcc_name: Name of the DCC this action manager is for
            context: Optional dictionary of context data to inject into actions
            auto_refresh: Whether to enable automatic refresh of actions
            refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
            registry: Optional ActionRegistry instance to use (creates a new one if not provided)

        Example:
            >>> manager = ActionManager("default", "maya")
            >>> manager.discover_actions_from_package("my_package")
            >>> result = manager.call_action("create_sphere", radius=1.0)

        """
        # Basic attributes
        self.name = name
        self.dcc_name = dcc_name
        self.context = context or {}

        # Use provided registry or create a new one
        self.registry = registry or ActionRegistry()

        # Initialize middleware chain
        self.middleware_chain = MiddlewareChain()
        self.middleware = None

        # Initialize event bus
        self.event_bus = event_bus

        # Refresh related settings
        self._last_refresh_time = None
        self._refresh_interval = refresh_interval
        self._auto_refresh = auto_refresh

        # Add default context data
        self._update_default_context()

        logger.info(f"Created ActionManager '{name}' for DCC '{dcc_name}'")

        # Publish creation event
        self.event_bus.publish("action_manager.created", {"manager": self, "name": name, "dcc_name": dcc_name})

    def _update_default_context(self) -> None:
        """Update the default context with common values.

        This method sets up the default context that will be provided to actions
        if no specific context is provided during execution. The default context
        includes common values such as the DCC name, manager name, and references
        to the manager and event bus.

        The context is used by actions to access shared resources and dependencies.

        Note:
            This method is called automatically during initialization and should
            not need to be called directly.

        """
        # Add basic information
        default_context = {
            # Basic identifier information
            "dcc_name": self.dcc_name,
            "manager_name": self.name,
            # Reference objects
            "manager": self,
            "event_bus": self.event_bus,
            "registry": self.registry,
            # System information
            "platform": platform.system().lower(),
            "python_version": platform.python_version(),
            "timestamp": datetime.datetime.now().isoformat(),
        }

        # Update context
        self.context.update(default_context)
        logger.debug(f"Updated default context for {self.name} manager with {len(default_context)} keys")

    def discover_actions_from_path(
        self, path: str, context: Optional[Dict[str, Any]] = None, dcc_name: Optional[str] = None
    ) -> List[Type[Action]]:
        """Discover and register Action classes from a file path.

        This method delegates to the ActionRegistry to discover and register
        Action classes from a file path. It publishes events before and after
        discovery to allow for monitoring and extension.

        Args:
            path: Path to the Python file to load
            context: Optional context to pass to the loaded module
            dcc_name: Optional DCC name to set for discovered actions

        Returns:
            List of discovered and registered Action classes

        """
        # Use provided context or default to self.context
        ctx = context or self.context
        # Use provided dcc_name or default to self.dcc_name
        dcc = dcc_name or self.dcc_name

        logger.debug(f"Discovering actions from path: {path}")

        # Publish before discovery event
        self.event_bus.publish("action_manager.before_discover_path", {"manager": self, "path": path, "dcc_name": dcc})

        # Delegate to registry
        discovered = self.registry.discover_actions_from_path(path=path, dependencies=ctx, dcc_name=dcc)

        # Publish after discovery event
        self.event_bus.publish(
            "action_manager.after_discover_path",
            {"manager": self, "path": path, "dcc_name": dcc, "discovered": discovered},
        )

        return discovered

    def discover_actions_from_package(self, package_name: str, dcc_name: Optional[str] = None) -> List[Type[Action]]:
        """Discover and register Action classes from a package.

        This method delegates to the ActionRegistry to discover and register
        Action classes from a package. It publishes events before and after
        discovery to allow for monitoring and extension.

        Args:
            package_name: Name of the package to search
            dcc_name: Optional DCC name to set for discovered actions

        Returns:
            List of discovered and registered Action classes

        """
        # Use provided dcc_name or default to self.dcc_name
        dcc = dcc_name or self.dcc_name

        logger.debug(f"Discovering actions from package: {package_name}")

        # Publish before discovery event
        self.event_bus.publish(
            "action_manager.before_discover_package", {"manager": self, "package_name": package_name, "dcc_name": dcc}
        )

        # Delegate to registry
        discovered = self.registry.discover_actions_from_package(package_name=package_name, dcc_name=dcc)

        # Publish after discovery event
        self.event_bus.publish(
            "action_manager.after_discover_package",
            {"manager": self, "package_name": package_name, "dcc_name": dcc, "discovered": discovered},
        )

        return discovered

    def refresh_actions(self, force: bool = False, action_paths: Optional[List[str]] = None) -> None:
        """Refresh actions from the registry.

        This method refreshes the actions from the registry, ensuring that
        the latest actions are available to the manager. By default, it only
        refreshes if the refresh interval has passed since the last refresh.

        Args:
            force: If True, forces a refresh regardless of the refresh interval
            action_paths: Optional list of paths to discover actions from

        """
        # Check if a refresh is needed
        current_time = time.time()
        needs_refresh = (
            force or not self._last_refresh_time or (current_time - self._last_refresh_time >= self._refresh_interval)
        )

        if not needs_refresh and not action_paths:
            logger.debug(
                f"Skipping refresh for {self.name} manager, "
                f"last refresh was {current_time - self._last_refresh_time:.2f}s ago"
            )
            return

        logger.info(f"Refreshing actions for {self.name} manager ({self.dcc_name})")

        # Update last refresh time
        self._last_refresh_time = current_time

        # Publish before refresh event
        self.event_bus.publish(
            "action_manager.before_refresh",
            {"manager": self, "dcc_name": self.dcc_name},
        )

        # Refresh registry
        self.registry.refresh()

        # If action_paths is provided, discover actions from those paths
        if action_paths:
            for path in action_paths:
                self.discover_actions_from_path(path=path, context=self.context, dcc_name=self.dcc_name)

        # Publish after refresh event
        self.event_bus.publish(
            "action_manager.after_refresh",
            {"manager": self, "dcc_name": self.dcc_name},
        )

        logger.debug(f"Refreshed actions for {self.name} manager, found {len(self.list_available_actions())} actions")

    @error_handler
    def call_action(self, action_name: str, context: Optional[Dict[str, Any]] = None, **kwargs) -> ActionResultModel:
        """Call an action by name.

        This method creates an Action instance, sets it up with the provided parameters,
        and processes it. It also handles event publishing and middleware processing.

        The execution flow is as follows:
        1. Get the Action class from the registry
        2. Create an Action instance with the merged context
        3. Set up the Action with the provided parameters
        4. Publish a before-execution event
        5. Process the Action (using middleware if available)
        6. Publish an after-execution event
        7. Return the result

        Args:
            action_name: Name of the action to call
            context: Optional dictionary of context data and dependencies
            **kwargs: Arguments to pass to the action

        Returns:
            ActionResultModel: Result of the action execution

        """
        # Prepare the action for execution
        action, result = _prepare_action_for_execution(self, action_name, context, **kwargs)
        if action is None:
            return result

        try:
            # Publish before-execution event
            self.event_bus.publish(
                f"action.before_execute.{action_name}", {"action": action, "action_name": action_name, "manager": self}
            )

            # Process action (using middleware or directly)
            if self.middleware:
                result = self.middleware.process(action)
            else:
                result = action.process()

            # Publish after-execution event
            self.event_bus.publish(
                f"action.after_execute.{action_name}",
                {"action": action, "action_name": action_name, "result": result, "manager": self},
            )

            # Ensure result message is not empty
            if not result.message:
                result.message = f"Action {action_name} executed successfully"

            return result

        except Exception as e:
            # Handle exception
            error_message = str(e)
            tb = traceback.format_exc()
            logger.error(f"Error calling action {action_name}: {error_message}")
            logger.debug(tb)

            # Publish error event
            self.event_bus.publish(
                f"action.error.{action_name}",
                {
                    "action": action,
                    "action_name": action_name,
                    "error": e,
                    "traceback": tb,
                    "manager": self,
                },
            )

            return ActionResultModel(
                success=False,
                message=f"Action {action_name} execution failed: {error_message}",
                error=error_message,
                prompt="Please check the input parameters and try again",
                context={"traceback": tb, "action_name": action_name},
            )

    @error_handler
    async def call_action_async(
        self, action_name: str, context: Optional[Dict[str, Any]] = None, **kwargs
    ) -> ActionResultModel:
        """Call an action by name asynchronously.

        This method creates an Action instance, sets it up with the provided parameters,
        and processes it asynchronously. It also handles event publishing and middleware processing.
        It is useful for long-running operations or when integrating with asynchronous frameworks.

        The execution flow is as follows:
        1. Get the Action class from the registry
        2. Create an Action instance with the merged context
        3. Set up the Action with the provided parameters
        4. Publish a before-execution event asynchronously
        5. Process the Action asynchronously (using middleware if available)
        6. Publish an after-execution event asynchronously
        7. Return the result

        Args:
            action_name: Name of the action to call
            context: Optional dictionary of context data and dependencies
            **kwargs: Arguments to pass to the action

        Returns:
            ActionResultModel: Result of the action execution

        """
        # Prepare the action for execution
        action, result = _prepare_action_for_execution(self, action_name, context, **kwargs)
        if action is None:
            return result

        try:
            # Publish before-execution event asynchronously
            await self.event_bus.publish_async(
                f"action.before_execute.{action_name}", {"action": action, "action_name": action_name, "manager": self}
            )

            # Process action asynchronously (using middleware or directly)
            if self.middleware:
                result = await self.middleware.process_async(action)
            else:
                result = await action.process_async()

            # Publish after-execution event asynchronously
            await self.event_bus.publish_async(
                f"action.after_execute.{action_name}",
                {"action": action, "action_name": action_name, "result": result, "manager": self},
            )

            # Ensure result message is not empty
            if not result.message:
                result.message = f"Action {action_name} executed successfully"

            return result

        except Exception as e:
            # Handle exception
            error_message = str(e)
            tb = traceback.format_exc()
            logger.error(f"Error calling action {action_name} asynchronously: {error_message}")
            logger.debug(tb)

            # Publish error event asynchronously
            await self.event_bus.publish_async(
                f"action.error.{action_name}",
                {
                    "action": action,
                    "action_name": action_name,
                    "error": e,
                    "traceback": tb,
                    "manager": self,
                },
            )

            return ActionResultModel(
                success=False,
                message=f"Action {action_name} async execution failed: {error_message}",
                error=error_message,
                prompt="Please check the input parameters and try again",
                context={"traceback": tb, "action_name": action_name, "async": True},
            )

    def _update_context(self, new_context: Dict[str, Any]) -> None:
        """Update the context with new values.

        This method updates the context with new values, preserving existing values
        that are not overwritten by the new context.

        Args:
            new_context: Dictionary of new context values

        """
        self.context.update(new_context)
        logger.debug(f"Updated context for {self.name} manager with {len(new_context)} keys")

    def _merge_context(self, context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Merge provided context with default context.

        This method combines the default context with any user-provided context,
        giving precedence to user-provided values in case of conflicts.

        Args:
            context: Optional user-provided context dictionary

        Returns:
            Dict[str, Any]: Merged context dictionary

        Example:
            >>> manager = ActionManager("default", "maya")
            >>> merged = manager._merge_context({"user_data": "value"})
            >>> # merged will contain both the default context and {"user_data": "value"}

        """
        # If no context is provided, return a copy of the default context
        if not context:
            return self.context.copy()

        # Create a new dictionary containing the default context and user-provided context
        merged = {**self.context, **context}

        logger.debug(f"Merged context for {self.name} manager with {len(context)} user-provided keys")
        return merged

    def configure_middleware(self) -> MiddlewareChain:
        """Configure middleware for this action manager.

        This method returns the middleware chain for this action manager,
        which can be used to add middleware to the chain.

        Returns:
            MiddlewareChain: Middleware chain for this action manager

        """
        return self.middleware_chain

    def build_middleware(self) -> None:
        """Build the middleware chain.

        This method builds the middleware chain from the configured middleware.
        It should be called after adding middleware to the chain.
        """
        self.middleware = self.middleware_chain.build()
        logger.debug(f"Built middleware chain for {self.name} action manager")

    def add_middleware(self, middleware_class: Type[Middleware], **kwargs) -> "ActionManager":
        """Add a middleware to the chain.

        This is a convenience method that adds a middleware to the chain and builds it.
        It returns self to allow for method chaining.

        Args:
            middleware_class: Middleware class to add
            **kwargs: Additional arguments for the middleware constructor

        Returns:
            ActionManager: Returns self for method chaining

        Example:
            >>> manager = ActionManager("default", "maya")
            >>> manager.add_middleware(LoggingMiddleware).add_middleware(ValidationMiddleware)

        """
        logger.debug(f"Adding middleware {middleware_class.__name__} to {self.name} action manager")
        self.middleware_chain.add(middleware_class, **kwargs)
        self.build_middleware()
        return self

    def get_actions_info(self) -> ActionResultModel:
        """Get information about all actions.

        This method retrieves information about all actions registered for this DCC
        and returns it as an ActionResultModel.

        Returns:
            ActionResultModel: Contains information about all actions

        """
        # Get all action metadata
        registry_actions = self.registry.list_actions(dcc_name=self.dcc_name)

        # Create action information dictionary
        actions_info = {}
        for action_info in registry_actions:
            action_name = action_info["name"]
            actions_info[action_name] = {
                "name": action_info["name"],
                "internal_name": action_info["internal_name"],
                "description": action_info["description"],
                "tags": action_info["tags"],
                "dcc": action_info["dcc"],
                "version": action_info.get("version", "1.0.0"),
                "has_input_schema": bool(action_info.get("input_schema", {}).get("properties")),
                "has_output_schema": bool(action_info.get("output_schema", {}).get("properties")),
            }

        # Return result
        return ActionResultModel(
            success=True,
            message=f"Found {len(actions_info)} actions for {self.dcc_name}",
            prompt="You can call any of these actions using the call_action method",
            context={"dcc_name": self.dcc_name, "actions": actions_info, "count": len(actions_info)},
        )

    def _get_action_class(self, action_name: str) -> Optional[Type[Action]]:
        """Get an Action class by name.

        This is a helper method that retrieves an Action class from the registry.

        Args:
            action_name: Name of the action to get

        Returns:
            Optional[Type[Action]]: The Action class or None if not found

        """
        # First try to get the action with the DCC name
        action_class = self.registry.get_action(action_name, self.dcc_name)
        if action_class:
            return action_class

        # If not found, try without DCC name
        return self.registry.get_action(action_name)

    def _create_action_instance(self, action_class: Type[Action]) -> Action:
        """Create an instance of an Action class.

        This is a helper method that creates an instance of an Action class
        with the current context.

        Args:
            action_class: The Action class to instantiate

        Returns:
            Action: An instance of the Action class

        """
        return action_class(context=self.context.copy())

    def _check_auto_refresh(self) -> None:
        """Check if actions need to be refreshed based on auto-refresh settings.

        This method checks if auto-refresh is enabled and if the refresh interval
        has passed since the last refresh. If both conditions are met, it refreshes
        the actions.
        """
        if not self._auto_refresh or not self._refresh_interval:
            return

        current_time = time.time()
        if not self._last_refresh_time or (current_time - self._last_refresh_time >= self._refresh_interval):
            logger.debug(f"Auto-refreshing actions for {self.name} manager")
            self.refresh_actions()

    def list_available_actions(self) -> List[str]:
        """List all available actions for this DCC.

        This method returns a list of action names that are registered for this DCC.

        Returns:
            List[str]: A list of available action names

        """
        # Get action names directly from registry
        return self.registry.list_actions_for_dcc(self.dcc_name)


# Cache for action managers
_action_managers: Dict[str, ActionManager] = {}
_action_managers_lock = threading.RLock()


def create_action_manager(
    dcc_name: str,
    name: str = "default",
    auto_refresh: bool = True,
    refresh_interval: int = 60,
    context: Optional[Dict[str, Any]] = None,
    load_env_paths: bool = True,
    registry: Optional[ActionRegistry] = None,
) -> ActionManager:
    """Create an action manager for a specific DCC.

    This function creates a new ActionManager instance for the specified DCC.
    It also sets up auto-refresh and loads action paths from environment variables
    if requested.

    Args:
        dcc_name: Name of the DCC to create an action manager for
        name: Name for the action manager instance (default: "default")
        auto_refresh: Whether to enable automatic refresh of actions
        refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
        context: Optional dictionary of context data to inject into action modules
        load_env_paths: Whether to load action paths from environment variables
        registry: Optional ActionRegistry instance to use (creates a new one if not provided)

    Returns:
        ActionManager: A new action manager instance for the specified DCC

    Example:
        >>> manager = create_action_manager("maya")
        >>> manager.discover_actions_from_package("my_package")

    """
    logger.info(f"Creating new ActionManager '{name}' for DCC '{dcc_name}'...")

    # Create new ActionManager instance
    manager = ActionManager(
        name=name,
        dcc_name=dcc_name,
        context=context,
        auto_refresh=auto_refresh,
        refresh_interval=refresh_interval,
        registry=registry,
    )

    # Load action paths from environment variables
    if load_env_paths:
        action_paths_env = os.environ.get("DCC_MCP_ACTION_PATHS", "")
        if action_paths_env:
            action_paths = action_paths_env.split(os.pathsep)  # Use system path separator
            for path in action_paths:
                if path and os.path.exists(path):
                    logger.debug(f"Loading actions from environment path: {path}")
                    manager.discover_actions_from_path(path)

    return manager


def get_action_manager(
    dcc_name: str,
    name: str = "default",
    auto_refresh: bool = True,
    refresh_interval: int = 60,
    context: Optional[Dict[str, Any]] = None,
    load_env_paths: bool = True,
    registry: Optional[ActionRegistry] = None,
    force_new: bool = False,
) -> ActionManager:
    """Get an action manager for a specific DCC.

    This function returns an existing ActionManager instance for the specified DCC
    if one exists, or creates a new one if it doesn't. This is useful for getting
    a shared instance of an ActionManager across different parts of your application.

    The managers are cached in a dictionary keyed by a combination of DCC name and
    manager name, so you can have multiple managers for the same DCC with different
    names.

    Args:
        dcc_name: Name of the DCC to get an action manager for
        name: Name for the action manager instance (default: "default")
        auto_refresh: Whether to enable automatic refresh of actions
        refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
        context: Optional dictionary of context data to inject into action modules
        load_env_paths: Whether to load action paths from environment variables
        registry: Optional ActionRegistry instance to use (creates a new one if not provided)
        force_new: If True, always creates a new instance even if one exists in the cache

    Returns:
        ActionManager: An action manager instance for the specified DCC

    Example:
        >>> manager = get_action_manager("maya")
        >>> # This will return the same instance if called again
        >>> manager2 = get_action_manager("maya")
        >>> assert manager is manager2
        >>> # Force a new instance
        >>> manager3 = get_action_manager("maya", force_new=True)
        >>> assert manager is not manager3

    """
    # Create cache key
    cache_key = f"{dcc_name}:{name}"

    with _action_managers_lock:
        # Check if cache exists and we're not forcing a new instance
        if not force_new and cache_key in _action_managers:
            logger.debug(f"Returning cached ActionManager '{name}' for DCC '{dcc_name}'...")
            return _action_managers[cache_key]

        # Create new ActionManager instance
        manager = create_action_manager(
            dcc_name=dcc_name,
            name=name,
            auto_refresh=auto_refresh,
            refresh_interval=refresh_interval,
            context=context,
            load_env_paths=load_env_paths,
            registry=registry,
        )

        # Add new instance to cache (unless force_new is True)
        if not force_new:
            _action_managers[cache_key] = manager
            logger.info(f"Cached ActionManager '{name}' for DCC '{dcc_name}'...")

        return manager


# Helper functions for preparing actions and handling results
def _prepare_action_for_execution(manager, action_name, context=None, **kwargs):
    """Prepare an action for execution.

    This helper function prepares an action for execution by getting the action class,
    creating an instance, and setting it up with the provided parameters.

    Args:
        manager: The ActionManager instance
        action_name: Name of the action to call
        context: Optional dictionary of context data and dependencies
        **kwargs: Arguments to pass to the action

    Returns:
        tuple: (action, None) or (None, result) if action not found

    """
    # Check if auto-refresh is enabled and needed
    if manager._auto_refresh:
        manager._check_auto_refresh()

    # Get Action class from registry
    action_class = manager._get_action_class(action_name)

    if action_class is None:
        logger.warning(f"Action {action_name} not found in registry")
        result = ActionResultModel(
            success=False,
            message=f"Action {action_name} not found",
            error=f"Action {action_name} not found in registry",
            prompt="Please check the action name or register the action first",
        )
        return None, result

    try:
        # Use merged context to create Action instance
        merged_context = manager._merge_context(context)
        action = action_class(context=merged_context)

        # Set up the Action with the provided parameters
        action.setup(**kwargs)

        return action, None
    except Exception as e:
        # Handle exception
        error_message = str(e)
        tb = traceback.format_exc()
        logger.error(f"Error preparing action {action_name}: {error_message}")
        logger.debug(tb)

        # Create error result
        result = ActionResultModel(
            success=False,
            message=f"Error preparing action {action_name}",
            error=error_message,
            prompt="Please check the input parameters and try again",
            context={"traceback": tb},
        )
        return None, result
