"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem.

This package is powered by a Rust core via PyO3. The native extension module
``dcc_mcp_core._core`` is compiled by maturin and provides all public APIs.
"""

# Import future modules
from __future__ import annotations

# Import local modules
from dcc_mcp_core import _core
from dcc_mcp_core._core import APP_AUTHOR
from dcc_mcp_core._core import APP_NAME
from dcc_mcp_core._core import DEFAULT_DCC
from dcc_mcp_core._core import DEFAULT_LOG_LEVEL
from dcc_mcp_core._core import ENV_LOG_LEVEL
from dcc_mcp_core._core import ENV_SKILL_PATHS
from dcc_mcp_core._core import SKILL_METADATA_FILE
from dcc_mcp_core._core import SKILL_SCRIPTS_DIR
from dcc_mcp_core._core import ActionRegistry
from dcc_mcp_core._core import ActionResultModel
from dcc_mcp_core._core import BooleanWrapper
from dcc_mcp_core._core import EventBus
from dcc_mcp_core._core import FloatWrapper
from dcc_mcp_core._core import IntWrapper
from dcc_mcp_core._core import PromptArgument
from dcc_mcp_core._core import PromptDefinition
from dcc_mcp_core._core import ResourceDefinition
from dcc_mcp_core._core import ResourceTemplateDefinition
from dcc_mcp_core._core import ServiceEntry
from dcc_mcp_core._core import ServiceStatus
from dcc_mcp_core._core import SkillMetadata
from dcc_mcp_core._core import SkillScanner
from dcc_mcp_core._core import StringWrapper
from dcc_mcp_core._core import ToolAnnotations
from dcc_mcp_core._core import ToolDefinition
from dcc_mcp_core._core import TransportManager
from dcc_mcp_core._core import error_result
from dcc_mcp_core._core import from_exception
from dcc_mcp_core._core import get_actions_dir
from dcc_mcp_core._core import get_config_dir
from dcc_mcp_core._core import get_data_dir
from dcc_mcp_core._core import get_log_dir
from dcc_mcp_core._core import get_platform_dir
from dcc_mcp_core._core import get_skill_paths_from_env
from dcc_mcp_core._core import get_skills_dir
from dcc_mcp_core._core import parse_skill_md
from dcc_mcp_core._core import scan_skill_paths
from dcc_mcp_core._core import success_result
from dcc_mcp_core._core import unwrap_parameters
from dcc_mcp_core._core import unwrap_value
from dcc_mcp_core._core import validate_action_result
from dcc_mcp_core._core import wrap_value

__version__: str
try:
    __version__ = _core.__version__  # type: ignore[attr-defined]
except AttributeError:
    __version__ = "0.0.0-dev"

__all__ = [
    "APP_AUTHOR",
    "APP_NAME",
    "DEFAULT_DCC",
    "DEFAULT_LOG_LEVEL",
    "ENV_LOG_LEVEL",
    "ENV_SKILL_PATHS",
    "SKILL_METADATA_FILE",
    "SKILL_SCRIPTS_DIR",
    "ActionRegistry",
    "ActionResultModel",
    "BooleanWrapper",
    "EventBus",
    "FloatWrapper",
    "IntWrapper",
    "PromptArgument",
    "PromptDefinition",
    "ResourceDefinition",
    "ResourceTemplateDefinition",
    "ServiceEntry",
    "ServiceStatus",
    "SkillMetadata",
    "SkillScanner",
    "StringWrapper",
    "ToolAnnotations",
    "ToolDefinition",
    "TransportManager",
    "__version__",
    "error_result",
    "from_exception",
    "get_actions_dir",
    "get_config_dir",
    "get_data_dir",
    "get_log_dir",
    "get_platform_dir",
    "get_skill_paths_from_env",
    "get_skills_dir",
    "parse_skill_md",
    "scan_skill_paths",
    "success_result",
    "unwrap_parameters",
    "unwrap_value",
    "validate_action_result",
    "wrap_value",
]
