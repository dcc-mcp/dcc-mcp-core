"""Sidecar-backed skill server facade for py37-lite ``DccServerBase``."""

from __future__ import annotations

from contextlib import suppress
import logging
import os
from pathlib import Path
from typing import Any
from typing import Callable

from dcc_mcp_core._install_lifecycle_sidecar import build_sidecar_command
from dcc_mcp_core._install_lifecycle_sidecar import launch_sidecar
from dcc_mcp_core._runtime.pure_skill_catalog import PurePythonSkillCatalog
from dcc_mcp_core._runtime.skill_paths import get_app_skill_paths_from_env
from dcc_mcp_core._runtime.skill_paths import get_local_skills_dir
from dcc_mcp_core._runtime.skill_paths import get_skill_paths_from_env
from dcc_mcp_core._runtime.skill_paths import skill_env_slug
from dcc_mcp_core._runtime.tool_registry_py import PurePythonToolRegistry
from dcc_mcp_core.constants import ENV_DISABLE_ACCUMULATED_SKILLS
from dcc_mcp_core.constants import ENV_DISABLE_DEFAULT_SKILL_PATHS
from dcc_mcp_core.constants import ENV_SKILL_PATHS
from dcc_mcp_core.constants import ENV_TEAM_SKILL_PATHS
from dcc_mcp_core.constants import ENV_USER_SKILL_PATHS

logger = logging.getLogger(__name__)


class SidecarServerHandle:
    """Handle returned by :meth:`SidecarBackedSkillServer.start`."""

    def __init__(self, *, port: int, mcp_url: str, shutdown: Callable[[], None]) -> None:
        self._port = int(port)
        self._mcp_url = str(mcp_url)
        self._shutdown = shutdown
        self.is_gateway = False

    @property
    def port(self) -> int:
        return self._port

    def mcp_url(self) -> str:
        return self._mcp_url

    def shutdown(self) -> None:
        self._shutdown()

    def update_gateway_metadata(self, **metadata: Any) -> bool:
        logger.debug("sidecar handle ignores update_gateway_metadata: %s", metadata)
        return False


