"""Utilities package for DCC-MCP-Core.

This package contains utility modules for various tasks, including filesystem operations,
logging, platform detection, and other helper functions.
"""

# Import from platform.py (previously platform_utils.py)
from dcc_mcp_core.utils.platform import get_platform_dir, get_config_dir, get_data_dir, get_log_dir, get_actions_dir

# Import from decorators.py
from dcc_mcp_core.utils.decorators import error_handler, method_error_handler, with_context, format_result, format_exception

# Import from exceptions.py
from dcc_mcp_core.utils.exceptions import MCPError, ValidationError, ConfigurationError, ConnectionError, OperationError, VersionError, ParameterValidationError

# Import from template.py
from dcc_mcp_core.utils.template import render_template, get_template

# Import from constants.py
from dcc_mcp_core.utils.constants import (
    APP_NAME, APP_AUTHOR, LOG_APP_NAME, DEFAULT_LOG_LEVEL,
    ENV_LOG_LEVEL, ENV_ACTION_PATH_PREFIX, ENV_ACTIONS_DIR,
    ACTION_PATHS_CONFIG, BOOLEAN_FLAG_KEYS, ACTION_METADATA
)

__all__ = [
    # Platform utilities
    'get_platform_dir',
    'get_config_dir',
    'get_data_dir',
    'get_log_dir',
    'get_actions_dir',
    
    # Decorators
    'error_handler',
    'method_error_handler',
    'with_context',
    'format_result',
    'format_exception',
    
    # Exceptions
    'MCPError',
    'ValidationError',
    'ConfigurationError',
    'ConnectionError',
    'OperationError',
    'VersionError',
    'ParameterValidationError',

    # Template utilities
    'render_template',
    'get_template',
    
    # Constants
    'APP_NAME',
    'APP_AUTHOR',
    'LOG_APP_NAME',
    'DEFAULT_LOG_LEVEL',
    'ENV_LOG_LEVEL',
    'ENV_ACTION_PATH_PREFIX',
    'ENV_ACTIONS_DIR',
    'ACTION_PATHS_CONFIG',
    'BOOLEAN_FLAG_KEYS',
    'ACTION_METADATA',
]
