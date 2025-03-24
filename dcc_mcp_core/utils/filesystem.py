"""Filesystem utilities for the DCC-MCP ecosystem.

This module provides utilities for file and directory operations,
particularly focused on plugin path management for different DCCs.
"""

# Import built-in modules
from collections import defaultdict
from contextlib import contextmanager
from functools import lru_cache
import importlib
import importlib.util
import json
import logging
import os
from pathlib import Path
import sys
from types import ModuleType
from typing import Any
from typing import Dict
from typing import Generator
from typing import List
from typing import Optional
from typing import Union

# Import third-party modules
from pydantic import BaseModel
from pydantic import Field

# Import local modules
from dcc_mcp_core.utils.constants import ACTION_PATHS_CONFIG
from dcc_mcp_core.utils.constants import ENV_ACTIONS_DIR
from dcc_mcp_core.utils.constants import ENV_ACTION_PATH_PREFIX
from dcc_mcp_core.utils.platform import get_actions_dir
from dcc_mcp_core.utils.platform import get_config_dir

# Configure logging
logger = logging.getLogger(__name__)

# Default config path using platform_utils
config_dir = get_config_dir()
DEFAULT_CONFIG_PATH = Path(config_dir) / ACTION_PATHS_CONFIG


class ActionPathsConfig(BaseModel):
    """Configuration model for action paths with environment variable support.

    This class automatically loads environment variables with the prefix
    defined in ENV_ACTION_PATH_PREFIX.
    """

    dcc_actions_paths: Dict[str, List[str]] = Field(default_factory=lambda: defaultdict(list))
    default_actions_paths: Dict[str, List[str]] = Field(default_factory=dict)

    def __init__(self, **data):
        super().__init__(**data)
        # Process environment variables that follow the pattern ENV_ACTION_PATH_PREFIX + DCC_NAME
        self._load_from_env()

    def _load_from_env(self):
        """Load action paths from environment variables.

        The environment variables should be in the format:
        ENV_ACTION_PATH_PREFIX + DCC_NAME (e.g. MCP_ACTION_PATH_MAYA)
        """
        for env_var, value in os.environ.items():
            if env_var.startswith(ENV_ACTION_PATH_PREFIX):
                dcc_name = env_var[len(ENV_ACTION_PATH_PREFIX) :].lower()

                # Split paths by system path separator and normalize
                paths = [str(Path(path).resolve()) for path in value.split(os.pathsep) if path]

                if paths:
                    for path in paths:
                        if path not in self.dcc_actions_paths[dcc_name]:
                            self.dcc_actions_paths[dcc_name].append(path)
                            logger.info(f"Loaded action path from environment: {path} for {dcc_name}")


# Global configuration instance
_config = ActionPathsConfig()


def register_dcc_actions_path(dcc_name: str, action_path: str) -> None:
    """Register an action path for a specific DCC.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        action_path: Path to the actions directory

    """
    # Normalize DCC name (lowercase)
    dcc_name = dcc_name.lower()

    # Normalize path
    action_path = str(Path(action_path).resolve())

    # Load current configuration
    _load_config_if_needed()

    # Ensure dcc_name exists in the dictionary
    if dcc_name not in _config.dcc_actions_paths:
        _config.dcc_actions_paths[dcc_name] = []
        logger.info(f"Created new action paths entry for DCC: {dcc_name}")

    # Add path if it's not already in the list
    if action_path not in _config.dcc_actions_paths[dcc_name]:
        _config.dcc_actions_paths[dcc_name].append(action_path)
        logger.info(f"Registered action path for {dcc_name}: {action_path}")

        # Save configuration
        save_actions_paths_config()


def register_dcc_actions_paths(dcc_name: str, action_paths: List[str]) -> None:
    """Register multiple action paths for a specific DCC.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        action_paths: List of paths to the actions directories

    """
    for action_path in action_paths:
        register_dcc_actions_path(dcc_name, action_path)


def get_action_paths(dcc_name: Optional[str] = None) -> Union[List[str], Dict[str, List[str]]]:
    """Get action paths for a specific DCC or all DCCs.

    This function returns action paths from both the configuration file and
    environment variables. Paths from environment variables are already integrated
    into the configuration through the ActionPathsConfig class.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini'). If None, returns paths for all DCCs.

    Returns:
        If dcc_name is provided, returns a list of action paths for that DCC.
        If dcc_name is None, returns a dictionary mapping DCC names to their action paths.

    """
    # Load current configuration
    _load_config_if_needed()

    if dcc_name is not None:
        # Normalize DCC name
        dcc_name = dcc_name.lower()

        # Get paths from configuration - dcc_actions_paths is now a defaultdict(list)
        config_paths = _config.dcc_actions_paths[dcc_name].copy()

        # If no registered paths, use default paths if available
        if not config_paths and dcc_name in _config.default_actions_paths:
            config_paths = _config.default_actions_paths[dcc_name].copy()

        return config_paths
    else:
        # Return all paths
        result = {dcc: paths.copy() for dcc, paths in _config.dcc_actions_paths.items()}
        return result


