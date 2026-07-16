"""Pure-Python ``McpHttpConfig`` for py37-lite and sidecar-backed adapters."""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from typing import Any


def _default_server_version() -> str:
    try:
        from importlib import metadata as importlib_metadata
    except ImportError:
        try:
            import importlib_metadata  # type: ignore[import-not-found]
        except ImportError:
            return "0.0.0-dev"
    try:
        return importlib_metadata.version("dcc-mcp-core")
    except Exception:
        return "0.0.0-dev"


@dataclass
class McpHttpConfig:
    """Python-visible MCP HTTP server configuration without PyO3."""

    port: int = 0
    server_name: str = "dcc-mcp"
    server_version: str = field(default_factory=_default_server_version)
    host: str = "127.0.0.1"
    endpoint_path: str = "/mcp"
    enable_cors: bool = False
    request_timeout_ms: int = 30000
    session_ttl_secs: int = 3600
    enable_tool_cache: bool = False
    enable_prometheus: bool = False
    gateway_port: int = 9765
    registry_dir: str | None = None
    dcc_version: str = ""
    scene: str = ""
    dcc_type: str = ""
    instance_metadata: dict[str, str] = field(default_factory=dict)
    standalone_main_thread_execution: bool = False
    exclude_skill_stubs_from_tools_list: bool = False
    exclude_group_stubs_from_tools_list: bool = False
    job_storage_path: str | None = None
    sandbox_policy: Any = None

    def __init__(
        self,
        port: int = 0,
        server_name: str | None = None,
        server_version: str | None = None,
        **kwargs: Any,
    ) -> None:
        self.port = int(port)
        self.server_name = server_name if server_name is not None else "dcc-mcp"
        self.server_version = server_version if server_version is not None else _default_server_version()
        self.host = str(kwargs.pop("host", "127.0.0.1"))
        self.endpoint_path = str(kwargs.pop("endpoint_path", "/mcp"))
        self.enable_cors = bool(kwargs.pop("enable_cors", False))
        self.request_timeout_ms = int(kwargs.pop("request_timeout_ms", 30000))
        self.session_ttl_secs = int(kwargs.pop("session_ttl_secs", 3600))
        self.enable_tool_cache = bool(kwargs.pop("enable_tool_cache", False))
        self.enable_prometheus = bool(kwargs.pop("enable_prometheus", False))
        self.gateway_port = int(kwargs.pop("gateway_port", 9765))
        self.registry_dir = kwargs.pop("registry_dir", None)
        self.dcc_version = str(kwargs.pop("dcc_version", ""))
        self.scene = str(kwargs.pop("scene", ""))
        self.dcc_type = str(kwargs.pop("dcc_type", ""))
        metadata = kwargs.pop("instance_metadata", None)
        self.instance_metadata = dict(metadata) if isinstance(metadata, dict) else {}
        self.standalone_main_thread_execution = bool(kwargs.pop("standalone_main_thread_execution", False))
        self.exclude_skill_stubs_from_tools_list = bool(kwargs.pop("exclude_skill_stubs_from_tools_list", False))
        self.exclude_group_stubs_from_tools_list = bool(kwargs.pop("exclude_group_stubs_from_tools_list", False))
        self.job_storage_path = kwargs.pop("job_storage_path", None)
        self.sandbox_policy = kwargs.pop("sandbox_policy", None)
        for key, value in kwargs.items():
            setattr(self, key, value)

    def __repr__(self) -> str:
        return (
            f"McpHttpConfig(port={self.port}, server_name={self.server_name!r}, server_version={self.server_version!r})"
        )
