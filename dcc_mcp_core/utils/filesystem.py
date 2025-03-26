"""Filesystem utilities for the DCC-MCP ecosystem.

This module provides utilities for file and directory operations,
particularly focused on plugin path management for different DCCs.
"""

# Import built-in modules
from functools import lru_cache
import logging
import os
from pathlib import Path
from typing import List
from typing import Optional
from typing import Union

# Import third-party modules
import platformdirs

# Import local modules
from dcc_mcp_core.constants import APP_AUTHOR
from dcc_mcp_core.constants import APP_NAME
from dcc_mcp_core.constants import DIR_FUNCTIONS
from dcc_mcp_core.constants import ENV_ACTION_PATH_PREFIX

# Configure logging
logger = logging.getLogger(__name__)


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


@lru_cache(maxsize=32)
def get_actions_paths_from_env(dcc_name: Optional[str] = None) -> List[str]:
    """Get action paths from environment variables for a specific DCC.

    The environment variables should be in the format:
    ENV_ACTION_PATH_PREFIX + DCC_NAME (e.g. MCP_ACTION_PATH_MAYA)

    Args:
        dcc_name: Name of the DCC to get action paths for. If None, returns an empty list.

    Returns:
        List of action paths from environment variables for the specified DCC

    """
    if dcc_name is None:
        return []
    paths = [get_user_actions_directory(dcc_name)]

    try:
        env_var = f"{ENV_ACTION_PATH_PREFIX}{dcc_name.upper()}"
        value = os.environ.get(env_var, "")

        if not value:
            return paths

        paths.extend([Path(path).resolve().as_posix() for path in value.split(os.pathsep) if path])

        return paths
    except Exception:
        return []


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


def get_user_actions_directory(dcc_name: str) -> Path:
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

    return action_dir


def get_templates_directory() -> Path:
    """Get the path to the templates directory.

    Returns:
        Path to the templates directory

    """
    return (Path(__file__).parent / "template").resolve()


def get_user_data_dir():
    """Get the user data directory for the application.

    Returns:
        str: Path to the user data directory

    """
    return platformdirs.user_data_dir(APP_NAME, APP_AUTHOR)