def set_default_action_paths(dcc_name: str, action_paths: List[str]) -> None:
    """Set default action paths for a specific DCC.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        action_paths: List of default action paths

    """
    # Normalize DCC name
    dcc_name = dcc_name.lower()

    # Normalize paths
    normalized_paths = [Path(path).resolve().as_posix() for path in action_paths]

    # Load current configuration
    _load_config_if_needed()

    # Set default paths
    _config.default_actions_paths[dcc_name] = normalized_paths
    logger.info(f"Set default action paths for {dcc_name}: {normalized_paths}")

    # Save configuration
    save_actions_paths_config()


def get_all_registered_dccs() -> List[str]:
    """Get a list of all registered DCCs.

    Returns:
        List of registered DCC names

    """
    # Load current configuration
    _load_config_if_needed()

    return list(_config.default_actions_paths.keys())


def save_actions_paths_config(config_path: Optional[Union[str, Path]] = None) -> bool:
    """Save the current action paths configuration to a file.

    Args:
        config_path: Path to the configuration file. If None, uses the default path.

    Returns:
        True if the configuration was saved successfully, False otherwise

    """
    if config_path is None:
        config_path = DEFAULT_CONFIG_PATH
    else:
        config_path = Path(config_path)

    try:
        # Create directory if it doesn't exist
        config_dir = config_path.parent
        config_dir.mkdir(parents=True, exist_ok=True)

        # Prepare data to save
        config_data = _config.model_dump()

        # Write to file
        with open(config_path, "w") as f:
            json.dump(config_data, f, indent=4)

        logger.info(f"Saved action paths configuration to {config_path}")
        return True
    except Exception as e:
        logger.error(f"Error saving action paths configuration: {e!s}")
        return False


def load_actions_paths_config(config_path: Optional[Union[str, Path]] = None) -> bool:
    """Load action paths configuration from a file.

    Args:
        config_path: Path to the configuration file. If None, uses the default path.

    Returns:
        True if the configuration was loaded successfully, False otherwise

    """
    if config_path is None:
        config_path = DEFAULT_CONFIG_PATH
    else:
        config_path = Path(config_path)

    if not config_path.exists():
        logger.warning(f"Action paths configuration file does not exist: {config_path}")
        return False

    try:
        # Read from file
        with open(config_path) as f:
            config_data = json.load(f)

        # Update configuration
        for key, value in config_data.items():
            if hasattr(_config, key):
                setattr(_config, key, value)

        logger.info(f"Loaded action paths configuration from {config_path}")
        return True
    except Exception as e:
        logger.error(f"Error loading action paths configuration: {e!s}")
        return False


@lru_cache(maxsize=32)
def get_actions_paths_from_env(dcc_name: Optional[str] = None) -> Dict[str, List[str]]:
    """Get action paths from environment variables.

    The environment variables should be in the format:
    ENV_ACTION_PATH_PREFIX + DCC_NAME (e.g. MCP_ACTION_PATH_MAYA)

    Args:
        dcc_name: Name of the DCC to get action paths for. If None, gets for all DCCs.

    Returns:
        Dictionary mapping DCC names to lists of action paths from environment variables

    """
    result = {}

    # Process environment variables
    for env_var, value in os.environ.items():
        if env_var.startswith(ENV_ACTION_PATH_PREFIX):
            key = env_var[len(ENV_ACTION_PATH_PREFIX) :].lower()

            if dcc_name is not None and key != dcc_name.lower():
                continue

            # Split paths by system path separator and normalize
            paths = [str(Path(path).resolve()) for path in value.split(os.pathsep) if path]

            if paths:
                result[key] = paths

    return result


def _load_config_if_needed() -> None:
    """Load configuration if the cache is empty."""
    if not _config.dcc_actions_paths and not _config.default_actions_paths:
        # Try to load from config file
        load_actions_paths_config()
        # Environment variables are already loaded in the ActionPathsConfig constructor


