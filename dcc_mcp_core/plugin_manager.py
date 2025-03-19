"""Plugin manager module for DCC-MCP-Core.

This module provides functionality for discovering, loading, and managing plugins
for various Digital Content Creation (DCC) applications. It includes utilities for
registering plugin paths, creating plugin managers, and calling plugin functions.
"""

# Import built-in modules
import importlib
import os
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import local modules
from dcc_mcp_core import filesystem

# Configure logging
from dcc_mcp_core.logg_config import setup_logging
from dcc_mcp_core.utils import extract_function_info
from dcc_mcp_core.utils import extract_module_metadata

logger = setup_logging("plugin_manager")


class PluginManager:
    """Manager for DCC plugins.

    This class provides functionality for discovering, loading, and managing plugins
    for different DCCs in the DCC-MCP ecosystem.

    Attributes:
        dcc_name: Name of the DCC this plugin manager is for

    """

    def __init__(self, dcc_name: str):
        """Initialize the plugin manager.

        Args:
            dcc_name: Name of the DCC this plugin manager is for

        """
        self.dcc_name = dcc_name.lower()
        self._plugins: Dict[str, Any] = {}
        self._plugin_modules: Dict[str, Any] = {}

    def discover_plugins(self, extension: str = ".py") -> List[str]:
        """Discover plugins for this DCC.

        Args:
            extension: File extension to filter plugins (default: '.py')

        Returns:
            List of discovered plugin paths

        """
        discovered = filesystem.discover_plugins(self.dcc_name, extension)
        if self.dcc_name in discovered:
            return discovered[self.dcc_name]
        return []

    def load_plugin(self, plugin_path: str) -> Optional[Any]:
        """Load a plugin from a file.

        Args:
            plugin_path: Path to the plugin file

        Returns:
            The loaded plugin module, or None if loading failed

        """
        try:
            # Get the module name from the file path
            module_name = os.path.splitext(os.path.basename(plugin_path))[0]

            # Add the plugin directory to sys.path if it's not already there
            plugin_dir = os.path.dirname(plugin_path)
            if plugin_dir not in sys.path:
                sys.path.insert(0, plugin_dir)

            # Import the module
            plugin_module = importlib.import_module(module_name)

            # Store the module
            plugin_name = getattr(plugin_module, "__plugin_name__", module_name)
            self._plugin_modules[plugin_name] = plugin_module

            # Check if the module has a 'register' function
            if hasattr(plugin_module, "register") and callable(plugin_module.register):
                # Call the register function
                self._plugins[plugin_name] = plugin_module.register()
            else:
                # Auto-register all public functions
                self._plugins[plugin_name] = self._auto_register_functions(plugin_module)

            logger.info(f"Loaded plugin: {plugin_name}")
            return plugin_module

        except Exception as e:
            logger.error(f"Error loading plugin {plugin_path}: {e!s}")
            return None

    def _auto_register_functions(self, plugin_module: Any) -> Dict[str, Any]:
        """Automatically register all public functions from a plugin module.

        Args:
            plugin_module: The plugin module to register functions from

        Returns:
            Dictionary mapping function names to callable functions

        """
        # Import built-in modules
        import inspect

        functions = {}

        # Auto-collect all non-internal functions
        for name, obj in inspect.getmembers(plugin_module):
            if inspect.isfunction(obj) and not name.startswith('_') and name != 'register':
                functions[name] = obj

        plugin_name = getattr(plugin_module, "__plugin_name__", plugin_module.__name__)
        logger.info(f"Auto-registered {len(functions)} functions for plugin: {plugin_name}")

        return functions

    def load_plugins(self, plugin_paths: Optional[List[str]] = None) -> Dict[str, Dict[str, Any]]:
        """Load multiple plugins and return AI-friendly structured information.

        Args:
            plugin_paths: List of paths to plugin files. If None, discovers and loads all plugins.

        Returns:
            Dictionary with AI-friendly structured information about loaded plugins including:
            - Plugin name, description, version
            - Available functions with their parameters and documentation
            - Usage examples where applicable

        """
        if plugin_paths is None:
            # Discover and load all plugins
            plugin_paths = self.discover_plugins()

        for plugin_path in plugin_paths:
            self.load_plugin(plugin_path)

        # Return AI-friendly structured information about loaded plugins
        return self.get_plugins_info()

    def get_plugin(self, plugin_name: str) -> Optional[Any]:
        """Get a loaded plugin by name.

        Args:
            plugin_name: Name of the plugin to get

        Returns:
            The plugin, or None if not found

        """
        return self._plugins.get(plugin_name)

    def get_plugins(self) -> Dict[str, Any]:
        """Get all loaded plugins.

        Returns:
            Dictionary mapping plugin names to loaded plugins

        """
        return self._plugins.copy()

    def get_plugin_info(self, plugin_name: str) -> Dict[str, Any]:
        """Get detailed information about a loaded plugin.

        Args:
            plugin_name: Name of the plugin to get information for

        Returns:
            Dictionary containing plugin information

        """
        if plugin_name not in self._plugin_modules:
            logger.warning(f"Plugin {plugin_name} not found")
            return {}

        plugin_module = self._plugin_modules[plugin_name]
        plugin_functions = self._plugins.get(plugin_name, {})

        # Extract plugin metadata
        plugin_info = extract_module_metadata(plugin_module, plugin_name)
        plugin_info["functions"] = {}

        # Extract function information
        if isinstance(plugin_functions, dict):
            for func_name, func in plugin_functions.items():
                if callable(func):
                    plugin_info["functions"][func_name] = extract_function_info(func)

        return plugin_info

    def get_plugins_info(self) -> Dict[str, Dict[str, Any]]:
        """Get AI-friendly structured information about all loaded plugins.

        Returns:
            Dictionary mapping plugin names to detailed plugin information including:
            - Plugin metadata (name, version, description, author)
            - Available functions with their parameters, return types, and documentation
            - Usage examples for each function

        """
        plugins_info = {}
        for plugin_name in self._plugin_modules.keys():
            plugin_info = self.get_plugin_info(plugin_name)
            if plugin_info:
                plugins_info[plugin_name] = plugin_info
        return plugins_info

    def call_plugin_function(self, plugin_name: str, function_name: str, *args, **kwargs) -> Any:
        """Call a function from a plugin.

        Args:
            plugin_name: Name of the plugin
            function_name: Name of the function to call
            *args: Positional arguments to pass to the function
            **kwargs: Keyword arguments to pass to the function

        Returns:
            The result of the function call

        Raises:
            ValueError: If the plugin or function is not found

        """
        if plugin_name not in self._plugin_modules:
            raise ValueError(f"Plugin not found: {plugin_name}")

        plugin_module = self._plugin_modules[plugin_name]

        if not hasattr(plugin_module, function_name) or not callable(getattr(plugin_module, function_name)):
            raise ValueError(f"Function not found in plugin {plugin_name}: {function_name}")

        # Get the function
        function = getattr(plugin_module, function_name)

        # Call the function with the provided arguments
        return function(*args, **kwargs)


