"""Frozen options dataclasses for :class:`~dcc_mcp_core.server_base.DccServerBase`.

Replaces the 17-parameter constructor with a small hierarchy of frozen
dataclasses so every cross-cutting concern lives in one place:

- :class:`GatewayOptions`      — port, registry dir, scene, DCC version, failover
- :class:`ObservabilityOptions` — file logging, job persistence, telemetry
- :class:`DiagnosticsOptions`  — window PID/title/handle, snapshot provider
- :class:`ExecutionOptions`    — dispatcher vs execution bridge (tagged union)
- :class:`DccServerOptions`    — runtime identity plus the root server options

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
class SidecarOptions:
    """Sidecar launch contract for py37-lite / no-_core adapters.

    When the compiled ``_core`` extension is unavailable, ``DccServerBase``
    delegates HTTP/MCP serving to ``dcc-mcp-server sidecar`` and routes tool
    dispatch through ``host_rpc``.
    """

    host_rpc: str | None = None
    adapter_version: str | None = None
    display_name: str | None = None
    wait_ready_timeout_secs: float = 15.0
    server_bin: str | None = None
    extra_args: tuple[str, ...] = ()

    @classmethod
    def from_env(
        cls,
        *,
        host_rpc: str | None = None,
        adapter_version: str | None = None,
        display_name: str | None = None,
        wait_ready_timeout_secs: float = 15.0,
        server_bin: str | None = None,
        extra_args: tuple[str, ...] = (),
    ) -> SidecarOptions:
        resolved_host_rpc = host_rpc
        if resolved_host_rpc is None:
            resolved_host_rpc = os.environ.get("DCC_MCP_HOST_RPC", "") or None
        resolved_server_bin = server_bin
        if resolved_server_bin is None:
            resolved_server_bin = os.environ.get("DCC_MCP_SERVER_BIN", "") or None
        return cls(
            host_rpc=resolved_host_rpc,
            adapter_version=adapter_version,
            display_name=display_name,
            wait_ready_timeout_secs=wait_ready_timeout_secs,
            server_bin=resolved_server_bin,
            extra_args=extra_args,
        )


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
        port: Resolved TCP port for the MCP HTTP server. ``0`` asks the OS for
            a free port. :meth:`from_env` accepts ``None`` to resolve
            ``DCC_MCP_<DCC>_PORT`` before falling back to ``0``.
        server_name: Name reported in the MCP ``initialize`` response.
        server_version: Version reported in the MCP ``initialize`` response.
            ``None`` defaults to the installed ``dcc_mcp_core`` version.
        instance_type: Runtime lifetime shape reported to discovery. Use
            ``"gui"`` for a DCC-bound adapter and ``"standalone"`` for a
            headless service whose own process is the complete lifetime. This
            is independent from the tool execution/threading mode.
        gateway: :class:`GatewayOptions` instance.
        observability: :class:`ObservabilityOptions` instance.
        diagnostics: :class:`DiagnosticsOptions` instance.
        execution: :class:`ExecutionOptions` instance.

    """

    dcc_name: str
    builtin_skills_dir: Path
    port: int = 0
    server_name: str | None = None
    server_version: str | None = None
    gateway: GatewayOptions = field(default_factory=GatewayOptions)
    observability: ObservabilityOptions = field(default_factory=ObservabilityOptions)
    diagnostics: DiagnosticsOptions = field(default_factory=DiagnosticsOptions)
    execution: ExecutionOptions = field(default_factory=ExecutionOptions)
    sidecar: SidecarOptions = field(default_factory=SidecarOptions)
    # Appended to preserve the public dataclass's historical positional order.
    instance_type: str | None = None

    def __post_init__(self) -> None:
        if self.instance_type is None:
            return
        normalized = self.instance_type.strip().lower()
        if normalized not in {"gui", "standalone"}:
            raise ValueError("instance_type must be 'gui' or 'standalone'")
        object.__setattr__(self, "instance_type", normalized)

    @classmethod
    def from_env(
        cls,
        dcc_name: str,
        builtin_skills_dir: Path,
        *,
        port: int | None = None,
        server_name: str | None = None,
        server_version: str | None = None,
        instance_type: str | None = None,
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
        # sidecar kwargs (py37-lite)
        host_rpc: str | None = None,
        adapter_version: str | None = None,
        sidecar_display_name: str | None = None,
        wait_ready_timeout_secs: float = 15.0,
    ) -> DccServerOptions:
        """Build a :class:`DccServerOptions` from keyword arguments + env vars.

        This is the **recommended** constructor for all adapters. Env-var
        resolution for the DCC instance port, gateway port, and registry
        directory happens here once, producing a fully-resolved frozen object.

        Raises:
            ValueError: If more than one execution mode is provided or the
                runtime instance type is invalid.

        """
        if dispatcher is not None and execution_bridge is not None:
            raise ValueError("Pass either dispatcher or execution_bridge, not both")
        if standalone_main_thread and (dispatcher is not None or execution_bridge is not None):
            raise ValueError("standalone_main_thread cannot be combined with dispatcher or execution_bridge")

        port_env = "DCC_MCP_{}_PORT".format(
            "".join(character if character.isalnum() else "_" for character in dcc_name.upper())
        )
        raw_port: object = port if port is not None else os.environ.get(port_env, "0")
        try:
            resolved_port = int(raw_port)
        except (TypeError, ValueError) as exc:
            raise ValueError(f"{port_env} must be an integer between 0 and 65535") from exc
        if not 0 <= resolved_port <= 65535:
            raise ValueError(f"{port_env} must be an integer between 0 and 65535")

        instance_type_env = "DCC_MCP_{}_INSTANCE_TYPE".format(
            "".join(character if character.isalnum() else "_" for character in dcc_name.upper())
        )
        resolved_instance_type = instance_type
        if resolved_instance_type is None:
            resolved_instance_type = os.environ.get(instance_type_env, os.environ.get("DCC_MCP_INSTANCE_TYPE"))

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
        sidecar = SidecarOptions.from_env(
            host_rpc=host_rpc,
            adapter_version=adapter_version,
            display_name=sidecar_display_name or server_name,
            wait_ready_timeout_secs=wait_ready_timeout_secs,
        )

        return cls(
            dcc_name=dcc_name,
            builtin_skills_dir=builtin_skills_dir,
            port=resolved_port,
            server_name=server_name,
            server_version=server_version,
            instance_type=resolved_instance_type,
            gateway=gateway,
            observability=observability,
            diagnostics=diagnostics,
            execution=execution,
            sidecar=sidecar,
        )
