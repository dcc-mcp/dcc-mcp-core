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
from dcc_mcp_core._core import APP_AUTHOR
from dcc_mcp_core._core import APP_NAME
from dcc_mcp_core._core import DEFAULT_DCC
from dcc_mcp_core._core import DEFAULT_LOG_LEVEL
from dcc_mcp_core._core import DEFAULT_MIME_TYPE
from dcc_mcp_core._core import DEFAULT_VERSION
from dcc_mcp_core._core import ENV_LOG_LEVEL
from dcc_mcp_core._core import ENV_SKILL_PATHS
from dcc_mcp_core._core import SKILL_METADATA_DIR
from dcc_mcp_core._core import SKILL_METADATA_FILE
from dcc_mcp_core._core import SKILL_SCRIPTS_DIR

# Telemetry
from dcc_mcp_core._core import ActionDispatcher
from dcc_mcp_core._core import ActionMetrics
from dcc_mcp_core._core import ActionPipeline
from dcc_mcp_core._core import ActionRecorder
from dcc_mcp_core._core import ActionRegistry
from dcc_mcp_core._core import ActionResultModel
from dcc_mcp_core._core import ActionValidator

# Sandbox
from dcc_mcp_core._core import AuditEntry
from dcc_mcp_core._core import AuditLog
from dcc_mcp_core._core import AuditMiddleware
from dcc_mcp_core._core import BooleanWrapper
from dcc_mcp_core._core import BoundingBox
from dcc_mcp_core._core import CaptureFrame
from dcc_mcp_core._core import Capturer
from dcc_mcp_core._core import CaptureResult
from dcc_mcp_core._core import DccCapabilities
from dcc_mcp_core._core import DccError
from dcc_mcp_core._core import DccErrorCode
from dcc_mcp_core._core import DccInfo
from dcc_mcp_core._core import EventBus
from dcc_mcp_core._core import FloatWrapper
from dcc_mcp_core._core import FramedChannel
from dcc_mcp_core._core import FrameRange
from dcc_mcp_core._core import InputValidator
from dcc_mcp_core._core import IntWrapper
from dcc_mcp_core._core import IpcListener
from dcc_mcp_core._core import ListenerHandle
from dcc_mcp_core._core import LoggingMiddleware
from dcc_mcp_core._core import McpHttpConfig
from dcc_mcp_core._core import McpHttpServer
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
from dcc_mcp_core._core import PySceneDataKind
from dcc_mcp_core._core import PySharedBuffer
from dcc_mcp_core._core import PySharedSceneBuffer
from dcc_mcp_core._core import RateLimitMiddleware
from dcc_mcp_core._core import RecordingGuard
from dcc_mcp_core._core import RenderOutput
from dcc_mcp_core._core import ResourceAnnotations
from dcc_mcp_core._core import ResourceDefinition
from dcc_mcp_core._core import ResourceTemplateDefinition
from dcc_mcp_core._core import RoutingStrategy
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
from dcc_mcp_core._core import ServerHandle as McpServerHandle
from dcc_mcp_core._core import ServiceEntry
from dcc_mcp_core._core import ServiceStatus
from dcc_mcp_core._core import SkillCatalog
from dcc_mcp_core._core import SkillMetadata
from dcc_mcp_core._core import SkillScanner
from dcc_mcp_core._core import SkillSummary
from dcc_mcp_core._core import SkillWatcher
from dcc_mcp_core._core import StringWrapper
from dcc_mcp_core._core import TelemetryConfig
from dcc_mcp_core._core import TimingMiddleware
from dcc_mcp_core._core import ToolAnnotations
from dcc_mcp_core._core import ToolDeclaration
from dcc_mcp_core._core import ToolDefinition
from dcc_mcp_core._core import TransportAddress
from dcc_mcp_core._core import TransportManager
from dcc_mcp_core._core import TransportScheme
from dcc_mcp_core._core import UsdPrim
from dcc_mcp_core._core import UsdStage
from dcc_mcp_core._core import VersionConstraint
from dcc_mcp_core._core import VersionedRegistry
from dcc_mcp_core._core import VtValue
from dcc_mcp_core._core import connect_ipc
from dcc_mcp_core._core import create_skill_manager
from dcc_mcp_core._core import decode_envelope
from dcc_mcp_core._core import deserialize_result
from dcc_mcp_core._core import encode_notify
from dcc_mcp_core._core import encode_request
from dcc_mcp_core._core import encode_response
from dcc_mcp_core._core import error_result
from dcc_mcp_core._core import expand_transitive_dependencies
from dcc_mcp_core._core import from_exception
from dcc_mcp_core._core import get_actions_dir
from dcc_mcp_core._core import get_app_skill_paths_from_env
from dcc_mcp_core._core import get_config_dir
from dcc_mcp_core._core import get_data_dir
from dcc_mcp_core._core import get_log_dir
from dcc_mcp_core._core import get_platform_dir
from dcc_mcp_core._core import get_skill_paths_from_env
from dcc_mcp_core._core import get_skills_dir
from dcc_mcp_core._core import is_telemetry_initialized
from dcc_mcp_core._core import mpu_to_units
from dcc_mcp_core._core import parse_skill_md
from dcc_mcp_core._core import resolve_dependencies
from dcc_mcp_core._core import scan_and_load
from dcc_mcp_core._core import scan_and_load_lenient
from dcc_mcp_core._core import scan_skill_paths