def create_plugin_manager(dcc_name: str) -> PluginManager:
    """Create a plugin manager for a specific DCC.

    Args:
        dcc_name: Name of the DCC to create a plugin manager for

    Returns:
        A plugin manager instance for the specified DCC

    """
    return PluginManager(dcc_name)


# Cache for plugin managers
_plugin_managers: Dict[str, PluginManager] = {}


def get_plugin_manager(dcc_name: str) -> PluginManager:
    """Get or create a plugin manager for a specific DCC.

    Args:
        dcc_name: Name of the DCC to get a plugin manager for

    Returns:
        A plugin manager instance for the specified DCC

    """
    # Normalize DCC name
    dcc_name = dcc_name.lower()

    # Check if a plugin manager already exists for this DCC
    if dcc_name not in _plugin_managers:
        # Create a new plugin manager
        _plugin_managers[dcc_name] = create_plugin_manager(dcc_name)

    return _plugin_managers[dcc_name]


def discover_plugins(dcc_name: str, extension: str = ".py") -> List[str]:
    """Discover plugins for a specific DCC.

    Args:
        dcc_name: Name of the DCC to discover plugins for
        extension: File extension to filter plugins (default: '.py')

    Returns:
        List of discovered plugin paths

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.discover_plugins(extension)


def load_plugin(dcc_name: str, plugin_path: str) -> Optional[Any]:
    """Load a plugin for a specific DCC.

    Args:
        dcc_name: Name of the DCC to load the plugin for
        plugin_path: Path to the plugin file

    Returns:
        The loaded plugin module, or None if loading failed

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.load_plugin(plugin_path)


def load_plugins(dcc_name: str, plugin_paths: Optional[List[str]] = None) -> Dict[str, Dict[str, Any]]:
    """Load multiple plugins for a specific DCC.

    Args:
        dcc_name: Name of the DCC to load the plugins for
        plugin_paths: List of paths to plugin files. If None, discovers and loads all plugins.

    Returns:
        Dictionary with AI-friendly structured information about loaded plugins including:
        - Plugin name, description, version
        - Available functions with their parameters and documentation
        - Usage examples where applicable

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.load_plugins(plugin_paths)


def get_plugin(dcc_name: str, plugin_name: str) -> Optional[Any]:
    """Get a loaded plugin for a specific DCC.

    Args:
        dcc_name: Name of the DCC to get the plugin for
        plugin_name: Name of the plugin to get

    Returns:
        The plugin, or None if not found

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.get_plugin(plugin_name)


def get_plugins(dcc_name: str) -> Dict[str, Any]:
    """Get all loaded plugins for a specific DCC.

    Args:
        dcc_name: Name of the DCC to get the plugins for

    Returns:
        Dictionary mapping plugin names to loaded plugins

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.get_plugins()


def get_plugin_info(dcc_name: str, plugin_name: str) -> Dict[str, Any]:
    """Get information about a plugin for a specific DCC.

    Args:
        dcc_name: Name of the DCC to get the plugin information for
        plugin_name: Name of the plugin to get information for

    Returns:
        Dictionary with plugin information

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.get_plugin_info(plugin_name)


def get_plugins_info(dcc_name: str) -> Dict[str, Dict[str, Any]]:
    """Get AI-friendly structured information about all loaded plugins for a specific DCC.

    Args:
        dcc_name: Name of the DCC to get plugin information for

    Returns:
        Dictionary mapping plugin names to detailed plugin information including:
        - Plugin metadata (name, version, description, author)
        - Available functions with their parameters, return types, and documentation
        - Usage examples for each function

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.get_plugins_info()


def call_plugin_function(dcc_name: str, plugin_name: str, function_name: str, *args, **kwargs) -> Any:
    """Call a function from a plugin for a specific DCC.

    Args:
        dcc_name: Name of the DCC to call the plugin function for
        plugin_name: Name of the plugin
        function_name: Name of the function to call
        *args: Positional arguments to pass to the function
        **kwargs: Keyword arguments to pass to the function

    Returns:
        The result of the function call

    """
    plugin_manager = get_plugin_manager(dcc_name)
    return plugin_manager.call_plugin_function(plugin_name, function_name, *args, **kwargs)
