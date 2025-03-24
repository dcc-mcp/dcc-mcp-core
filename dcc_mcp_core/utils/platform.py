"""Platform utilities for the DCC-MCP ecosystem.

This module provides platform-specific utilities for directory paths,
ensuring consistent behavior across different platforms.
"""

# Import built-in modules
import os
from typing import Dict

# Import third-party modules
import platformdirs

# Import local modules
from dcc_mcp_core.utils.constants import APP_AUTHOR
from dcc_mcp_core.utils.constants import APP_NAME

# Map of directory types to platformdirs functions
DIR_FUNCTIONS: Dict[str, str] = {
    "config": "user_config_dir",
    "data": "user_data_dir",
    "log": "user_log_dir",
    "cache": "user_cache_dir",
    "state": "user_state_dir",
    "documents": "user_documents_dir",
}


def get_platform_dir(
    dir_type: str, app_name: str = APP_NAME, app_author: str = APP_AUTHOR, ensure_exists: bool = True
) -> str:
    """Get a platform-specific directory path.

    Args:
        dir_type: Type of directory ('config', 'data', 'log', 'cache', etc.)
        app_name: Application name
        app_author: Application author
        ensure_exists: Whether to ensure the directory exists

    Returns:
        Path to the requested directory as a string

    """
    if dir_type not in DIR_FUNCTIONS:
        raise ValueError(f"Unknown directory type: {dir_type}. Valid types are: {', '.join(DIR_FUNCTIONS.keys())}")

    # Get the appropriate function for this directory type
    func_name = DIR_FUNCTIONS[dir_type]
    path_func = getattr(platformdirs, func_name)

    # Get the directory path
    dir_path = path_func(app_name, appauthor=app_author)

    # Ensure directory exists if requested
    if ensure_exists:
        os.makedirs(dir_path, exist_ok=True)

    return dir_path


def get_config_dir(ensure_exists: bool = True) -> str:
    """Get the platform-specific configuration directory.

    Args:
        ensure_exists: Whether to ensure the directory exists

    Returns:
        Path to the configuration directory

    """
    return get_platform_dir("config", ensure_exists=ensure_exists)


def get_data_dir(ensure_exists: bool = True) -> str:
    """Get the platform-specific data directory.

    Args:
        ensure_exists: Whether to ensure the directory exists

    Returns:
        Path to the data directory

    """
    return get_platform_dir("data", ensure_exists=ensure_exists)


def get_log_dir(ensure_exists: bool = True) -> str:
    """Get the platform-specific log directory.

    Args:
        ensure_exists: Whether to ensure the directory exists

    Returns:
        Path to the log directory

    """
    return get_platform_dir("log", ensure_exists=ensure_exists)


def get_actions_dir(dcc_name: str, ensure_exists: bool = True) -> str:
    """Get the platform-specific actions directory for a specific DCC.

    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        ensure_exists: Whether to ensure the directory exists

    Returns:
        Path to the actions directory

    """
    data_dir = get_data_dir(ensure_exists=False)
    actions_dir = os.path.join(data_dir, "actions", dcc_name)

    if ensure_exists:
        os.makedirs(actions_dir, exist_ok=True)

    return actions_dir
