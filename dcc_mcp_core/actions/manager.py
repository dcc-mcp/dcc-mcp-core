"""Action manager module for DCC-MCP-Core.

This module provides functionality for discovering, loading, and managing actions
for various Digital Content Creation (DCC) applications. It includes utilities for
registering action paths, creating action managers, and calling action functions.

The ActionManager class is responsible for managing Action classes, which represent
operations that can be performed in DCC applications.
"""

# Import built-in modules
import asyncio
from concurrent.futures import ThreadPoolExecutor
import logging
import os
import threading
import time
import traceback
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.events import event_bus
from dcc_mcp_core.actions.middleware import Middleware
from dcc_mcp_core.actions.middleware import MiddlewareChain
from dcc_mcp_core.actions.registry import ActionRegistry
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils.decorators import error_handler
from dcc_mcp_core.utils.filesystem import get_actions_paths_from_env
from dcc_mcp_core.utils.module_loader import append_to_python_path

logger = logging.getLogger(__name__)


class ActionManager:
    """Manager for DCC actions.

    This class provides functionality for discovering, loading, and managing actions
    for different DCCs in the DCC-MCP ecosystem. Actions represent operations that can be
    performed in a DCC application and are exposed to AI for execution.

    Attributes:
        dcc_name: Name of the DCC this action manager is for

    """

    def __init__(
        self,
        dcc_name: str,
        auto_refresh: bool = True,
        refresh_interval: int = 60,
        context: Optional[Dict[str, Any]] = None,
        cache_ttl: int = 120,
        load_env_paths: bool = True,
    ):
        """Initialize a new ActionManager instance.

        Args:
            dcc_name: Name of the DCC this action manager is for
            auto_refresh: Whether to enable automatic refresh of actions
            refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
            context: Optional dictionary of context data to inject into action modules
            cache_ttl: Cache TTL in seconds
            load_env_paths: Whether to load action paths from environment variables

        """
        self.dcc_name = dcc_name
        self.auto_refresh = auto_refresh
        self.refresh_interval = refresh_interval
        self.context = context or {}
        self.cache_ttl = cache_ttl

        # Initialize registry
        self.registry = ActionRegistry()

        # Initialize middleware chain
        self.middleware_chain = MiddlewareChain()
        self.middleware = None

        # Initialize event bus
        self.event_bus = event_bus

        # Action paths and info
        self._action_paths: List[str] = []
        if load_env_paths:
            self._action_paths = get_actions_paths_from_env(dcc_name)
        self._last_refresh = 0
        self._refresh_lock = threading.RLock()
        self._refresh_thread = None
        self._stop_refresh = threading.Event()

        # Start auto refresh if enabled
        if auto_refresh:
            self._start_auto_refresh()

    def register_action_path(self, path: str) -> None:
        """Register a path to discover actions from.

        Args:
            path: Path to discover actions from

        """
        if path not in self._action_paths:
            self._action_paths.append(path)

    def refresh_actions(self, force: bool = False) -> None:
        """Refresh actions from registered paths.

        Args:
            force: Whether to force a refresh even if the cache is still valid

        """
        # Check if refresh is needed
        current_time = time.time()
        if not force and current_time - self._last_refresh < self.cache_ttl:
            return

        with self._refresh_lock:
            # Discover actions from registered paths
            for path in self._action_paths:
                self._discover_actions_from_path(path)

            # Update last refresh time
            self._last_refresh = current_time

    def _discover_actions_from_path(self, path: str) -> None:
        """Discover actions from a path.

        Args:
            path: Path to discover actions from

        """
        # Find Python files in the path
        action_files = []
        for root, _, files in os.walk(path):
            for file in files:
                if file.endswith(".py") and not file.startswith("__"):
                    action_files.append(os.path.join(root, file))

        # Load action modules asynchronously
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            tasks = [self._load_action_module(file_path) for file_path in action_files]
            loop.run_until_complete(asyncio.gather(*tasks))
        finally:
            loop.close()

    def _discover_actions_from_path_sync(self, path: str) -> None:
        """Discover actions from a path synchronously (for testing).

        Args:
            path: Path to discover actions from

        """
        # Find Python files in the path
        action_files = []
        for root, _, files in os.walk(path):
            for file in files:
                if file.endswith(".py") and not file.startswith("__"):
                    action_files.append(os.path.join(root, file))

        # Load action modules synchronously
        for file_path in action_files:
            try:
                # Add the directory to Python path
                dir_path = os.path.dirname(file_path)
                append_to_python_path(dir_path)

                # Load the module
                module_name = os.path.basename(file_path).replace(".py", "")
                module_path = os.path.join(dir_path, module_name)
                self.registry.discover_actions_from_module(module_name, module_path, self.dcc_name)
            except Exception as e:
                logger.error(f"Error loading action module {file_path}: {e}")
                logger.debug(traceback.format_exc())

    async def _load_action_module(self, file_path: str) -> None:
        """Load an action module asynchronously.

        Args:
            file_path: Path to the action module file

        """
        # Use ThreadPoolExecutor to load modules in parallel
        with ThreadPoolExecutor() as executor:
            loop = asyncio.get_event_loop()
            await loop.run_in_executor(executor, self._load_action_module_sync, file_path)

    def _load_action_module_sync(self, file_path: str) -> None:
        """Load an action module synchronously.

        Args:
            file_path: Path to the action module file

        """
        try:
            with append_to_python_path(file_path):
                action_classes = self.registry.discover_actions_from_path(
                    path=file_path, context=self.context, dcc_name=self.dcc_name
                )

                if action_classes:
                    logger.info(f"Loaded {len(action_classes)} action classes from {file_path}")
        except Exception as e:
            logger.error(f"Error loading action module {file_path}: {e}")
            logger.debug(traceback.format_exc())

    def _start_auto_refresh(self) -> None:
        """Start automatic refresh of actions."""
        if self._refresh_thread is not None and self._refresh_thread.is_alive():
            return

        self._stop_refresh.clear()
        self._refresh_thread = threading.Thread(
            target=self._auto_refresh_worker,
            daemon=True,
            name=f"ActionManager-AutoRefresh-{self.dcc_name}",
        )
        self._refresh_thread.start()

    def _stop_auto_refresh(self) -> None:
        """Stop automatic refresh of actions."""
        if self._refresh_thread is None or not self._refresh_thread.is_alive():
            return

        self._stop_refresh.set()
        self._refresh_thread.join(timeout=1.0)
        self._refresh_thread = None

    def _auto_refresh_worker(self) -> None:
        """Worker function for automatic refresh of actions."""
        while not self._stop_refresh.is_set():
            try:
                self.refresh_actions()
            except Exception as e:
                logger.error(f"Error refreshing actions: {e}")
                logger.debug(traceback.format_exc())

            # Sleep until next refresh
            self._stop_refresh.wait(self.refresh_interval)

    @error_handler
    def call_action(self, action_name: str, context: Optional[Dict[str, Any]] = None, **kwargs) -> ActionResultModel:
        """Call an action by name.

        Args:
            action_name: Name of the action to call
            context: Optional dictionary of context data and dependencies
            **kwargs: Arguments to pass to the action

        Returns:
            Result of the action execution

        """
        # Get action class from registry
        action_class = self.registry.get_action(action_name)

        if action_class is None:
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} not found",
                error=f"Action {action_name} not found",
            )

        try:
            # Create action instance with context
            if context is None:
                context = self._get_default_context()

            action = action_class(context=context)

            # Setup action
            action.setup(**kwargs)

            # Publish action.before_execute event
            self.event_bus.publish(f"action.before_execute.{action_name}", action=action)

            # If middleware is available, use middleware to process action, otherwise process directly
            if self.middleware:
                result = self.middleware.process(action)
            else:
                result = action.process()

            # Publish action.after_execute event
            self.event_bus.publish(f"action.after_execute.{action_name}", action=action, result=result)

            # If result message is not set, add a default message
            if not result.message:
                result.message = f"Action {action_name} executed successfully"

            return result
        except Exception as e:
            error_message = str(e)
            tb = traceback.format_exc()
            logger.error(f"Error calling action {action_name}: {error_message}")
            logger.debug(tb)

            # Publish action.error event
            self.event_bus.publish(f"action.error.{action_name}", action=action, error=e, traceback=tb)

            return ActionResultModel(
                success=False,
                message=f"Action {action_name} execution failed: {error_message}",
                error=error_message,
                context={"traceback": tb},
            )

    @error_handler
    async def call_action_async(
        self, action_name: str, context: Optional[Dict[str, Any]] = None, **kwargs
    ) -> ActionResultModel:
        """Call an action by name asynchronously.

        This method allows actions to be executed in an asynchronous context, which is useful
        for long-running operations or when integrating with asynchronous frameworks.

        Args:
            action_name: Name of the action to call
            context: Optional dictionary of context data and dependencies
            **kwargs: Arguments to pass to the action

        Returns:
            Result of the action execution

        """
        # Get action class from registry
        action_class = self.registry.get_action(action_name)

        if action_class is None:
            return ActionResultModel(
                success=False,
                message=f"Action {action_name} not found",
                error=f"Action {action_name} not found",
            )

        try:
            # Create action instance with context
            if context is None:
                context = self._get_default_context()

            action = action_class(context=context)

            # Setup action
            action.setup(**kwargs)

            # Publish action.before_execute event
            await self.event_bus.publish_async(f"action.before_execute.{action_name}", action=action)

            # If middleware is available, use middleware to process action, otherwise process directly
            if self.middleware:
                result = await self.middleware.process_async(action)
            else:
                result = await action.process_async()

            # Publish action.after_execute event
            await self.event_bus.publish_async(f"action.after_execute.{action_name}", action=action, result=result)

            # If result message is not set, add a default message
            if not result.message:
                result.message = f"Action {action_name} executed successfully"

            return result
        except Exception as e:
            error_message = str(e)
            tb = traceback.format_exc()
            logger.error(f"Error calling action {action_name} asynchronously: {error_message}")
            logger.debug(tb)

            # Publish action.error event
            await self.event_bus.publish_async(f"action.error.{action_name}", action=action, error=e, traceback=tb)

            return ActionResultModel(
                success=False,
                message=f"Action {action_name} async execution failed: {error_message}",
                error=error_message,
                context={"traceback": tb},
            )

    def _get_default_context(self) -> Dict[str, Any]:
        """Get default context for actions.

        This method returns a dictionary of default context data that will be
        provided to actions if no specific context is provided.

        Returns:
            Dict[str, Any]: Default context data

        """
        return {
            "dcc_name": self.dcc_name,
            "manager": self,
            "event_bus": self.event_bus,
            # Add more default context data here
        }

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

    def add_middleware(self, middleware_class: Type[Middleware], **kwargs) -> "ActionManager":
        """Add a middleware to the chain.

        This is a convenience method that adds a middleware to the chain and builds it.

        Args:
            middleware_class: Middleware class to add
            **kwargs: Additional arguments for the middleware constructor

        Returns:
            self: Returns self for method chaining

        """
        self.middleware_chain.add(middleware_class, **kwargs)
        self.build_middleware()
        return self

    def get_actions_info(self) -> ActionResultModel:
        """Get information about all actions.

        Returns:
            ActionResultModel containing information about all actions

        """
        # Refresh actions if needed
        self.refresh_actions()

        # Get all actions from registry
        registry_actions = self.registry.list_actions(dcc_name=self.dcc_name)

        # Create a dictionary of action information
        actions_info = {}
        for action_info in registry_actions:
            action_name = action_info["name"]
            action_class = self.registry.get_action(action_name)

            if action_class:
                # Extract information from the Action class
                actions_info[action_name] = {
                    "name": action_class.name,
                    "description": action_class.description,
                    "tags": action_class.tags,
                    "dcc": action_class.dcc,
                    "order": action_class.order,
                }

        # Return result as ActionResultModel
        return ActionResultModel(
            success=True,
            message=f"Actions info retrieved for {self.dcc_name}",
            context={"dcc_name": self.dcc_name, "actions": actions_info},
        )

    def list_available_actions(self) -> List[str]:
        """List all available actions for this DCC.

        Returns:
            A list of available action names

        """
        # Ensure actions are refreshed
        self.refresh_actions()

        # Get all actions from the registry for this DCC
        return self.registry.list_actions_for_dcc(self.dcc_name)


