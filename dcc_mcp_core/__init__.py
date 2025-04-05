"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem."""

# Import Pydantic extensions module, automatically apply patches
# Import local modules
from dcc_mcp_core import models
from dcc_mcp_core.actions.manager import create_action_manager
from dcc_mcp_core.actions.manager import get_action_manager
from dcc_mcp_core.log_config import get_logger
from dcc_mcp_core.log_config import setup_dcc_logging
from dcc_mcp_core.log_config import setup_logging
from dcc_mcp_core.log_config import setup_rpyc_logging
from dcc_mcp_core.models import ActionResultModel
from dcc_mcp_core.utils import pydantic_extensions
from dcc_mcp_core.utils.dependency_injector import inject_dependencies
from dcc_mcp_core.utils.module_loader import convert_path_to_module
from dcc_mcp_core.utils.module_loader import load_module_from_path
from dcc_mcp_core.utils.result_factory import error_result
from dcc_mcp_core.utils.result_factory import from_exception
from dcc_mcp_core.utils.result_factory import success_result
from dcc_mcp_core.utils.result_factory import validate_action_result
from dcc_mcp_core.utils.type_wrappers import BaseWrapper
from dcc_mcp_core.utils.type_wrappers import BooleanWrapper
from dcc_mcp_core.utils.type_wrappers import FloatWrapper
from dcc_mcp_core.utils.type_wrappers import IntWrapper
from dcc_mcp_core.utils.type_wrappers import StringWrapper
from dcc_mcp_core.utils.type_wrappers import unwrap_parameters
from dcc_mcp_core.utils.type_wrappers import unwrap_value

__all__ = [
    # Action result models and factories
    "ActionResultModel",
    # Type wrappers
    "BaseWrapper",
    "BooleanWrapper",
    "FloatWrapper",
    "IntWrapper",
    "StringWrapper",
    # Core functionality
    "convert_path_to_module",
    "create_action_manager",
    "error_result",
    "from_exception",
    "get_action_manager",
    "get_logger",
    "inject_dependencies",
    "load_module_from_path",
    "models",
    "setup_dcc_logging",
    "setup_logging",
    "setup_rpyc_logging",
    "success_result",
    "unwrap_parameters",
    "unwrap_value",
    "validate_action_result",
]
