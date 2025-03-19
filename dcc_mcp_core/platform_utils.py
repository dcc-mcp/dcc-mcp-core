"""Platform utilities for the DCC-MCP ecosystem.

This module provides platform-specific utilities for directory paths,
ensuring consistent behavior across different platforms and platformdirs versions.
"""

# Import built-in modules
import os
from pathlib import Path
from typing import Union

# Import third-party modules
import platformdirs

# Import local modules
from dcc_mcp_core.constants import APP_AUTHOR
from dcc_mcp_core.constants import APP_NAME


def get_platform_dir(dir_type: str, app_name: str = APP_NAME, app_author: str = APP_AUTHOR, 
                     ensure_exists: bool = True) -> Union[str, Path]:
    """Get a platform-specific directory path with compatibility for different platformdirs versions.
    
    Args:
        dir_type: Type of directory ('config', 'data', 'log', 'cache', etc.)
        app_name: Application name
        app_author: Application author
        ensure_exists: Whether to ensure the directory exists
        
    Returns:
        Path to the requested directory

    """
    # Map of directory types to platformdirs functions
    dir_functions = {
        'config': ('user_config_path', 'user_config_dir'),
        'data': ('user_data_path', 'user_data_dir'),
        'log': ('user_log_path', 'user_log_dir'),
        'cache': ('user_cache_path', 'user_cache_dir'),
        'state': ('user_state_path', 'user_state_dir'),
        'documents': ('user_documents_path', 'user_documents_dir'),
    }
    
    if dir_type not in dir_functions:
        raise ValueError(f"Unknown directory type: {dir_type}. "  
                         f"Valid types are: {', '.join(dir_functions.keys())}")
    
    # Get the appropriate function names for this directory type
    new_func_name, old_func_name = dir_functions[dir_type]
    
    try:
        # Try using the new version API (path objects with ensure_exists parameter)
        path_func = getattr(platformdirs, new_func_name)
        dir_path = path_func(app_name, appauthor=app_author, ensure_exists=ensure_exists)
    except (AttributeError, TypeError):
        # Fallback to old version API
        path_func = getattr(platformdirs, old_func_name)
        dir_path = path_func(app_name, appauthor=app_author)
        
        # Ensure directory exists if requested
        if ensure_exists and isinstance(dir_path, str):
            os.makedirs(dir_path, exist_ok=True)
    
    return dir_path


def get_config_dir(ensure_exists: bool = True) -> Union[str, Path]:
    """Get the platform-specific configuration directory.
    
    Args:
        ensure_exists: Whether to ensure the directory exists
        
    Returns:
        Path to the configuration directory

    """
    return get_platform_dir('config', ensure_exists=ensure_exists)


def get_data_dir(ensure_exists: bool = True) -> Union[str, Path]:
    """Get the platform-specific data directory.
    
    Args:
        ensure_exists: Whether to ensure the directory exists
        
    Returns:
        Path to the data directory

    """
    return get_platform_dir('data', ensure_exists=ensure_exists)


def get_log_dir(ensure_exists: bool = True) -> Union[str, Path]:
    """Get the platform-specific log directory.
    
    Args:
        ensure_exists: Whether to ensure the directory exists
        
    Returns:
        Path to the log directory

    """
    return get_platform_dir('log', ensure_exists=ensure_exists)


def get_plugin_dir(dcc_name: str, ensure_exists: bool = True) -> Union[str, Path]:
    """Get the platform-specific plugin directory for a specific DCC.
    
    Args:
        dcc_name: Name of the DCC (e.g., 'maya', 'houdini')
        ensure_exists: Whether to ensure the directory exists
        
    Returns:
        Path to the plugin directory

    """
    data_dir = get_data_dir(ensure_exists=False)
    
    # Handle both Path and string types
    if isinstance(data_dir, Path):
        plugin_dir = data_dir / "plugins" / dcc_name
        if ensure_exists:
            plugin_dir.mkdir(parents=True, exist_ok=True)
    else:  # string type
        plugin_dir = os.path.join(data_dir, "plugins", dcc_name)
        if ensure_exists:
            os.makedirs(plugin_dir, exist_ok=True)
            
    return plugin_dir