def discover_actions(
    dcc_name: Optional[str] = None, extension: str = ".py", additional_paths: Optional[Dict[str, List[str]]] = None
) -> Dict[str, List[str]]:
    """Discover actions in registered action paths.

    Args:
        dcc_name: Name of the DCC to discover actions for. If None, discovers for all DCCs.
        extension: File extension to filter actions (default: '.py')
        additional_paths: Optional dictionary mapping DCC names to additional paths to search

    Returns:
        Dictionary mapping DCC names to lists of discovered action paths

    """
    # Load configuration if needed
    _load_config_if_needed()

    # Get action paths
    if dcc_name:
        # Get paths for a specific DCC
        dcc_name = dcc_name.lower()
        paths = _config.dcc_actions_paths.get(dcc_name, [])

        # Add additional paths for this DCC if provided
        if additional_paths and dcc_name in additional_paths:
            paths.extend(additional_paths[dcc_name])

        return {dcc_name: _discover_actions_in_paths(paths, extension)}
    else:
        # Get paths for all DCCs
        result = {}
        for dcc, paths in _config.dcc_actions_paths.items():
            # Add additional paths for this DCC if provided
            if additional_paths and dcc in additional_paths:
                paths = paths + additional_paths[dcc]
            result[dcc] = _discover_actions_in_paths(paths, extension)
        return result


def _discover_actions_in_paths(action_paths: List[str], extension: str) -> List[str]:
    """Discover actions in the given paths with the specified extension.

    Args:
        action_paths: List of paths to search for actions
        extension: File extension to filter actions

    Returns:
        List of discovered action paths

    """
    discovered_actions = []

    for action_dir in action_paths:
        action_dir_path = Path(action_dir)
        if not action_dir_path.exists():
            logger.warning(f"Action directory does not exist: {action_dir}")
            continue

        try:
            # Recursively get all files in the directory with the specified extension
            for action_file in action_dir_path.glob(f"**/*{extension}"):
                # Skip files that start with underscore and __init__.py
                file_name = action_file.name
                if file_name.startswith("_") or file_name == "__init__.py":
                    continue

                discovered_actions.append(str(action_file))
        except Exception as e:
            logger.error(f"Error discovering actions in {action_dir}: {e!s}")

    return discovered_actions


def ensure_directory_exists(directory_path: Union[str, Path]) -> bool:
    """Ensure that a directory exists, creating it if necessary.

    Args:
        directory_path: Path to the directory

    Returns:
        True if the directory exists or was created successfully, False otherwise

    """
    try:
        path = Path(directory_path)
        if not path.exists():
            path.mkdir(parents=True, exist_ok=True)
            logger.info(f"Created directory: {directory_path}")
        return True
    except Exception as e:
        logger.error(f"Error creating directory {directory_path}: {e!s}")
        return False


def get_user_actions_directory(dcc_name: str) -> str:
    """Get the user's action directory for a specific DCC.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')

    Returns:
        Path to the user's action directory

    """
    # Normalize DCC name
    dcc_name = dcc_name.lower()

    # Get user's action directory using platform_utils
    action_dir = get_actions_dir(dcc_name)

    # Ensure the directory exists
    ensure_directory_exists(action_dir)

    return action_dir.as_posix()


def get_actions_dir_from_env() -> str:
    """Get the actions directory path from environment variables.

    Returns:
        Path to the actions directory from environment variables, or an empty string if not set

    """
    # This is a special case that gets a single directory path rather than a list of paths
    actions_dir = os.getenv(ENV_ACTIONS_DIR, "")
    if actions_dir:
        return Path(actions_dir).resolve().as_posix()
    return ""


def get_templates_directory() -> str:
    """Get the path to the templates directory.

    Returns:
        Path to the templates directory

    """
    return (Path(__file__).parent / "template").resolve().as_posix()


def convert_path_to_module(file_path: str) -> str:
    """Convert a file path to a Python module path.

    This function converts a file path (e.g., 'path/to/module.py') to a
    Python module path that can be used with importlib. For simplicity and to avoid
    issues with absolute paths, it returns only the filename without extension.

    Args:
        file_path: Path to the Python file

    Returns:
        Python module path suitable for importlib.import_module

    """
    # Convert to Path object
    path = Path(file_path)

    # Get the file name without extension
    file_name = path.stem

    # Remove any invalid characters for module names
    module_path = file_name.replace("-", "_")

    return module_path


