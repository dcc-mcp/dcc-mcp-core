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
    _logger = logging.getLogger(__name__)

    def __new__(cls):
        """Ensure only one instance of ActionRegistry exists (Singleton pattern)."""
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._actions = {}
            cls._instance._dcc_actions = {}
            cls._logger.debug("Created new ActionRegistry instance")
        return cls._instance

    @classmethod
    def _reset_instance(cls):
        """Reset the singleton instance.

        This method is primarily used for testing purposes.
        """
        cls._instance = None
        cls._logger.debug("Reset ActionRegistry singleton instance")

    def register(self, action_class: Type[Action]) -> None:
        """Register an Action class.

        Args:
            action_class: The Action subclass to register

        Raises:
            TypeError: If action_class is not a subclass of Action

        """
        if not issubclass(action_class, Action):
            raise TypeError(f"{action_class.__name__} must be a subclass of Action")

        # Get action name and DCC type
        name = action_class.name or action_class.__name__
        dcc = action_class.dcc

        # Check if source file information is available
        source_file = getattr(action_class, "_source_file", None)

        # Create a unique key for the action if source file is available
        # This allows multiple actions with the same name from different files
        if source_file:
            # Create a unique identifier based on the class name and source file
            # Use only the filename part, not the full path
            # Import built-in modules
            import os

            filename = os.path.basename(source_file)
            unique_name = f"{name}@{filename}"

            # Store the original name for reference
            setattr(action_class, "_original_name", name)

            # Use the unique name for registration
            name = unique_name

            self._logger.debug(f"Using unique name '{name}' for action from {source_file}")

        # Register in the main registry
        self._actions[name] = action_class

        # Register in the DCC-specific registry
        if dcc not in self._dcc_actions:
            self._dcc_actions[dcc] = {}
        self._dcc_actions[dcc][name] = action_class

        self._logger.debug(f"Registered action '{name}' for DCC '{dcc}' from {source_file or 'unknown source'}")

    def get_action(self, name: str, dcc_name: Optional[str] = None) -> Optional[Type[Action]]:
        """Get an Action class by name.

        Args:
            name: Name of the Action
            dcc_name: Optional DCC name to get a DCC-specific action

        Returns:
            Optional[Type[Action]]: The Action class or None if not found

        """
        # First, try to get the action directly by name
        action = None

        if dcc_name:
            # If DCC name is specified
            if dcc_name in self._dcc_actions:
                # Look in that DCC's registry
                action = self._dcc_actions[dcc_name].get(name)
            else:
                # If the specified DCC doesn't exist, fall back to main registry
                action = self._actions.get(name)
        else:
            # If no DCC specified, look in main registry
            action = self._actions.get(name)

        # If action is found, return it
        if action:
            return action

        # If action is not found, try to find it by original name
        # This handles the case where the action was registered with a unique name
        if dcc_name and dcc_name in self._dcc_actions:
            # Search through DCC-specific actions
            for action_name, action_class in self._dcc_actions[dcc_name].items():
                original_name = getattr(action_class, "_original_name", None)
                if original_name == name:
                    self._logger.debug(f"Found action '{name}' by original name, registered as '{action_name}'")
                    return action_class
        else:
            # Search through all actions
            for action_name, action_class in self._actions.items():
                original_name = getattr(action_class, "_original_name", None)
                if original_name == name:
                    self._logger.debug(f"Found action '{name}' by original name, registered as '{action_name}'")
                    return action_class

        # If still not found, return None
        return None

    def list_actions(self, dcc_name: Optional[str] = None) -> List[Dict[str, Any]]:
        """List all registered Actions and their metadata.

        Args:
            dcc_name: Optional DCC name to filter actions

        Returns:
            List[Dict[str, Any]]: List of action metadata dictionaries

        """
        result = []

        if dcc_name and dcc_name in self._dcc_actions:
            # List only actions for the specified DCC
            actions_to_list = self._dcc_actions[dcc_name].items()
        else:
            # List all actions
            actions_to_list = self._actions.items()

        for name, action_class in actions_to_list:
            # Skip if we're filtering by DCC name and this action is for a different DCC
            if dcc_name and action_class.dcc != dcc_name:
                continue

            # Do not generate JSON Schema, instead return a simplified model description
            # This avoids Pydantic's issue with UUID type handling in JSON Schema generation

            # Extract input model fields
            input_schema = {"title": "InputModel", "type": "object", "properties": {}}
            try:
                if hasattr(action_class, "InputModel") and action_class.InputModel:
                    # Get model fields
                    for field_name, field_info in action_class.InputModel.model_fields.items():
                        input_schema["properties"][field_name] = {
                            "title": field_name,
                            "type": "string",
                            "description": field_info.description or "",
                        }
            except Exception as e:
                self._logger.warning(f"Error extracting input model fields for {name}: {e}")

            # Extract output model fields
            output_schema = None
            try:
                if hasattr(action_class, "OutputModel") and action_class.OutputModel:
                    output_schema = {"title": "OutputModel", "type": "object", "properties": {}}
                    # Get model fields
                    for field_name, field_info in action_class.OutputModel.model_fields.items():
                        output_schema["properties"][field_name] = {
                            "title": field_name,
                            "type": "string",
                            "description": field_info.description or "",
                        }
            except Exception as e:
                self._logger.warning(f"Error extracting output model fields for {name}: {e}")

            # Get original name if available (for uniquely named actions)
            display_name = getattr(action_class, "_original_name", name)

            # Get source file if available
            source_file = getattr(action_class, "_source_file", None)

            result.append(
                {
                    "name": display_name,  # Use display name for user-facing information
                    "internal_name": name,  # Include internal name for reference
                    "description": action_class.description,
                    "tags": action_class.tags,
                    "dcc": action_class.dcc,
                    "input_schema": input_schema,
                    "output_schema": output_schema,
                    "version": getattr(action_class, "version", "1.0.0"),
                    "author": getattr(action_class, "author", None),
                    "examples": getattr(action_class, "examples", None),
                    "source_file": source_file,  # Include source file for reference
                }
            )
        return result

    def list_actions_for_dcc(self, dcc_name: str) -> List[str]:
        """List all action names for a specific DCC.

        Args:
            dcc_name: Name of the DCC to list actions for

        Returns:
            A list of action names for the specified DCC

        """
        if dcc_name not in self._dcc_actions:
            return []

        return list(self._dcc_actions[dcc_name].keys())

    def discover_actions(self, package_name: str, dcc_name: Optional[str] = None) -> List[Type[Action]]:
        """Discover and register Action classes from a package.

        This method recursively searches through a package and its subpackages
        for Action subclasses and registers them.

        Args:
            package_name: Name of the package to search
            dcc_name: Optional DCC name to set for discovered actions

        Returns:
            List of discovered and registered Action classes

        """
        discovered_actions = []
        try:
            package = importlib.import_module(package_name)
            package_path = Path(package.__file__).parent

            for _, module_name, is_pkg in pkgutil.iter_modules([str(package_path)]):
                if is_pkg:
                    # Recursively process subpackages
                    discovered_actions.extend(self.discover_actions(f"{package_name}.{module_name}", dcc_name))
                else:
                    # Import module and find Action subclasses
                    try:
                        module = importlib.import_module(f"{package_name}.{module_name}")

                        for name, obj in inspect.getmembers(module):
                            if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                                # Set DCC name if provided and not already set
                                if dcc_name and not obj.dcc:
                                    obj.dcc = dcc_name

                                self.register(obj)
                                discovered_actions.append(obj)
                                self._logger.debug(f"Discovered action '{obj.__name__}' in module '{module_name}'")
                    except (ImportError, AttributeError) as e:
                        # Log error but continue processing other modules
                        self._logger.warning(f"Error importing module {module_name}: {e}")
        except ImportError as e:
            self._logger.warning(f"Error importing package {package_name}: {e}")

        return discovered_actions

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

                    # Set source file path for the action class
                    setattr(obj, "_source_file", path)

                    # Register the action class
                    self.register(obj)
                    discovered_actions.append(obj)
                    self._logger.debug(f"Discovered action '{obj.__name__}' from path '{path}')")
        except (ImportError, AttributeError) as e:
            # Log error but continue processing
            self._logger.warning(f"Error discovering actions from {path}: {e}")
        return discovered_actions

    def get_actions_by_dcc(self, dcc_name: str) -> Dict[str, Type[Action]]:
        """Get all actions for a specific DCC.

        Args:
            dcc_name: Name of the DCC

        Returns:
            Dict[str, Type[Action]]: Dictionary of action name to action class

        """
        if dcc_name in self._dcc_actions:
            return self._dcc_actions[dcc_name]
        return {}

    def get_all_dccs(self) -> List[str]:
        """Get a list of all DCCs that have registered actions.

        Returns:
            List[str]: List of DCC names

        """
        return list(self._dcc_actions.keys())

    def reset(self):
        """Reset the registry to its initial state.

        This method is primarily used for testing purposes.
        """
        self._actions.clear()
        self._dcc_actions.clear()
        self._logger.debug("Reset ActionRegistry instance")
