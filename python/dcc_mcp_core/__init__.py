"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem.

This package is powered by a Rust core via PyO3. The native extension module
``dcc_mcp_core._core`` is compiled by maturin and provides all public APIs.
"""

from dcc_mcp_core._core import (  # noqa: F401
    # Models
    ActionResultModel,
    SkillMetadata,
    # Factory functions
    error_result,
    from_exception,
    success_result,
    validate_action_result,
    # Actions
    ActionRegistry,
    EventBus,
    # Protocol types
    PromptArgument,
    PromptDefinition,
    ResourceDefinition,
    ResourceTemplateDefinition,
    ToolAnnotations,
    ToolDefinition,
    # Skills
    SkillScanner,
    scan_skill_paths,
    # Utils: filesystem
    get_actions_dir,
    get_config_dir,
    get_data_dir,
    get_log_dir,
    get_platform_dir,
    get_skill_paths_from_env,
    get_skills_dir,
    # Utils: type wrappers
    BooleanWrapper,
    FloatWrapper,
    IntWrapper,
    StringWrapper,
    unwrap_parameters,
    unwrap_value,
    wrap_value,
    # Constants
    APP_AUTHOR,
    APP_NAME,
    DEFAULT_DCC,
    DEFAULT_LOG_LEVEL,
    ENV_LOG_LEVEL,
    ENV_SKILL_PATHS,
    SKILL_METADATA_FILE,
    SKILL_SCRIPTS_DIR,
)

__version__: str
try:
    __version__ = _core.__version__  # type: ignore[attr-defined]  # noqa: F811
except Exception:
    __version__ = "0.0.0-dev"

__all__ = [
    # Models
    "ActionResultModel",
    "SkillMetadata",
    # Factory
    "error_result",
    "from_exception",
    "success_result",
    "validate_action_result",
    # Actions
    "ActionRegistry",
    "EventBus",
    # Protocol types
    "PromptArgument",
    "PromptDefinition",
    "ResourceDefinition",
    "ResourceTemplateDefinition",
    "ToolAnnotations",
    "ToolDefinition",
    # Skills
    "SkillScanner",
    "scan_skill_paths",
    # Filesystem
    "get_actions_dir",
    "get_config_dir",
    "get_data_dir",
    "get_log_dir",
    "get_platform_dir",
    "get_skill_paths_from_env",
    "get_skills_dir",
    # Type wrappers
    "BooleanWrapper",
    "FloatWrapper",
    "IntWrapper",
    "StringWrapper",
    "unwrap_parameters",
    "unwrap_value",
    "wrap_value",
    # Constants
    "APP_AUTHOR",
    "APP_NAME",
    "DEFAULT_DCC",
    "DEFAULT_LOG_LEVEL",
    "ENV_LOG_LEVEL",
    "ENV_SKILL_PATHS",
    "SKILL_METADATA_FILE",
    "SKILL_SCRIPTS_DIR",
]
