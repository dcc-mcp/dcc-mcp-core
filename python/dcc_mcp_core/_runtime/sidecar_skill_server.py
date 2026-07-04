"""Sidecar-backed skill server facade for py37-lite ``DccServerBase``."""

from __future__ import annotations

import logging
import os
from typing import Any
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional

from dcc_mcp_core._install_lifecycle_sidecar import build_sidecar_command
from dcc_mcp_core._install_lifecycle_sidecar import launch_sidecar
from dcc_mcp_core._runtime.tool_registry_py import PurePythonToolRegistry

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
        watch_pid: Optional[int] = None,
        adapter_version: Optional[str] = None,
        display_name: Optional[str] = None,
        wait_ready_timeout_secs: float = 15.0,
    ) -> None:
        self._dcc_name = str(dcc_name or "dcc")
        self._config = config
        self._host_rpc = str(host_rpc or "").strip()
        self._watch_pid = watch_pid
        self._adapter_version = adapter_version
        self._display_name = display_name or self._dcc_name
        self._wait_ready_timeout_secs = float(wait_ready_timeout_secs)
        self._registry = PurePythonToolRegistry()
        self._pending_skill_paths: List[str] = []
        self._handle: Optional[SidecarServerHandle] = None
        self._launch_result: Dict[str, Any] = {}

    @property
    def registry(self) -> PurePythonToolRegistry:
        return self._registry

    def discover(self, extra_paths: Optional[List[str]] = None) -> int:
        paths = [str(path) for path in (extra_paths or []) if str(path).strip()]
        self._pending_skill_paths = paths
        if paths:
            existing = os.environ.get("DCC_MCP_SKILL_PATHS", "")
            merged = os.pathsep.join([existing] + paths) if existing else os.pathsep.join(paths)
            os.environ["DCC_MCP_SKILL_PATHS"] = merged
        logger.info(
            "[%s] sidecar discover recorded %d path(s); HTTP/MCP is owned by dcc-mcp-server sidecar",
            self._dcc_name,
            len(paths),
        )
        return len(paths)

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
        command = build_sidecar_command(
            dcc_type=self._dcc_name,
            host_rpc=self._host_rpc,
            watch_pid=self._watch_pid,
            registry_dir=registry_dir,
            display_name=self._display_name,
            adapter_version=self._adapter_version,
            gateway_port=gateway_port if gateway_port > 0 else None,
            require_dispatch_capable=True,
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

    # SkillQueryClient compatibility stubs ---------------------------------

    def list_skills(self) -> List[Any]:
        return []

    def search_skills(self, **kwargs: Any) -> List[Any]:
        return []

    def load_skill(self, name: str) -> None:
        logger.debug("[%s] load_skill(%r) delegated to sidecar", self._dcc_name, name)

    def get_skill(self, name: str) -> Any:
        return None

    def load_skill_object(self, skill: Any) -> None:
        logger.debug("[%s] load_skill_object delegated to sidecar", self._dcc_name)

    def unload_skill(self, name: str) -> None:
        logger.debug("[%s] unload_skill(%r) delegated to sidecar", self._dcc_name, name)

    def is_loaded(self, name: str) -> bool:
        return False

    def get_skill_info(self, name: str) -> Any:
        return None

    def set_in_process_executor(self, executor: Any) -> None:
        logger.debug("[%s] set_in_process_executor stored for host-rpc dispatch", self._dcc_name)

    def attach_dispatcher(self, dispatcher: Any) -> None:
        logger.debug("[%s] attach_dispatcher ignored in sidecar mode", self._dcc_name)

    def resources(self) -> Any:
        return None

    def set_readiness_probe(self, probe: Any) -> bool:
        return False


def _resolve_mcp_url(launch: Dict[str, Any]) -> str:
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


def _port_from_url(url: str) -> int:
    try:
        from urllib.parse import urlsplit

        split = urlsplit(url)
        if split.port is not None:
            return int(split.port)
    except Exception:
        pass
    return 0