# USD bridge functions
from dcc_mcp_core._core import scene_info_json_to_stage
from dcc_mcp_core._core import serialize_result
from dcc_mcp_core._core import shutdown_telemetry
from dcc_mcp_core._core import stage_to_scene_info_json
from dcc_mcp_core._core import success_result
from dcc_mcp_core._core import units_to_mpu
from dcc_mcp_core._core import unwrap_parameters
from dcc_mcp_core._core import unwrap_value
from dcc_mcp_core._core import validate_action_result
from dcc_mcp_core._core import validate_dependencies
from dcc_mcp_core._core import wrap_value

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
    "APP_AUTHOR",
    "APP_NAME",
    "DEFAULT_DCC",
    "DEFAULT_LOG_LEVEL",
    "DEFAULT_MIME_TYPE",
    "DEFAULT_VERSION",
    "ENV_LOG_LEVEL",
    "ENV_SKILL_PATHS",
    "SKILL_METADATA_DIR",
    "SKILL_METADATA_FILE",
    "SKILL_SCRIPTS_DIR",
    "ActionDispatcher",
    "ActionMetrics",
    "ActionPipeline",
    "ActionRecorder",
    "ActionRegistry",
    "ActionResultModel",
    "ActionValidator",
    "AuditEntry",
    "AuditLog",
    "AuditMiddleware",
    "BooleanWrapper",
    "BoundingBox",
    "CaptureFrame",
    "CaptureResult",
    "Capturer",
    "DccCapabilities",
    "DccError",
    "DccErrorCode",
    "DccInfo",
    "EventBus",
    "FloatWrapper",
    "FrameRange",
    "FramedChannel",
    "InputValidator",
    "IntWrapper",
    "IpcListener",
    "ListenerHandle",
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
    "PySceneDataKind",
    "PySharedBuffer",
    "PySharedSceneBuffer",
    "RateLimitMiddleware",
    "RecordingGuard",
    "RenderOutput",
    "ResourceAnnotations",
    "ResourceDefinition",
    "ResourceTemplateDefinition",
    "RoutingStrategy",
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
    "SkillMetadata",
    "SkillScanner",
    "SkillSummary",
    "SkillWatcher",
    "StringWrapper",
    "TelemetryConfig",
    "TimingMiddleware",
    "ToolAnnotations",
    "ToolDeclaration",
    "ToolDefinition",
    "TransportAddress",
    "TransportManager",
    "TransportScheme",
    "UsdPrim",
    "UsdStage",
    "VersionConstraint",
    "VersionedRegistry",
    "VtValue",
    "__author__",
    "__version__",
    "connect_ipc",
    "create_skill_manager",
    "decode_envelope",
    "deserialize_result",
    "encode_notify",
    "encode_request",
    "encode_response",
    "error_result",
    "expand_transitive_dependencies",
    "from_exception",
    "get_actions_dir",
    "get_app_skill_paths_from_env",
    # Pure-Python skill script helpers
    "get_bundled_skill_paths",
    "get_bundled_skills_dir",
    "get_config_dir",
    "get_data_dir",
    "get_log_dir",
    "get_platform_dir",
    "get_skill_paths_from_env",
    "get_skills_dir",
    "is_telemetry_initialized",
    "mpu_to_units",
    "parse_skill_md",
    "resolve_dependencies",
    "run_main",
    "scan_and_load",
    "scan_and_load_lenient",
    "scan_skill_paths",
    "scene_info_json_to_stage",
    "serialize_result",
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
    "validate_action_result",
    "validate_dependencies",
    "wrap_value",
]
