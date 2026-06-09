"""Frozen options dataclasses for :class:`~dcc_mcp_core.server_base.DccServerBase`.

Replaces the 17-parameter constructor with a small hierarchy of frozen
dataclasses so every cross-cutting concern lives in one place:

- :class:`GatewayOptions`      — port, registry dir, scene, DCC version, failover
- :class:`ObservabilityOptions` — file logging, job persistence, telemetry
- :class:`DiagnosticsOptions`  — window PID/title/handle, snapshot provider
- :class:`ExecutionOptions`    — dispatcher vs execution bridge (tagged union)
- :class:`DccServerOptions`    — root object passed to ``DccServerBase.__init__``

Usage::

    from dcc_mcp_core.server_base.options import DccServerOptions

    # Minimal (required fields only):
    opts = DccServerOptions(dcc_name="blender", builtin_skills_dir=Path("/skills"))

    # With env-var resolution baked in (recommended):
    opts = DccServerOptions.from_env("maya", Path("/skills"))

    # With explicit overrides:
    opts = DccServerOptions.from_env("maya", Path("/skills"), port=9000)
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import os
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any
from typing import Union

if TYPE_CHECKING:
    from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge


# ---------------------------------------------------------------------------
# Sub-option groups
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class GatewayOptions:
    """Gateway election and registry configuration.

    Args:
        port: TCP port for the multi-DCC gateway competition.
            ``None`` reads ``DCC_MCP_GATEWAY_PORT`` at resolution time;
            ``0`` disables the gateway.
        registry_dir: Directory for the shared ``FileRegistry`` JSON file.
            ``None`` reads ``DCC_MCP_REGISTRY_DIR`` at resolution time.
        dcc_version: DCC application version string for the gateway registry.
            ``None`` means the server will call ``_version_string()`` at startup.
        scene: Currently open scene file path for the gateway registry.
        enable_failover: Enable automatic gateway failover / election.

    """

    port: int | None = None
    registry_dir: str | None = None
    dcc_version: str | None = None
    scene: str | None = None
    enable_failover: bool = True
    strict_gateway: bool = False

    @classmethod
    def from_env(
        cls,
        *,
        port: int | None = None,
        registry_dir: str | None = None,
        dcc_version: str | None = None,
        scene: str | None = None,
        enable_failover: bool = True,
        strict_gateway: bool = False,
    ) -> GatewayOptions:
        """Resolve gateway options, reading env-vars where parameters are ``None``.

        When ``port`` is ``None`` and ``DCC_MCP_GATEWAY_PORT`` is not set (or
        invalid), the result keeps ``port=None`` so downstream builders can
        fall back to the Rust-side default (9765).  Pass ``port=0`` explicitly
        to disable the gateway.

        ``DCC_MCP_STRICT_GATEWAY=1`` enables strict gateway mode:
        ``ensure_gateway_daemon()`` failures raise an exception instead of
        silently falling back to ``embedded-fallback`` mode.
        """
        resolved_port = port
        if resolved_port is None:
            env_val = os.environ.get("DCC_MCP_GATEWAY_PORT", "")
            resolved_port = int(env_val) if env_val.isdigit() else None

        resolved_registry_dir = registry_dir
        if resolved_registry_dir is None:
            resolved_registry_dir = os.environ.get("DCC_MCP_REGISTRY_DIR", "") or None

        resolved_strict = strict_gateway or (
            os.environ.get("DCC_MCP_STRICT_GATEWAY", "").strip().lower() in {"1", "true", "yes", "on"}
        )

        return cls(
            port=resolved_port,
            registry_dir=resolved_registry_dir,
            dcc_version=dcc_version,
            scene=scene,
            enable_failover=enable_failover,
            strict_gateway=resolved_strict,
        )


@dataclass(frozen=True)
class ObservabilityOptions:
    """File logging, job persistence, and telemetry configuration.

    All three flags can be overridden at runtime via env vars
    (``DCC_MCP_DISABLE_FILE_LOGGING``, ``DCC_MCP_DISABLE_JOB_PERSISTENCE``,
    ``DCC_MCP_DISABLE_TELEMETRY``).  The *effective* flag is the logical AND
    of the option and the absence of the env override — resolved at server
    startup, not here.
    """

    enable_file_logging: bool = True
    enable_job_persistence: bool = True
    enable_telemetry: bool = True


@dataclass(frozen=True)
class DiagnosticsOptions:
    """DCC process / window context used by diagnostic tools.

    Args:
        dcc_pid: Process ID of the DCC application.
            ``None`` resolves to ``os.getpid()`` at server startup.
        window_title: Substring of the DCC window title used to find the
            owner window for diagnostic screenshots.
        window_handle: Pre-resolved native window handle (HWND/XID).
            Takes precedence over PID/title lookup.
        snapshot_provider: Optional callable for post-tool context snapshots.

    """

    dcc_pid: int | None = None
    window_title: str | None = None
    window_handle: int | None = None
    snapshot_provider: Any | None = None


# ---------------------------------------------------------------------------
# Tagged union for execution mode
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class _InlineExecution:
    """Run skills inline on the calling thread (no dispatcher)."""

    kind: str = field(default="inline", init=False)


@dataclass(frozen=True)
class _DispatcherExecution:
    """Lightweight dispatcher-only execution (legacy shortcut)."""

    dispatcher: BaseDccCallableDispatcher
    kind: str = field(default="dispatcher", init=False)


@dataclass(frozen=True)
class _BridgeExecution:
    """Full :class:`HostExecutionBridge` (recommended for new adapters)."""

    bridge: HostExecutionBridge
    kind: str = field(default="bridge", init=False)


@dataclass(frozen=True)
class _StandaloneMainThreadExecution:
    """Run in-process skills inline and treat that lane as main-thread safe."""

    kind: str = field(default="standalone-main-thread", init=False)


#: Tagged union — only one of the three variants is valid at a time.
ExecutionMode = Union[
    _InlineExecution,
    _DispatcherExecution,
    _BridgeExecution,
    _StandaloneMainThreadExecution,
]

# Convenience constructors (avoids importing the private variants everywhere).
InlineExecution: _InlineExecution = _InlineExecution()
StandaloneMainThreadExecution: _StandaloneMainThreadExecution = _StandaloneMainThreadExecution()


def DispatcherExecution(dispatcher: BaseDccCallableDispatcher) -> _DispatcherExecution:
    """Return an execution mode that wraps ``dispatcher``."""
    return _DispatcherExecution(dispatcher=dispatcher)


def BridgeExecution(bridge: HostExecutionBridge) -> _BridgeExecution:
    """Return an execution mode that wraps ``bridge``."""
    return _BridgeExecution(bridge=bridge)


@dataclass(frozen=True)
class ExecutionOptions:
    """Execution mode selection.

    Args:
        mode: One of :data:`InlineExecution`,
            :data:`StandaloneMainThreadExecution`, ``DispatcherExecution(d)``,
            or ``BridgeExecution(b)``.  Defaults to :data:`InlineExecution`.

    """

    mode: ExecutionMode = field(default_factory=lambda: InlineExecution)


# ---------------------------------------------------------------------------
# Root options object
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class DccServerOptions:
    """Complete construction options for :class:`~dcc_mcp_core.server_base.DccServerBase`.

    Replaces the 17-parameter constructor.  All env-var resolution is
    centralised in :meth:`from_env` so there are no hidden side-effects
    inside ``__init__``.

    Args:
        dcc_name: Short DCC identifier (``"maya"``, ``"blender"``, …).
        builtin_skills_dir: Path to the adapter's bundled ``skills/`` directory.
        port: TCP port for the MCP HTTP server.  ``0`` → OS picks a free port.
        server_name: Name reported in the MCP ``initialize`` response.
        server_version: Version reported in the MCP ``initialize`` response.
            ``None`` defaults to the installed ``dcc_mcp_core`` version.
        gateway: :class:`GatewayOptions` instance.
        observability: :class:`ObservabilityOptions` instance.
        diagnostics: :class:`DiagnosticsOptions` instance.
        execution: :class:`ExecutionOptions` instance.

    """

    dcc_name: str
    builtin_skills_dir: Path
    port: int = 8765
    server_name: str | None = None
    server_version: str | None = None
    gateway: GatewayOptions = field(default_factory=GatewayOptions)
    observability: ObservabilityOptions = field(default_factory=ObservabilityOptions)
    diagnostics: DiagnosticsOptions = field(default_factory=DiagnosticsOptions)
    execution: ExecutionOptions = field(default_factory=ExecutionOptions)

    @classmethod
    def from_env(
        cls,
        dcc_name: str,
        builtin_skills_dir: Path,
        *,
        port: int = 8765,
        server_name: str | None = None,
        server_version: str | None = None,
        # gateway kwargs
        gateway_port: int | None = None,
        registry_dir: str | None = None,
        dcc_version: str | None = None,
        scene: str | None = None,
        enable_gateway_failover: bool = True,
        strict_gateway: bool = False,
        # observability kwargs
        enable_file_logging: bool = True,
        enable_job_persistence: bool = True,
        enable_telemetry: bool = True,
        # diagnostics kwargs
        dcc_pid: int | None = None,
        dcc_window_title: str | None = None,
        dcc_window_handle: int | None = None,
        snapshot_provider: Any | None = None,
        # execution kwargs
        dispatcher: BaseDccCallableDispatcher | None = None,
        execution_bridge: HostExecutionBridge | None = None,
        standalone_main_thread: bool = False,
    ) -> DccServerOptions:
        """Build a :class:`DccServerOptions` from keyword arguments + env vars.

        This is the **recommended** constructor for all adapters.  Env-var
        resolution for gateway port and registry directory happens here once,
        producing a fully-resolved frozen object.

        Raises:
            ValueError: If more than one execution mode is provided.

        """
        if dispatcher is not None and execution_bridge is not None:
            raise ValueError("Pass either dispatcher or execution_bridge, not both")
        if standalone_main_thread and (dispatcher is not None or execution_bridge is not None):
            raise ValueError("standalone_main_thread cannot be combined with dispatcher or execution_bridge")

        gateway = GatewayOptions.from_env(
            port=gateway_port,
            registry_dir=registry_dir,
            dcc_version=dcc_version,
            scene=scene,
            enable_failover=enable_gateway_failover,
            strict_gateway=strict_gateway,
        )
        observability = ObservabilityOptions(
            enable_file_logging=enable_file_logging,
            enable_job_persistence=enable_job_persistence,
            enable_telemetry=enable_telemetry,
        )
        diagnostics = DiagnosticsOptions(
            dcc_pid=dcc_pid,
            window_title=dcc_window_title,
            window_handle=dcc_window_handle,
            snapshot_provider=snapshot_provider,
        )

        if execution_bridge is not None:
            exec_mode: ExecutionMode = BridgeExecution(execution_bridge)
        elif dispatcher is not None:
            exec_mode = DispatcherExecution(dispatcher)
        elif standalone_main_thread:
            exec_mode = StandaloneMainThreadExecution
        else:
            exec_mode = InlineExecution

        execution = ExecutionOptions(mode=exec_mode)

        return cls(
            dcc_name=dcc_name,
            builtin_skills_dir=builtin_skills_dir,
            port=port,
            server_name=server_name,
            server_version=server_version,
            gateway=gateway,
            observability=observability,
            diagnostics=diagnostics,
            execution=execution,
        )
