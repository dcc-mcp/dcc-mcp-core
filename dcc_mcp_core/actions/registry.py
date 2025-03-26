"""Action registry for DCC-MCP-Core.

This module provides the ActionRegistry class for registering and discovering Action classes.
"""

# Import built-in modules
import importlib
import inspect
import logging
from pathlib import Path
import pkgutil
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.utils.module_loader import load_module_from_path


class ActionRegistry:
    """Registry for Action classes.

    This class provides functionality for registering, discovering, and retrieving
    Action classes. It follows the singleton pattern to ensure a single registry
    instance is used throughout the application.
    """

    _instance = None

    def __new__(cls):
        """Ensure only one instance of ActionRegistry exists (Singleton pattern)."""
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._actions = {}
        return cls._instance

    def register(self, action_class: Type[Action]) -> None:
        """Register an Action class.

        Args:
            action_class: The Action subclass to register

        Raises:
            TypeError: If action_class is not a subclass of Action

        """
        if not issubclass(action_class, Action):
            raise TypeError(f"{action_class.__name__} must be a subclass of Action")

        name = action_class.name or action_class.__name__
        self._actions[name] = action_class

    def get_action(self, name: str) -> Optional[Type[Action]]:
        """Get an Action class by name.

        Args:
            name: Name of the Action

        Returns:
            Optional[Type[Action]]: The Action class or None if not found

        """
        return self._actions.get(name)

    def list_actions(self, dcc_name: Optional[str] = None) -> List[Dict[str, Any]]:
        """List all registered Actions and their metadata.

        Args:
            dcc_name: Optional DCC name to filter actions

        Returns:
            List[Dict[str, Any]]: List of action metadata dictionaries

        """
        result = []
        for name, action_class in self._actions.items():
            # Filter by DCC name if specified
            if dcc_name and action_class.dcc != dcc_name:
                continue

            # Extract input schema
            input_schema = action_class.InputModel.model_json_schema()

            result.append(
                {
                    "name": name,
                    "description": action_class.description,
                    "tags": action_class.tags,
                    "dcc": action_class.dcc,
                    "input_schema": input_schema,
                }
            )
        return result

    def discover_actions(self, package_name: str) -> None:
        """Discover and register Action classes from a package.

        This method recursively searches through a package and its subpackages
        for Action subclasses and registers them.

        Args:
            package_name: Name of the package to search

        """
        try:
            package = importlib.import_module(package_name)
            package_path = Path(package.__file__).parent

            for _, module_name, is_pkg in pkgutil.iter_modules([str(package_path)]):
                if is_pkg:
                    # Recursively process subpackages
                    self.discover_actions(f"{package_name}.{module_name}")
                else:
                    # Import module and find Action subclasses
                    try:
                        module = importlib.import_module(f"{package_name}.{module_name}")

                        for name, obj in inspect.getmembers(module):
                            if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                                self.register(obj)
                    except (ImportError, AttributeError) as e:
                        # Log error but continue processing other modules
                        logging.getLogger(__name__).warning(f"Error importing module {module_name}: {e}")
        except ImportError as e:
            logging.getLogger(__name__).warning(f"Error importing package {package_name}: {e}")

    def discover_actions_from_path(
        self, path: str, dependencies: Optional[Dict[str, Any]] = None, dcc_name: Optional[str] = None
    ) -> List[Type[Action]]:
        """Discover and register Action classes from a file path.

        This method loads a Python module from a file path and registers any Action
        subclasses found in the module.

        Args:
            path: Path to the Python file to load
            dependencies: Optional dictionary of dependencies to inject into the module
            dcc_name: Optional DCC name to inject DCC-specific dependencies

        Returns:
            List of discovered and registered Action classes

        """
        discovered_actions = []
        try:
            module = load_module_from_path(path, dependencies=dependencies, dcc_name=dcc_name)

            # Find and register Action subclasses
            for name, obj in inspect.getmembers(module):
                if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                    # Set DCC name if not already set and dcc_name is provided
                    if dcc_name and not obj.dcc:
                        obj.dcc = dcc_name

                    # Register the action class
                    self.register(obj)
                    discovered_actions.append(obj)
        except (ImportError, AttributeError) as e:
            # Log error but continue processing
            logging.getLogger(__name__).warning(f"Error discovering actions from {path}: {e}")
        return discovered_actions
