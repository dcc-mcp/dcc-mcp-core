"""Utilities package for DCC-MCP-Core.

This package contains utility modules for various tasks, including filesystem operations,
logging, platform detection, and other helper functions.
"""

# Import local modules
from dcc_mcp_core.utils.decorators import error_handler
from dcc_mcp_core.utils.decorators import format_exception
from dcc_mcp_core.utils.decorators import format_result
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
from dcc_mcp_core.utils.filesystem import get_actions_dir
from dcc_mcp_core.utils.filesystem import get_config_dir
from dcc_mcp_core.utils.filesystem import get_data_dir
from dcc_mcp_core.utils.filesystem import get_log_dir
from dcc_mcp_core.utils.filesystem import get_platform_dir

# Import from template.py
from dcc_mcp_core.utils.template import get_template
from dcc_mcp_core.utils.template import render_template

__all__ = [
    "BOOLEAN_FLAG_KEYS",
    "DEFAULT_LOG_LEVEL",
    "ENV_ACTIONS_DIR",
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