class SidecarBackedSkillServer:
    """Launch ``dcc-mcp-server sidecar`` instead of embedding ``McpHttpServer``."""

    backend = "sidecar"

    def __init__(
        self,
        dcc_name: str,
        config: Any,
        *,
        host_rpc: str,
        watch_pid: int | None = None,
        adapter_version: str | None = None,
        display_name: str | None = None,
        wait_ready_timeout_secs: float = 15.0,
        server_bin: str | None = None,
        extra_args: tuple | None = None,
    ) -> None:
        self._dcc_name = str(dcc_name or "dcc")
        self._config = config
        self._host_rpc = str(host_rpc or "").strip()
        self._watch_pid = watch_pid
        self._adapter_version = adapter_version
        self._display_name = display_name or self._dcc_name
        self._wait_ready_timeout_secs = float(wait_ready_timeout_secs)
        self._server_bin = str(server_bin).strip() if server_bin else None
        self._extra_args = tuple(str(arg) for arg in (extra_args or ()))
        self._accumulated = True
        self._registry = PurePythonToolRegistry()
        self._catalog = PurePythonSkillCatalog(self._dcc_name)
        self._pending_skill_paths: list[str] = []
        self._handle: SidecarServerHandle | None = None
        self._launch_result: dict[str, Any] = {}

    @property
    def registry(self) -> PurePythonToolRegistry:
        return self._registry

    def discover(self, extra_paths: list[str] | None = None, *, accumulated: bool | None = None) -> int:
        paths = [str(path).strip() for path in (extra_paths or []) if str(path).strip()]
        self._pending_skill_paths = list(dict.fromkeys(paths))
        if accumulated is not None:
            self._accumulated = bool(accumulated)
        discovered = self._catalog.discover(self._skill_search_paths())
        logger.info(
            "[%s] lite metadata catalog discovered %d skill(s) from %d path(s)",
            self._dcc_name,
            discovered,
            len(self._pending_skill_paths),
        )
        return discovered

    def _skill_search_paths(self) -> list[tuple[str, str]]:
        paths = [(path, "repo") for path in self._pending_skill_paths]
        paths.extend((path, "repo") for path in get_app_skill_paths_from_env(self._dcc_name))
        paths.extend((path, "repo") for path in get_skill_paths_from_env())
        if self._accumulated and not _env_flag_enabled(ENV_DISABLE_ACCUMULATED_SKILLS):
            paths.extend(_accumulated_skill_paths(self._dcc_name))
        if not _env_flag_enabled(ENV_DISABLE_DEFAULT_SKILL_PATHS):
            local = Path(get_local_skills_dir(self._dcc_name))
            with suppress(OSError):
                local.mkdir(parents=True, exist_ok=True)
            paths.append((str(local), "repo"))
        return list(dict.fromkeys((path, scope) for path, scope in paths if str(path).strip()))

    def _launch_environment(self) -> dict[str, str]:
        """Build sidecar-only overrides without mutating the DCC process."""
        overrides: dict[str, str] = {}
        paths = list(self._pending_skill_paths)
        existing = os.environ.get(ENV_SKILL_PATHS, "")
        if existing:
            paths.extend(part.strip() for part in existing.split(os.pathsep) if part.strip())
        if paths:
            overrides[ENV_SKILL_PATHS] = os.pathsep.join(dict.fromkeys(paths))
        if not self._accumulated:
            overrides[ENV_DISABLE_ACCUMULATED_SKILLS] = "1"
        return overrides

    def start(self) -> SidecarServerHandle:
        if self._handle is not None:
            return self._handle
        if not self._host_rpc:
            raise RuntimeError(
                "host_rpc is required for py37-lite sidecar mode; "
                "set DccServerOptions.sidecar.host_rpc or DCC_MCP_HOST_RPC"
            )

        gateway_port = int(getattr(self._config, "gateway_port", 0) or 0)
        registry_dir = getattr(self._config, "registry_dir", None)
        launch_env = self._launch_environment()
        command = build_sidecar_command(
            dcc_type=self._dcc_name,
            host_rpc=self._host_rpc,
            watch_pid=self._watch_pid,
            registry_dir=registry_dir,
            display_name=self._display_name,
            adapter_version=self._adapter_version,
            gateway_port=gateway_port if gateway_port > 0 else None,
            require_dispatch_capable=True,
            env=launch_env or None,
        )
        if not command.get("success"):
            raise RuntimeError(command.get("message") or "failed to build sidecar launch command")

        launch = launch_sidecar(
            dcc_type=self._dcc_name,
            host_rpc=self._host_rpc,
            watch_pid=self._watch_pid,
            registry_dir=registry_dir,
            display_name=self._display_name,
            adapter_version=self._adapter_version,
            gateway_port=gateway_port if gateway_port > 0 else None,
            wait_ready_timeout_secs=self._wait_ready_timeout_secs,
            require_dispatch_capable=True,
            server_bin=self._server_bin,
            extra_args=self._extra_args or None,
            env=launch_env or None,
        )
        self._launch_result = dict(launch)
        if not launch.get("success"):
            raise RuntimeError(launch.get("message") or "sidecar launch failed")

        mcp_url = _resolve_mcp_url(launch)
        port = _port_from_url(mcp_url)
        proc = launch.get("process")

        def _shutdown() -> None:
            if proc is not None:
                try:
                    proc.terminate()
                except Exception as exc:
                    logger.debug("[%s] sidecar terminate failed: %s", self._dcc_name, exc)

        self._handle = SidecarServerHandle(port=port, mcp_url=mcp_url, shutdown=_shutdown)
        return self._handle

    # Metadata discovery is local; activation remains unsupported because the
    # Rust sidecar intentionally exposes dispatch-only tools/call.

    def list_skills(self) -> list[Any]:
        return self._catalog.list_skills()

    def search_skills(self, **kwargs: Any) -> list[Any]:
        return self._catalog.search_skills(**kwargs)

    def load_skill(self, name: str) -> None:
        raise RuntimeError(_activation_error(name))

    def get_skill(self, name: str) -> Any:
        return self._catalog.get_skill(name)

    def load_skill_object(self, skill: Any) -> None:
        raise RuntimeError(_activation_error(getattr(skill, "name", "<object>")))

    def unload_skill(self, name: str) -> None:
        raise RuntimeError(_activation_error(name))

    def is_loaded(self, name: str) -> bool:
        return False

    def get_skill_info(self, name: str) -> Any:
        return self._catalog.get_skill(name)

    def set_in_process_executor(self, executor: Any) -> None:
        logger.debug("[%s] set_in_process_executor stored for host-rpc dispatch", self._dcc_name)

    def attach_dispatcher(self, dispatcher: Any) -> None:
        logger.debug("[%s] attach_dispatcher ignored in sidecar mode", self._dcc_name)

    def resources(self) -> Any:
        return None

    def set_readiness_probe(self, probe: Any) -> bool:
        return False


def _resolve_mcp_url(launch: dict[str, Any]) -> str:
    readiness = launch.get("readiness") or {}
    for key in ("mcp_url", "discovery_mcp_url", "endpoint"):
        value = readiness.get(key) or launch.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    metadata = readiness.get("metadata") or launch.get("metadata") or {}
    if isinstance(metadata, dict):
        value = metadata.get("mcp_url")
        if isinstance(value, str) and value.strip():
            return value.strip()
    return "http://127.0.0.1:0/mcp"


def _env_flag_enabled(name: str) -> bool:
    value = os.environ.get(name, "")
    return value == "1" or value.lower() == "true"


def _accumulated_skill_paths(dcc_name: str) -> list[tuple[str, str]]:
    app = skill_env_slug(dcc_name or "dcc")
    names = (
        (f"DCC_MCP_USER_{app}_SKILL_PATHS", "user"),
        (ENV_USER_SKILL_PATHS, "user"),
        (f"DCC_MCP_TEAM_{app}_SKILL_PATHS", "team"),
        (ENV_TEAM_SKILL_PATHS, "team"),
    )
    paths: list[tuple[str, str]] = []
    for name, scope in names:
        paths.extend((part.strip(), scope) for part in os.environ.get(name, "").split(os.pathsep) if part.strip())
    return paths


def _activation_error(name: Any) -> str:
    return (
        f"py37-lite discovered skill metadata for {str(name)!r}, but skill activation is unavailable: "
        "dcc-mcp-server sidecar is dispatch-only; install a native Python 3.7 wheel "
        "for load_skill and declarative skill execution"
    )


def _port_from_url(url: str) -> int:
    try:
        from urllib.parse import urlsplit

        split = urlsplit(url)
        if split.port is not None:
            return int(split.port)
    except Exception:
        pass
    return 0
