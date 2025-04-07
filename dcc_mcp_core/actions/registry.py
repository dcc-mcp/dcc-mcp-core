"""Action registry for DCC-MCP-Core.

This module provides the ActionRegistry class for registering and discovering Action classes.
"""

# Import built-in modules
import importlib
import inspect
import logging
from pathlib import Path
from typing import Any
from typing import ClassVar
from typing import Dict
from typing import List
from typing import Optional
from typing import Set
from typing import Type

# Use importlib_metadata for compatibility with all Python versions
# This should be added to project dependencies
try:
    # Import third-party modules
    pass
except ImportError:
    # Fallback to built-in importlib.metadata in Python 3.8+
    pass

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.utils.module_loader import load_module_from_path


class ActionRegistry:
    """Registry for Action classes.

    This class provides functionality for registering, discovering, and retrieving
    Action classes. It follows the singleton pattern to ensure a single registry
    instance is used throughout the application.
    """

    _instance: ClassVar[Optional["ActionRegistry"]] = None
    _logger: ClassVar = logging.getLogger(__name__)
    # Action discovery hooks.
    _action_discovery_hooks: ClassVar[Dict[str, Any]] = {}

    def __new__(cls):
        """Ensure only one instance of ActionRegistry exists (Singleton pattern)."""
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            # Main registry: maps action name to action class
            cls._instance._actions = {}
            # DCC-specific registry: maps DCC name to a dict of {action_name: action_class}
            cls._instance._dcc_actions = {}
            # Cache of loaded modules to improve performance
            cls._instance._module_cache = {}
            cls._logger.debug("Created new ActionRegistry instance")
        return cls._instance

    @classmethod
    def _reset_instance(cls, full_reset=True):
        """Reset the ActionRegistry instance.

        This method is primarily used for testing purposes.
        It resets the registry state, including all registered actions,
        DCC-specific registries, and module cache.

        Args:
            full_reset: If True, completely resets the singleton instance.
                       If False, only clears the current instance data.

        """
        if cls._instance is not None:
            cls._instance._actions = {}
            cls._instance._dcc_actions = {}
            cls._instance._module_cache = {}
            cls._logger.debug("Cleared ActionRegistry instance state")

        if full_reset:
            cls._instance = None
            cls._logger.debug("Reset ActionRegistry singleton instance")

    @classmethod
    def reset(cls):
        """Reset the registry to its initial state.

        This method is primarily used for testing purposes.
        """
        cls._reset_instance(full_reset=False)

    def register(self, action_class: Type[Action]) -> None:
        """Register an Action class.

        This method registers an Action subclass in both the main registry and the
        DCC-specific registry. The action is indexed by its name in both registries.

        Args:
            action_class: The Action subclass to register

        Raises:
            TypeError: If action_class is not a subclass of Action
            ValueError: If action_class is abstract or does not implement _execute method

        """
        if not issubclass(action_class, Action):
            raise TypeError(f"{action_class.__name__} must be a subclass of Action")

        # Check if the action is abstract
        if getattr(action_class, "abstract", False):
            self._logger.debug(f"Skipping registration of abstract action class: {action_class.__name__}")
            return

        # Check if the action implements _execute method
        if not hasattr(action_class, "_execute") or action_class._execute is Action._execute:
            self._logger.debug(
                f"Skipping registration of action class without _execute implementation: {action_class.__name__}"
            )
            return

        # Get action name and DCC type
        name = action_class.name or action_class.__name__
        dcc = action_class.dcc

        # Register in the main registry
        self._actions[name] = action_class
        self._logger.debug(f"Registered action '{name}' in main registry")

        # Register in the DCC-specific registry
        if dcc not in self._dcc_actions:
            # Initialize DCC registry if it doesn't exist
            self._dcc_actions[dcc] = {}
            self._logger.debug(f"Created registry for DCC '{dcc}'")

        # Add the action to its DCC-specific registry
        # Use the action's name as the key
        self._dcc_actions[dcc][name] = action_class

        self._logger.debug(f"Registered action '{name}' in DCC-specific registry for '{dcc}'")

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

        # Determine the set of actions to list
        if dcc_name and dcc_name in self._dcc_actions:
            # Only list actions from the specified DCC
            actions_to_list = self._dcc_actions[dcc_name].items()
        else:
            # List all actions
            actions_to_list = self._actions.items()

        for name, action_class in actions_to_list:
            # If filtering by DCC name, skip actions that don't match
            if dcc_name and action_class.dcc != dcc_name:
                continue

            # Get original name (if available)
            display_name = getattr(action_class, "_original_name", name)
            # Get source file (if available)
            source_file = getattr(action_class, "_source_file", None)

            # Create action metadata
            action_metadata = {
                "name": display_name,  # Use display name for user interface
                "internal_name": name,  # Include internal name for reference
                "description": action_class.description,
                "tags": getattr(action_class, "tags", []),
                "dcc": action_class.dcc,
                "version": getattr(action_class, "version", "1.0.0"),
                "author": getattr(action_class, "author", None),
                "examples": getattr(action_class, "examples", None),
                "source_file": source_file,  # Include source file for reference
            }

            # Use Pydantic's model export functionality to get input model
            try:
                if hasattr(action_class, "InputModel") and action_class.InputModel:
                    # Use model_json_schema to get the complete JSON Schema
                    input_schema = action_class.InputModel.model_json_schema()
                    # Simplify schema to avoid UUID type handling issues
                    action_metadata["input_schema"] = self._simplify_schema(input_schema)
                else:
                    action_metadata["input_schema"] = {"title": "InputModel", "type": "object", "properties": {}}
            except Exception as e:
                self._logger.warning(f"Error extracting input schema for {name}: {e}")
                action_metadata["input_schema"] = {"title": "InputModel", "type": "object", "properties": {}}

            # Use Pydantic's model export functionality to get output model
            try:
                if hasattr(action_class, "OutputModel") and action_class.OutputModel:
                    # Use model_json_schema to get the complete JSON Schema
                    output_schema = action_class.OutputModel.model_json_schema()
                    # Simplify schema to avoid UUID type handling issues
                    action_metadata["output_schema"] = self._simplify_schema(output_schema)
                else:
                    action_metadata["output_schema"] = {"title": "OutputModel", "type": "object", "properties": {}}
            except Exception as e:
                self._logger.warning(f"Error extracting output schema for {name}: {e}")
                action_metadata["output_schema"] = {"title": "OutputModel", "type": "object", "properties": {}}

            result.append(action_metadata)

        return result

    def _simplify_schema(self, schema: Dict[str, Any]) -> Dict[str, Any]:
        """Simplify JSON Schema, removing unnecessary complexity.

        Args:
            schema: Original JSON Schema

        Returns:
            Dict[str, Any]: Simplified Schema

        """
        # Create basic structure
        simplified = {"title": schema.get("title", ""), "type": "object", "properties": {}}

        # Extract property information
        properties = schema.get("properties", {})
        for prop_name, prop_info in properties.items():
            # Skip internal fields
            if prop_name.startswith("_"):
                continue

            simplified["properties"][prop_name] = {
                "type": prop_info.get("type", "string"),
                "description": prop_info.get("description", ""),
            }

            # Handle enum type
            if "enum" in prop_info:
                simplified["properties"][prop_name]["enum"] = prop_info["enum"]

            # Handle default value
            if "default" in prop_info:
                simplified["properties"][prop_name]["default"] = prop_info["default"]

        return simplified

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

    @classmethod
    def register_discovery_hook(cls, package_name: str, hook_func):
        """Register an Action discovery hook function.

        This method allows registering custom Action discovery logic for specific packages.
        This is useful for testing, plugin systems, or special package structures.

        Args:
            package_name: Package name, used as the hook key
            hook_func: Hook function, receiving registry and dcc_name parameters, returning Action class list

        """
        cls._action_discovery_hooks[package_name] = hook_func

    @classmethod
    def clear_discovery_hooks(cls):
        """Clear all Action discovery hooks."""
        cls._action_discovery_hooks.clear()

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
        # Check for custom Action discovery hook
        if package_name in self.__class__._action_discovery_hooks:
            self._logger.debug(f"Using custom discovery hook for package {package_name}")
            return self.__class__._action_discovery_hooks[package_name](self, dcc_name)

        # Standard package processing for non-test packages
        discovered_actions = []
        try:
            # Import the package
            package = importlib.import_module(package_name)
            package_path = Path(package.__file__).parent
            self._logger.debug(f"Discovering actions from package {package_name} at {package_path}")

            # Process actions in the package itself
            for name, obj in inspect.getmembers(package):
                if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                    if dcc_name and not obj.dcc:
                        obj.dcc = dcc_name
                    self.register(obj)
                    discovered_actions.append(obj)

            # Find all Python modules in the package
            modules_to_process = []
            for path in package_path.glob("**/*.py"):
                if "__pycache__" in str(path) or path.name == "__init__.py":
                    continue

                rel_path = path.relative_to(package_path)
                parts = list(rel_path.parent.parts)
                if parts:
                    module_name = ".".join([package_name, *parts, rel_path.stem])
                else:
                    module_name = f"{package_name}.{rel_path.stem}"

                modules_to_process.append(module_name)

            # Process each module
            for module_name in modules_to_process:
                try:
                    if module_name in self._module_cache:
                        module = self._module_cache[module_name]
                    else:
                        module = importlib.import_module(module_name)
                        self._module_cache[module_name] = module

                    for name, obj in inspect.getmembers(module):
                        if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                            if dcc_name and not obj.dcc:
                                obj.dcc = dcc_name
                            self.register(obj)
                            discovered_actions.append(obj)
                except ImportError as e:
                    self._logger.warning(f"Error importing module {module_name}: {e}")

        except ImportError as e:
            self._logger.warning(f"Error importing package {package_name}: {e}")

        return discovered_actions

    def discover_actions_from_path(
        self, path: str, dependencies: Optional[Dict[str, Any]] = None, dcc_name: Optional[str] = None
    ) -> List[Type[Action]]:
        """Load a Python module from a file path and register Action subclasses.

        This function is useful for loading actions from standalone Python files
        that are not part of a package.

        Args:
            path: Path to the Python file to load
            dependencies: Optional dictionary of dependencies to inject into the module
            dcc_name: Optional DCC name to inject DCC-specific dependencies

        Returns:
            List[Type[Action]]: List of discovered and registered Action classes

        Example:
            >>> registry = ActionRegistry()
            >>> actions = registry.discover_actions_from_path('/path/to/my_actions.py')
            >>> len(actions)
            2  # Discovered two actions in the file

        """
        discovered_actions = []
        discovered_action_classes = set()
        self._discover_actions_from_module(path, dependencies, dcc_name, discovered_actions, discovered_action_classes)
        return discovered_actions

    def _discover_actions_from_module(
        self,
        path: str,
        dependencies: Optional[Dict[str, Any]],
        dcc_name: Optional[str],
        discovered_actions: List[Type[Action]],
        discovered_action_classes: Set[Type[Action]],
    ) -> None:
        """Discover and register Action classes from a module.

        This method is used by both discover_actions_from_path and _discover_modules_from_path.

        Args:
            path: Path to the Python file to load
            dependencies: Optional dictionary of dependencies to inject into the module
            dcc_name: Optional DCC name to inject DCC-specific dependencies
            discovered_actions: List to append discovered actions to
            discovered_action_classes: Set of already discovered action classes to avoid duplicates

        """
        try:
            # Check if the module is already in the cache
            if path in self._module_cache:
                module = self._module_cache[path]
            else:
                # Use load_module_from_path from module_loader.py
                module = load_module_from_path(path, dependencies=dependencies, dcc_name=dcc_name)
                # Add to cache
                self._module_cache[path] = module

            # Find and register Action subclasses
            for name, obj in inspect.getmembers(module):
                if inspect.isclass(obj) and issubclass(obj, Action) and obj is not Action:
                    # Skip if already discovered
                    if obj in discovered_action_classes:
                        continue

                    # Set DCC name if not already set and dcc_name is provided
                    if dcc_name and not obj.dcc:
                        obj.dcc = dcc_name

                    # Set source file path for the action class
                    setattr(obj, "_source_file", path)

                    # Register the action class
                    self.register(obj)
                    discovered_action_classes.add(obj)
                    discovered_actions.append(obj)
                    self._logger.debug(f"Discovered action '{obj.__name__}' from path '{path}'")
            return
        except (ImportError, AttributeError) as e:
            # Log error but continue processing
            self._logger.warning(f"Error discovering actions from {path}: {e}")

    def get_actions_by_dcc(self, dcc_name: str) -> Dict[str, Type[Action]]:
        """Get all actions for a specific DCC.

        This method returns a dictionary of all actions registered for a specific DCC.
        The dictionary maps action names to action classes.

        Args:
            dcc_name: Name of the DCC to get actions for

        Returns:
            Dict[str, Type[Action]]: Dictionary of action name to action class
                                     Returns an empty dict if no actions are found

        """
        # Return a copy of the DCC-specific registry to prevent modification
        if dcc_name in self._dcc_actions:
            return dict(self._dcc_actions[dcc_name])

        # Return an empty dict if the DCC is not found
        self._logger.debug(f"No actions found for DCC '{dcc_name}'")
        return {}

    def get_all_dccs(self) -> List[str]:
        """Get a list of all DCCs that have registered actions.

        Returns:
            List[str]: List of DCC names

        """
        return list(self._dcc_actions.keys())
