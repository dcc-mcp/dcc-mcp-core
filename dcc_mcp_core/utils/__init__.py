"""Utilities package for DCC-MCP-Core.

This package contains utility modules for various tasks, including filesystem operations,
logging, platform detection, and other helper functions.
"""

# Import local modules
# Import from constants.py
from dcc_mcp_core.utils.constants import ACTION_METADATA
from dcc_mcp_core.utils.constants import ACTION_PATHS_CONFIG
from dcc_mcp_core.utils.constants import APP_AUTHOR
from dcc_mcp_core.utils.constants import APP_NAME
from dcc_mcp_core.utils.constants import BOOLEAN_FLAG_KEYS
from dcc_mcp_core.utils.constants import DEFAULT_LOG_LEVEL
from dcc_mcp_core.utils.constants import ENV_ACTIONS_DIR
from dcc_mcp_core.utils.constants import ENV_ACTION_PATH_PREFIX
from dcc_mcp_core.utils.constants import ENV_LOG_LEVEL
from dcc_mcp_core.utils.constants import LOG_APP_NAME

# Import from decorators.py
from dcc_mcp_core.utils.decorators import error_handler
from dcc_mcp_core.utils.decorators import format_exception
from dcc_mcp_core.utils.decorators import format_result
from dcc_mcp_core.utils.decorators import method_error_handler
from dcc_mcp_core.utils.decorators import with_context

# Import from exceptions.py
from dcc_mcp_core.utils.exceptions import ConfigurationError
from dcc_mcp_core.utils.exceptions import ConnectionError
from dcc_mcp_core.utils.exceptions import MCPError
from dcc_mcp_core.utils.exceptions import OperationError
from dcc_mcp_core.utils.exceptions import ParameterValidationError
from dcc_mcp_core.utils.exceptions import ValidationError
from dcc_mcp_core.utils.exceptions import VersionError

# Import from platform.py (previously platform_utils.py)
from dcc_mcp_core.utils.platform import get_actions_dir
from dcc_mcp_core.utils.platform import get_config_dir
from dcc_mcp_core.utils.platform import get_data_dir
from dcc_mcp_core.utils.platform import get_log_dir
from dcc_mcp_core.utils.platform import get_platform_dir

# Import from template.py
from dcc_mcp_core.utils.template import get_template
from dcc_mcp_core.utils.template import render_template

__all__ = [
    "ACTION_METADATA",
    "ACTION_PATHS_CONFIG",
    "APP_AUTHOR",
    "APP_NAME",
    "BOOLEAN_FLAG_KEYS",
    "DEFAULT_LOG_LEVEL",
    "ENV_ACTIONS_DIR",
    "ENV_ACTION_PATH_PREFIX",
    "ENV_LOG_LEVEL",
    "ConfigurationError",
    "ConnectionError",
    "MCPError",
    "OperationError",
    "ParameterValidationError",
    "ValidationError",
    "VersionError",
    "error_handler",
    "format_exception",
    "format_result",
    "get_actions_dir",
    "get_config_dir",
    "get_data_dir",
    "get_log_dir",
    "get_platform_dir",
    "get_template",
    "method_error_handler",
    "render_template",
    "with_context",
]