# Cache for action managers
_action_managers: Dict[str, ActionManager] = {}
_action_managers_lock = threading.RLock()


def create_action_manager(
    dcc_name: str,
    auto_refresh: bool = True,
    refresh_interval: int = 60,
    context: Optional[Dict[str, Any]] = None,
    cache_ttl: int = 120,
    load_env_paths: bool = True,
) -> ActionManager:
    """Create an action manager for a specific DCC.

    Args:
        dcc_name: Name of the DCC to create an action manager for
        auto_refresh: Whether to enable automatic refresh of actions
        refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
        context: Optional dictionary of context data to inject into action modules
        cache_ttl: Cache TTL in seconds
        load_env_paths: Whether to load action paths from environment variables

    Returns:
        An action manager instance for the specified DCC

    """
    with _action_managers_lock:
        if dcc_name in _action_managers:
            return _action_managers[dcc_name]

        manager = ActionManager(
            dcc_name=dcc_name,
            auto_refresh=auto_refresh,
            refresh_interval=refresh_interval,
            context=context,
            cache_ttl=cache_ttl,
            load_env_paths=load_env_paths,
        )

        _action_managers[dcc_name] = manager
        return manager


def get_action_manager(
    dcc_name: str,
    auto_refresh: bool = True,
    refresh_interval: int = 60,
    context: Optional[Dict[str, Any]] = None,
    load_env_paths: bool = True,
) -> ActionManager:
    """Get an action manager for a specific DCC.

    If an action manager for the specified DCC already exists, it will be returned.
    Otherwise, a new action manager will be created.

    Args:
        dcc_name: Name of the DCC to get an action manager for
        auto_refresh: Whether to enable automatic refresh of actions
        refresh_interval: Refresh interval in seconds (only used if auto_refresh is True)
        context: Optional dictionary of context data to inject into action modules
        load_env_paths: Whether to load action paths from environment variables

    Returns:
        An action manager instance for the specified DCC

    """
    with _action_managers_lock:
        if dcc_name in _action_managers:
            return _action_managers[dcc_name]

        return create_action_manager(
            dcc_name=dcc_name,
            auto_refresh=auto_refresh,
            refresh_interval=refresh_interval,
            context=context,
            load_env_paths=load_env_paths,
        )
