"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem.

This package is powered by a Rust core via PyO3. The native extension module
``dcc_mcp_core._core`` is compiled by maturin and provides all public APIs.

The pure-Python ``dcc_mcp_core.skill`` sub-module provides lightweight helpers
for skill script authors — no compiled extension required:

.. code-block:: python

    from dcc_mcp_core.skill import skill_entry, skill_success, skill_error
"""

# Import future modules
from __future__ import annotations

# Import local modules
from dcc_mcp_core import _core

# Naming validators (SEP-986)
from dcc_mcp_core._core import ACTION_ID_RE
from dcc_mcp_core._core import APP_AUTHOR
from dcc_mcp_core._core import APP_NAME
from dcc_mcp_core._core import DEFAULT_DCC
from dcc_mcp_core._core import DEFAULT_LOG_FILE_PREFIX
from dcc_mcp_core._core import DEFAULT_LOG_LEVEL
from dcc_mcp_core._core import DEFAULT_LOG_MAX_FILES
from dcc_mcp_core._core import DEFAULT_LOG_MAX_SIZE
from dcc_mcp_core._core import DEFAULT_LOG_ROTATION
from dcc_mcp_core._core import DEFAULT_MIME_TYPE
from dcc_mcp_core._core import DEFAULT_VERSION
from dcc_mcp_core._core import ENV_LOG_DIR
from dcc_mcp_core._core import ENV_LOG_FILE
from dcc_mcp_core._core import ENV_LOG_FILE_PREFIX
from dcc_mcp_core._core import ENV_LOG_LEVEL
from dcc_mcp_core._core import ENV_LOG_MAX_FILES
from dcc_mcp_core._core import ENV_LOG_MAX_SIZE
from dcc_mcp_core._core import ENV_LOG_ROTATION
from dcc_mcp_core._core import ENV_SKILL_PATHS
from dcc_mcp_core._core import MAX_TOOL_NAME_LEN
from dcc_mcp_core._core import SKILL_METADATA_DIR
from dcc_mcp_core._core import SKILL_METADATA_FILE
from dcc_mcp_core._core import SKILL_SCRIPTS_DIR
from dcc_mcp_core._core import TOOL_NAME_RE

# Sandbox
from dcc_mcp_core._core import AuditEntry
from dcc_mcp_core._core import AuditLog
from dcc_mcp_core._core import AuditMiddleware
from dcc_mcp_core._core import BooleanWrapper
from dcc_mcp_core._core import BoundingBox
from dcc_mcp_core._core import BridgeContext
from dcc_mcp_core._core import BridgeRegistry
from dcc_mcp_core._core import CaptureBackendKind
from dcc_mcp_core._core import CaptureFrame
from dcc_mcp_core._core import Capturer
from dcc_mcp_core._core import CaptureResult
from dcc_mcp_core._core import CaptureTarget
from dcc_mcp_core._core import DccCapabilities
from dcc_mcp_core._core import DccError
from dcc_mcp_core._core import DccErrorCode
from dcc_mcp_core._core import DccInfo
from dcc_mcp_core._core import DccLinkFrame
from dcc_mcp_core._core import EventBus
from dcc_mcp_core._core import FileLoggingConfig

# Artefact hand-off (issue #349)
from dcc_mcp_core._core import FileRef
from dcc_mcp_core._core import FloatWrapper
from dcc_mcp_core._core import FrameRange
from dcc_mcp_core._core import GracefulIpcChannelAdapter
from dcc_mcp_core._core import InputValidator
from dcc_mcp_core._core import IntWrapper
from dcc_mcp_core._core import IpcChannelAdapter
from dcc_mcp_core._core import LoggingMiddleware
from dcc_mcp_core._core import McpHttpConfig
from dcc_mcp_core._core import McpHttpServer
from dcc_mcp_core._core import McpServerHandle
from dcc_mcp_core._core import ObjectTransform
from dcc_mcp_core._core import PromptArgument
from dcc_mcp_core._core import PromptDefinition

# Shared memory
from dcc_mcp_core._core import PyBufferPool

# Process management
from dcc_mcp_core._core import PyCrashRecoveryPolicy
from dcc_mcp_core._core import PyDccLauncher
from dcc_mcp_core._core import PyProcessMonitor
from dcc_mcp_core._core import PyProcessWatcher
from dcc_mcp_core._core import PyPumpedDispatcher
from dcc_mcp_core._core import PySceneDataKind
from dcc_mcp_core._core import PySharedBuffer
from dcc_mcp_core._core import PySharedSceneBuffer
from dcc_mcp_core._core import PyStandaloneDispatcher
from dcc_mcp_core._core import RateLimitMiddleware
from dcc_mcp_core._core import RecordingGuard
from dcc_mcp_core._core import RenderOutput
from dcc_mcp_core._core import ResourceAnnotations
from dcc_mcp_core._core import ResourceDefinition
from dcc_mcp_core._core import ResourceTemplateDefinition
from dcc_mcp_core._core import SandboxContext
from dcc_mcp_core._core import SandboxPolicy
from dcc_mcp_core._core import SceneInfo
from dcc_mcp_core._core import SceneNode
from dcc_mcp_core._core import SceneObject
from dcc_mcp_core._core import SceneStatistics
from dcc_mcp_core._core import ScriptLanguage
from dcc_mcp_core._core import ScriptResult

# USD scene description
from dcc_mcp_core._core import SdfPath

# Action Version Management
from dcc_mcp_core._core import SemVer

# Serialization
from dcc_mcp_core._core import SerializeFormat
from dcc_mcp_core._core import ServiceEntry
from dcc_mcp_core._core import ServiceStatus
from dcc_mcp_core._core import SkillCatalog
from dcc_mcp_core._core import SkillGroup
from dcc_mcp_core._core import SkillMetadata
from dcc_mcp_core._core import SkillScanner
from dcc_mcp_core._core import SkillSummary
from dcc_mcp_core._core import SkillWatcher
from dcc_mcp_core._core import SocketServerAdapter
from dcc_mcp_core._core import StringWrapper
from dcc_mcp_core._core import TelemetryConfig
from dcc_mcp_core._core import TimingMiddleware
from dcc_mcp_core._core import ToolAnnotations
from dcc_mcp_core._core import ToolDeclaration
from dcc_mcp_core._core import ToolDefinition

# Telemetry
from dcc_mcp_core._core import ToolDispatcher
from dcc_mcp_core._core import ToolMetrics
from dcc_mcp_core._core import ToolPipeline
from dcc_mcp_core._core import ToolRecorder
from dcc_mcp_core._core import ToolRegistry
from dcc_mcp_core._core import ToolResult
from dcc_mcp_core._core import ToolValidator
from dcc_mcp_core._core import TransportAddress
from dcc_mcp_core._core import TransportScheme
from dcc_mcp_core._core import UsdPrim
from dcc_mcp_core._core import UsdStage
from dcc_mcp_core._core import VersionConstraint
from dcc_mcp_core._core import VersionedRegistry
from dcc_mcp_core._core import VtValue
from dcc_mcp_core._core import WindowFinder
from dcc_mcp_core._core import WindowInfo
from dcc_mcp_core._core import artefact_get_bytes
from dcc_mcp_core._core import artefact_list
from dcc_mcp_core._core import artefact_put_bytes
from dcc_mcp_core._core import artefact_put_file
from dcc_mcp_core._core import create_skill_server
from dcc_mcp_core._core import deserialize_result
from dcc_mcp_core._core import error_result
from dcc_mcp_core._core import expand_transitive_dependencies
from dcc_mcp_core._core import from_exception
from dcc_mcp_core._core import gc_orphans
from dcc_mcp_core._core import get_app_skill_paths_from_env
from dcc_mcp_core._core import get_bridge_context
from dcc_mcp_core._core import get_config_dir
from dcc_mcp_core._core import get_data_dir
from dcc_mcp_core._core import get_log_dir
from dcc_mcp_core._core import get_platform_dir
from dcc_mcp_core._core import get_skill_paths_from_env
from dcc_mcp_core._core import get_skills_dir
from dcc_mcp_core._core import get_tools_dir
from dcc_mcp_core._core import init_file_logging
from dcc_mcp_core._core import is_telemetry_initialized
from dcc_mcp_core._core import mpu_to_units
from dcc_mcp_core._core import parse_skill_md
from dcc_mcp_core._core import register_bridge
from dcc_mcp_core._core import resolve_dependencies
from dcc_mcp_core._core import scan_and_load
from dcc_mcp_core._core import scan_and_load_lenient
from dcc_mcp_core._core import scan_skill_paths

# USD bridge functions
from dcc_mcp_core._core import scene_info_json_to_stage
from dcc_mcp_core._core import serialize_result
from dcc_mcp_core._core import shutdown_file_logging
from dcc_mcp_core._core import shutdown_telemetry
from dcc_mcp_core._core import stage_to_scene_info_json
from dcc_mcp_core._core import success_result
from dcc_mcp_core._core import units_to_mpu
from dcc_mcp_core._core import unwrap_parameters
from dcc_mcp_core._core import unwrap_value
from dcc_mcp_core._core import validate_action_id
from dcc_mcp_core._core import validate_action_result
from dcc_mcp_core._core import validate_dependencies
from dcc_mcp_core._core import validate_tool_name
from dcc_mcp_core._core import wrap_value

# Workflow primitive — optional (Cargo `workflow` feature, issue #348 skeleton).
# Step execution is stubbed; only WorkflowSpec/WorkflowStatus (parse+validate)
# are Python-visible here.
try:
    from dcc_mcp_core._core import BackoffKind  # type: ignore[attr-defined]
    from dcc_mcp_core._core import RetryPolicy  # type: ignore[attr-defined]
    from dcc_mcp_core._core import StepPolicy  # type: ignore[attr-defined]
    from dcc_mcp_core._core import WorkflowSpec  # type: ignore[attr-defined]
    from dcc_mcp_core._core import WorkflowStatus  # type: ignore[attr-defined]
    from dcc_mcp_core._core import WorkflowStep  # type: ignore[attr-defined]
except ImportError:  # pragma: no cover — feature off
    BackoffKind = None  # type: ignore[assignment,misc]
    RetryPolicy = None  # type: ignore[assignment,misc]
    StepPolicy = None  # type: ignore[assignment,misc]
    WorkflowSpec = None  # type: ignore[assignment,misc]
    WorkflowStatus = None  # type: ignore[assignment,misc]
    WorkflowStep = None  # type: ignore[assignment,misc]

# Adapters (pure-Python, non-DccServerBase)
from dcc_mcp_core.adapters import CAPABILITY_KEYS
from dcc_mcp_core.adapters import WEBVIEW_DEFAULT_CAPABILITIES
from dcc_mcp_core.adapters import WebViewAdapter
from dcc_mcp_core.adapters import WebViewContext

# Pure-Python DCC adapter base classes (no _core dependency)
from dcc_mcp_core.bridge import BridgeConnectionError
from dcc_mcp_core.bridge import BridgeError
from dcc_mcp_core.bridge import BridgeRpcError
from dcc_mcp_core.bridge import BridgeTimeoutError
from dcc_mcp_core.bridge import DccBridge

# Cooperative cancellation (pure-Python, no _core dependency)
from dcc_mcp_core.cancellation import CancelledError
from dcc_mcp_core.cancellation import CancelToken
from dcc_mcp_core.cancellation import check_cancelled
from dcc_mcp_core.cancellation import current_cancel_token
from dcc_mcp_core.cancellation import reset_cancel_token
from dcc_mcp_core.cancellation import set_cancel_token

# Pure-Python DCC server diagnostic helpers (no _core dependency)
from dcc_mcp_core.dcc_server import register_diagnostic_handlers
from dcc_mcp_core.dcc_server import register_diagnostic_mcp_tools
from dcc_mcp_core.factory import create_dcc_server
from dcc_mcp_core.factory import get_server_instance
from dcc_mcp_core.factory import make_start_stop
from dcc_mcp_core.gateway_election import DccGatewayElection
from dcc_mcp_core.hotreload import DccSkillHotReloader
from dcc_mcp_core.server_base import DccServerBase

# Pure-Python skill script helpers (no _core dependency)
from dcc_mcp_core.skill import get_bundled_skill_paths
from dcc_mcp_core.skill import get_bundled_skills_dir
from dcc_mcp_core.skill import run_main
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_exception
from dcc_mcp_core.skill import skill_success
from dcc_mcp_core.skill import skill_warning

__version__: str
try:
    __version__ = _core.__version__  # type: ignore[attr-defined]
except AttributeError:
    __version__ = "0.0.0-dev"

__author__: str
try:
    __author__ = _core.__author__  # type: ignore[attr-defined]
except AttributeError:
    __author__ = ""

__all__ = [
    "ACTION_ID_RE",
    "APP_AUTHOR",
    "APP_NAME",
    "CAPABILITY_KEYS",
    "DEFAULT_DCC",
    "DEFAULT_LOG_FILE_PREFIX",
    "DEFAULT_LOG_LEVEL",
    "DEFAULT_LOG_MAX_FILES",
    "DEFAULT_LOG_MAX_SIZE",
    "DEFAULT_LOG_ROTATION",
    "DEFAULT_MIME_TYPE",
    "DEFAULT_VERSION",
    "ENV_LOG_DIR",
    "ENV_LOG_FILE",
    "ENV_LOG_FILE_PREFIX",
    "ENV_LOG_LEVEL",
    "ENV_LOG_MAX_FILES",
    "ENV_LOG_MAX_SIZE",
    "ENV_LOG_ROTATION",
    "ENV_SKILL_PATHS",
    "MAX_TOOL_NAME_LEN",
    "SKILL_METADATA_DIR",
    "SKILL_METADATA_FILE",
    "SKILL_SCRIPTS_DIR",
    "TOOL_NAME_RE",
    "WEBVIEW_DEFAULT_CAPABILITIES",
    "AuditEntry",
    "AuditLog",
    "AuditMiddleware",
    "BackoffKind",
    "BooleanWrapper",
    "BoundingBox",
    "BridgeConnectionError",
    "BridgeContext",
    "BridgeError",
    "BridgeRegistry",
    "BridgeRpcError",
    "BridgeTimeoutError",
    "CancelToken",
    "CancelledError",
    "CaptureBackendKind",
    "CaptureFrame",
    "CaptureResult",
    "CaptureTarget",
    "Capturer",
    "DccBridge",
    "DccCapabilities",
    "DccError",
    "DccErrorCode",
    "DccGatewayElection",
    "DccInfo",
    "DccLinkFrame",
    "DccServerBase",
    "DccSkillHotReloader",
    "EventBus",
    "FileLoggingConfig",
    "FileRef",
    "FloatWrapper",
    "FrameRange",
    "GracefulIpcChannelAdapter",
    "InputValidator",
    "IntWrapper",
    "IpcChannelAdapter",
    "LoggingMiddleware",
    "McpHttpConfig",
    "McpHttpServer",
    "McpServerHandle",
    "ObjectTransform",
    "PromptArgument",
    "PromptDefinition",
    "PyBufferPool",
    "PyCrashRecoveryPolicy",
    "PyDccLauncher",
    "PyProcessMonitor",
    "PyProcessWatcher",
    "PyPumpedDispatcher",
    "PySceneDataKind",
    "PySharedBuffer",
    "PySharedSceneBuffer",
    "PyStandaloneDispatcher",
    "RateLimitMiddleware",
    "RecordingGuard",
    "RenderOutput",
    "ResourceAnnotations",
    "ResourceDefinition",
    "ResourceTemplateDefinition",
    "RetryPolicy",
    "SandboxContext",
    "SandboxPolicy",
    "SceneInfo",
    "SceneNode",
    "SceneObject",
    "SceneStatistics",
    "ScriptLanguage",
    "ScriptResult",
    "SdfPath",
    "SemVer",
    # Serialization
    "SerializeFormat",
    "ServiceEntry",
    "ServiceStatus",
    "SkillCatalog",
    "SkillGroup",
    "SkillMetadata",
    "SkillScanner",
    "SkillSummary",
    "SkillWatcher",
    "SocketServerAdapter",
    "StepPolicy",
    "StringWrapper",
    "TelemetryConfig",
    "TimingMiddleware",
    "ToolAnnotations",
    "ToolDeclaration",
    "ToolDefinition",
    "ToolDispatcher",
    "ToolMetrics",
    "ToolPipeline",
    "ToolRecorder",
    "ToolRegistry",
    "ToolResult",
    "ToolValidator",
    "TransportAddress",
    "TransportScheme",
    "UsdPrim",
    "UsdStage",
    "VersionConstraint",
    "VersionedRegistry",
    "VtValue",
    "WebViewAdapter",
    "WebViewContext",
    "WindowFinder",
    "WindowInfo",
    "WorkflowSpec",
    "WorkflowStatus",
    "WorkflowStep",
    "__author__",
    "__version__",
    "artefact_get_bytes",
    "artefact_list",
    "artefact_put_bytes",
    "artefact_put_file",
    "check_cancelled",
    "create_dcc_server",
    "create_skill_server",
    "current_cancel_token",
    "deserialize_result",
    "error_result",
    "expand_transitive_dependencies",
    "from_exception",
    "gc_orphans",
    "get_app_skill_paths_from_env",
    "get_bridge_context",
    "get_bundled_skill_paths",
    "get_bundled_skills_dir",
    "get_config_dir",
    "get_data_dir",
    "get_log_dir",
    "get_platform_dir",
    "get_server_instance",
    "get_skill_paths_from_env",
    "get_skills_dir",
    "get_tools_dir",
    "init_file_logging",
    "is_telemetry_initialized",
    "make_start_stop",
    "mpu_to_units",
    "parse_skill_md",
    "register_bridge",
    "register_diagnostic_handlers",
    "register_diagnostic_mcp_tools",
    "reset_cancel_token",
    "resolve_dependencies",
    "run_main",
    "scan_and_load",
    "scan_and_load_lenient",
    "scan_skill_paths",
    "scene_info_json_to_stage",
    "serialize_result",
    "set_cancel_token",
    "shutdown_file_logging",
    "shutdown_telemetry",
    "skill_entry",
    "skill_error",
    "skill_exception",
    "skill_success",
    "skill_warning",
    "stage_to_scene_info_json",
    "success_result",
    "units_to_mpu",
    "unwrap_parameters",
    "unwrap_value",
    "validate_action_id",
    "validate_action_result",
    "validate_dependencies",
    "validate_tool_name",
    "wrap_value",
]
