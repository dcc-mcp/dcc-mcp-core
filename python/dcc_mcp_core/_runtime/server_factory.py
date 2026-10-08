"""Factories for the MCP server objects adapters build.

Two entry points live here:

* :func:`create_adapter_server` — the inner server ``DccServerBase`` wires up.
* :func:`create_skill_server` — the public one-call Skills-First factory.

Both funnel ``config`` through :func:`validate_mcp_http_config` so a wrong type
fails here, with the construction snippet, instead of inside PyO3.
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING
from typing import Any

from dcc_mcp_core._runtime.config_bridge import validate_mcp_http_config
from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer
from dcc_mcp_core._runtime.skill_paths import get_app_skill_paths_from_env
from dcc_mcp_core.constants import ENV_HOST_RPC

if TYPE_CHECKING:
    from dcc_mcp_core._core import McpHttpServer
    from dcc_mcp_core._server.options import DccServerOptions

try:
    from dcc_mcp_core import _core
except ImportError:
    _core = None


def create_adapter_server(
    dcc_name: str,
    config: McpHttpConfig | None = None,
    options: DccServerOptions | None = None,
) -> McpHttpServer | SidecarBackedSkillServer:
    """Create the inner server object used by :class:`DccServerBase`.

    Args:
        dcc_name: DCC identifier (``"maya"``, ``"blender"``, …) used for skill
            scoping and registry lookup.
        config: MCP HTTP configuration. ``None`` is a supported value rather
            than a validation bypass: both backends define their own defaults
            (OS-assigned port, shared gateway port, no job persistence) and
            apply them when no config is supplied. A config object from either
            spelling is accepted — the PyO3 type the wheel resolves or the
            pure-Python dataclass — because they are distinct classes exposing
            the same attributes. Any other type (a plain ``dict`` being the
            common mistake) is rejected with a message showing how to build a
            real ``McpHttpConfig``.
        options: Resolved :class:`DccServerOptions`. Only the py37-lite sidecar
            path reads it (``sidecar`` for the RPC endpoint, ``diagnostics``
            for the watched PID); the embedded path ignores it.

    Returns:
        The embedded ``McpHttpServer`` when the compiled extension is
        importable, otherwise a :class:`SidecarBackedSkillServer`.

    Raises:
        TypeError: ``config`` is neither ``None`` nor an ``McpHttpConfig``.

    """
    validate_mcp_http_config(config, owner=f"create_adapter_server(dcc_name={dcc_name!r})")
    if is_core_extension_available():
        from dcc_mcp_core._core import create_skill_server

        return create_skill_server(dcc_name, config)

    sidecar = getattr(options, "sidecar", None) if options is not None else None
    host_rpc = _resolve_host_rpc(sidecar)
    return SidecarBackedSkillServer(
        dcc_name,
        config,
        host_rpc=host_rpc,
        watch_pid=_resolve_watch_pid(options),
        adapter_version=getattr(sidecar, "adapter_version", None) if sidecar is not None else None,
        display_name=getattr(sidecar, "display_name", None) if sidecar is not None else None,
        wait_ready_timeout_secs=_resolve_wait_ready(sidecar),
        server_bin=getattr(sidecar, "server_bin", None) if sidecar is not None else None,
        extra_args=_resolve_extra_args(sidecar),
    )


def create_skill_server(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
    accumulated: bool = True,
) -> McpHttpServer | SidecarBackedSkillServer:
    """Public one-call Skills-First server factory.

    Args:
        app_name: DCC name (``"maya"``, ``"blender"``, …). Drives the skill-path
            env vars and the default MCP server name.
        config: MCP HTTP configuration, or ``None`` to take the backend
            defaults. An ``McpHttpConfig`` from either spelling is accepted;
            anything else (a ``dict`` in particular) is rejected up front with
            the construction snippet instead of failing deep inside PyO3.
        extra_paths: Additional skill directories scanned ahead of the env-var
            paths.
        dcc_name: Override the DCC filter used for discovery (defaults to
            ``app_name``).
        accumulated: Also discover user/team skill directories.

    Raises:
        TypeError: ``config`` is neither ``None`` nor an ``McpHttpConfig``.

    """
    validate_mcp_http_config(config, owner=f"create_skill_server(app_name={app_name!r})")
    if _core is not None and is_core_extension_available():
        return _core.create_skill_server(
            app_name,
            config=config,
            extra_paths=extra_paths,
            dcc_name=dcc_name,
            accumulated=accumulated,
        )
    server = create_adapter_server(dcc_name or app_name, config, None)
    discovery_paths = list(extra_paths or [])
    discovery_paths.extend(get_app_skill_paths_from_env(app_name))
    server.discover(extra_paths=list(dict.fromkeys(discovery_paths)), accumulated=accumulated)
    return server


def _resolve_host_rpc(sidecar: Any) -> str:
    if sidecar is not None:
        value = getattr(sidecar, "host_rpc", None)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return str(os.environ.get(ENV_HOST_RPC, "")).strip()


def _resolve_watch_pid(options: Any | None) -> int | None:
    if options is None:
        return None
    diagnostics = getattr(options, "diagnostics", None)
    if diagnostics is not None and getattr(diagnostics, "dcc_pid", None) is not None:
        return int(diagnostics.dcc_pid)
    return None


def _resolve_wait_ready(sidecar: Any) -> float:
    if sidecar is None:
        return 15.0
    value = getattr(sidecar, "wait_ready_timeout_secs", None)
    if value is None:
        return 15.0
    return float(value)


def _resolve_extra_args(sidecar: Any) -> tuple:
    if sidecar is None:
        return ()
    value = getattr(sidecar, "extra_args", None)
    if not value:
        return ()
    return tuple(str(arg) for arg in value)
