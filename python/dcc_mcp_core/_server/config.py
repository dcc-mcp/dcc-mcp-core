"""Server construction helpers for :class:`dcc_mcp_core.server_base.DccServerBase`.

This module keeps environment probing and ``McpHttpConfig`` assembly out of the
public server facade. The facade still owns orchestration; these helpers own the
construction contract.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
import os
from typing import TYPE_CHECKING
from typing import Any

from dcc_mcp_core._core import McpHttpConfig
from dcc_mcp_core._server.options import DccServerOptions
from dcc_mcp_core._server.options import DiagnosticsOptions
from dcc_mcp_core._server.options import ExecutionMode
from dcc_mcp_core._server.options import ObservabilityOptions
from dcc_mcp_core._server.options import _BridgeExecution
from dcc_mcp_core._server.options import _DispatcherExecution
from dcc_mcp_core._server.options import _StandaloneMainThreadExecution
from dcc_mcp_core._server.tools_list_policy import apply_tools_list_stub_policy

if TYPE_CHECKING:
    from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge


@dataclass(frozen=True)
class ObservabilityFlags:
    """Effective observability switches after runtime env overrides."""

    file_logging: bool
    job_persistence: bool
    telemetry: bool


@dataclass(frozen=True)
class DiagnosticsState:
    """Resolved DCC process/window state used by diagnostics."""

    dcc_pid: int
    window_title: str | None
    window_handle: int | None
    snapshot_provider: Any | None


@dataclass(frozen=True)
class ExecutionBinding:
    """Resolved host execution collaborators for one server instance."""

    bridge: HostExecutionBridge | None
    dispatcher: BaseDccCallableDispatcher | None
    standalone_main_thread: bool = False
    register_inprocess_executor: bool = False


CONTEXT_METADATA_ENV: dict[str, str] = {
    "context_bundle": "DCC_MCP_CONTEXT_BUNDLE",
    "production_domain": "DCC_MCP_PRODUCTION_DOMAIN",
    "context_kind": "DCC_MCP_CONTEXT_KIND",
    "project": "DCC_MCP_PROJECT",
    "sequence": "DCC_MCP_SEQUENCE",
    "shot": "DCC_MCP_SHOT",
    "asset": "DCC_MCP_ASSET",
    "asset_type": "DCC_MCP_ASSET_TYPE",
    "task": "DCC_MCP_TASK",
    "toolset_profile": "DCC_MCP_TOOLSET_PROFILE",
    "package_provenance": "DCC_MCP_PACKAGE_PROVENANCE",
    "skill_paths": "DCC_MCP_SKILL_PATHS",
    "resource_paths": "DCC_MCP_RESOURCE_PATHS",
    "prompt_paths": "DCC_MCP_PROMPT_PATHS",
}


def _env_enabled(disable_env_name: str) -> bool:
    return os.environ.get(disable_env_name, "0") != "1"


def resolve_observability_flags(options: ObservabilityOptions) -> ObservabilityFlags:
    """Return effective observability flags after env-var overrides."""
    return ObservabilityFlags(
        file_logging=options.enable_file_logging and _env_enabled("DCC_MCP_DISABLE_FILE_LOGGING"),
        job_persistence=options.enable_job_persistence and _env_enabled("DCC_MCP_DISABLE_JOB_PERSISTENCE"),
        telemetry=options.enable_telemetry and _env_enabled("DCC_MCP_DISABLE_TELEMETRY"),
    )


def resolve_diagnostics_state(options: DiagnosticsOptions) -> DiagnosticsState:
    """Return diagnostic process/window context with defaults resolved."""
    return DiagnosticsState(
        dcc_pid=options.dcc_pid if options.dcc_pid is not None else os.getpid(),
        window_title=options.window_title,
        window_handle=options.window_handle,
        snapshot_provider=options.snapshot_provider,
    )


def resolve_execution_binding(mode: ExecutionMode) -> ExecutionBinding:
    """Resolve the execution tagged union to concrete collaborators."""
    if isinstance(mode, _BridgeExecution):
        return ExecutionBinding(
            bridge=mode.bridge,
            dispatcher=mode.bridge.dispatcher,
            register_inprocess_executor=True,
        )
    if isinstance(mode, _DispatcherExecution):
        return ExecutionBinding(
            bridge=None,
            dispatcher=mode.dispatcher,
            register_inprocess_executor=True,
        )
    if isinstance(mode, _StandaloneMainThreadExecution):
        return ExecutionBinding(
            bridge=None,
            dispatcher=None,
            standalone_main_thread=True,
            register_inprocess_executor=True,
        )
    return ExecutionBinding(bridge=None, dispatcher=None)


def collect_context_metadata_from_env(dcc_name: str) -> dict[str, str]:
    """Collect Rez-resolved context metadata for gateway discovery."""
    metadata: dict[str, str] = {}
    for key, env_name in CONTEXT_METADATA_ENV.items():
        value = os.environ.get(env_name, "")
        if value:
            metadata[key] = value
    dcc_skill_paths = os.environ.get(f"DCC_MCP_{dcc_name.upper()}_SKILL_PATHS", "")
    if dcc_skill_paths:
        metadata["dcc_skill_paths"] = dcc_skill_paths
    return metadata


def build_mcp_http_config(
    options: DccServerOptions,
    *,
    package_version: str,
    version_provider: Callable[[], str],
) -> McpHttpConfig:
    """Build the ``McpHttpConfig`` for ``DccServerBase`` from resolved options."""
    config = McpHttpConfig(
        port=options.port,
        server_name=options.server_name or f"{options.dcc_name}-mcp",
        server_version=options.server_version if options.server_version is not None else package_version,
    )

    gateway = options.gateway
    # Explicit port (including 0 to disable) overrides the Rust default.
    if gateway.port is not None:
        config.gateway_port = gateway.port
    if gateway.registry_dir:
        config.registry_dir = gateway.registry_dir

    resolved_dcc_version = gateway.dcc_version if gateway.dcc_version is not None else version_provider()
    if resolved_dcc_version:
        config.dcc_version = resolved_dcc_version
    if gateway.scene:
        config.scene = gateway.scene

    config.dcc_type = options.dcc_name
    config.instance_metadata = collect_context_metadata_from_env(options.dcc_name)
    config.standalone_main_thread_execution = resolve_execution_binding(options.execution.mode).standalone_main_thread
    apply_tools_list_stub_policy(config, options.dcc_name)
    return config


__all__ = [
    "CONTEXT_METADATA_ENV",
    "DiagnosticsState",
    "ExecutionBinding",
    "ObservabilityFlags",
    "build_mcp_http_config",
    "collect_context_metadata_from_env",
    "resolve_diagnostics_state",
    "resolve_execution_binding",
    "resolve_observability_flags",
]