@contextmanager
def append_to_python_path(script: Union[str, Path]) -> Generator[None, None, None]:
    """Temporarily append a directory to sys.path within a context.

    This context manager adds the directory containing the specified script
    to sys.path and removes it when exiting the context.

    Args:
        script: The absolute path to a script file.

    Yields:
        None

    Example:
        >>> with append_to_python_path('/path/to/script.py'):
        ...     import some_module  # module in script's directory

    """
    script_path = Path(script)
    if script_path.is_file():
        script_dir = script_path.parent
    else:
        script_dir = script_path

    # Convert to string representation for sys.path
    script_dir_str = str(script_dir)

    # Check if the path is already in sys.path
    if script_dir_str not in sys.path:
        sys.path.insert(0, script_dir_str)
        path_added = True
    else:
        path_added = False

    try:
        yield
    finally:
        # Only remove the path if we added it
        if path_added and script_dir_str in sys.path:
            sys.path.remove(script_dir_str)


def clear_dcc_actions_paths(dcc_name: Optional[str] = None, keep_paths: Optional[List[str]] = None) -> None:
    """Clear registered action paths for a specific DCC or all DCCs.

    This function allows cleaning up invalid or unwanted action paths from the configuration.
    It can either clear all paths for a specific DCC, or selectively keep certain paths.

    Args:
        dcc_name: Name of the DCC to clear paths for. If None, clears paths for all DCCs.
        keep_paths: List of paths to keep. If None, clears all paths except those that exist.

    """
    # Normalize DCC name if provided
    if dcc_name is not None:
        dcc_name = dcc_name.lower()

    # Initialize keep_paths if None
    if keep_paths is None:
        keep_paths = []

    # Convert to absolute paths for comparison
    keep_paths = [str(Path(path).resolve()) for path in keep_paths]

    # Load current configuration
    _load_config_if_needed()

    if dcc_name is not None:
        # Clear paths for a specific DCC
        if dcc_name in _config.dcc_actions_paths:
            # Filter paths to keep only valid ones and those in keep_paths
            valid_paths = []
            for path in _config.dcc_actions_paths[dcc_name]:
                path_obj = Path(path)
                if path in keep_paths or str(path_obj.resolve()) in keep_paths or path_obj.exists():
                    valid_paths.append(path)

            _config.dcc_actions_paths[dcc_name] = valid_paths
            logger.info(f"Cleaned action paths for {dcc_name}: {valid_paths}")
    else:
        # Clear paths for all DCCs
        for dcc in list(_config.dcc_actions_paths.keys()):
            # Filter paths to keep only valid ones and those in keep_paths
            valid_paths = []
            for path in _config.dcc_actions_paths[dcc]:
                path_obj = Path(path)
                if path in keep_paths or str(path_obj.resolve()) in keep_paths or path_obj.exists():
                    valid_paths.append(path)

            _config.dcc_actions_paths[dcc] = valid_paths
            logger.info(f"Cleaned action paths for {dcc}: {valid_paths}")

    # Save configuration
    save_actions_paths_config()


def load_module_from_path(
    file_path: str, module_name: Optional[str] = None, dependencies: Optional[Dict[str, Any]] = None
) -> ModuleType:
    """Load a Python module directly from a file path and inject dependencies.

    This function allows loading a Python module directly from a file path and injecting necessary dependencies.
    This is particularly useful for loading plugins or actions that may depend on specific environments.

    Args:
        file_path: Path to the Python file to load
        module_name: Optional module name, if not provided, will be generated from the file name
        dependencies: Optional dictionary of dependencies to inject into the module, keys are attribute
            names, values are objects

    Returns:
        Loaded Python module

    Raises:
        ImportError: If module specification creation fails or module loading fails

    """
    # Ensure file exists
    if not os.path.isfile(file_path):
        raise ImportError(f"File does not exist: {file_path}")

    # If module name is not provided, generate from file name
    if module_name is None:
        module_name = convert_path_to_module(file_path)

    # Create module spec
    spec = importlib.util.spec_from_file_location(module_name, file_path)
    if spec is None:
        raise ImportError(f"无法为 {file_path} 创建模块规范")

    # Create module from spec
    module = importlib.util.module_from_spec(spec)

    # Set module __name__ attribute
    module.__name__ = module_name

    # Inject default dependency
    # Import local modules
    import dcc_mcp_core

    module.dcc_mcp_core = dcc_mcp_core

    # Inject additional dependencies
    if dependencies:
        for name, obj in dependencies.items():
            setattr(module, name, obj)

    # Execute module code
    spec.loader.exec_module(module)

    return module
