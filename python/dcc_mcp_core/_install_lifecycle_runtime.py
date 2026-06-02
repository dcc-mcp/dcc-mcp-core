"""Import-light runtime registry helpers for DCC adapter installers."""

# ruff: noqa: UP006, UP045

from __future__ import annotations

import json
import os
from pathlib import Path
import tempfile
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

from ._install_lifecycle_process import entry_runtime_alive as _entry_runtime_alive

ROLE_METADATA_KEY = "dcc_mcp_role"
ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"
DISPATCH_STATUS_METADATA_KEY = "dispatch_status"
DISPATCH_STATUS_BOOTING = "booting"
DISPATCH_STATUS_READY = "ready"
DISPATCH_STATUS_UNAVAILABLE = "unavailable"
REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"
REGISTRY_FILE = "services.json"

_INSTALL_ROOT_KEYS = (
    "install_root",
    "adapter_root",
    "adapter_install_root",
    "package_root",
    "root_path",
    "dcc_mcp_core_root",
    "dcc_mcp_server_root",
)
_VERSION_KEYS = {
    "core": ("dcc_mcp_core_version", "core_version"),
    "server": ("dcc_mcp_server_version", "server_version"),
    "adapter": ("adapter_version", "dcc_mcp_adapter_version"),
}
_RESTART_COMMAND_KEYS = ("restart_command", "dcc_mcp_restart_command")
_LAUNCH_COMMAND_KEYS = ("launch_command", "dcc_mcp_launch_command")


def default_registry_dir() -> str:
    """Return the shared FileRegistry directory without importing ``_core``."""
    return os.environ.get(REGISTRY_ENV) or str(Path(tempfile.gettempdir()) / "dcc-mcp-registry")


def query_runtime_state(
    registry_dir: Optional[Any] = None,
    *,
    dcc_type: Optional[str] = None,
    role: Optional[str] = None,
    install_root: Optional[Any] = None,
    include_dead: bool = True,
) -> Dict[str, Any]:
    """Read registered DCC runtimes from ``services.json`` using stdlib only."""
    root = _to_path(install_root)
    entries = []
    for raw in _read_registry_entries(registry_dir):
        entry = _normalise_entry(raw)
        if dcc_type and entry.get("dcc_type") != dcc_type:
            continue
        if role and entry.get("role") != role:
            continue
        if root is not None:
            roots = [_to_path(item) for item in entry.get("install_roots", [])]
            if not any(_path_under(item, root) or _path_under(root, item) for item in roots):
                continue
        if not include_dead and entry.get("runtime_alive") is False:
            continue
        entries.append(entry)

    return {
        "success": True,
        "registry_dir": str(_to_path(registry_dir) or Path(default_registry_dir())),
        "total": len(entries),
        "alive_count": sum(1 for entry in entries if entry.get("runtime_alive") is True),
        "dead_count": sum(1 for entry in entries if entry.get("runtime_alive") is False),
        "entries": entries,
    }


def _normalise_entry(entry: Dict[str, Any]) -> Dict[str, Any]:
    metadata = entry.get("metadata") if isinstance(entry.get("metadata"), dict) else {}
    sidecar_pid = _parse_int(metadata.get("sidecar_pid"))
    parent_pid = _parse_int(entry.get("pid"))
    runtime_pid = sidecar_pid if sidecar_pid is not None else parent_pid
    sentinel_path = entry.get("sentinel_path")
    runtime_alive = _entry_runtime_alive(sentinel_path, runtime_pid)
    host = str(entry.get("host") or "127.0.0.1")
    port = _parse_int(entry.get("port")) or 0
    mcp_url = metadata.get("mcp_url") or (f"http://{host}:{port}/mcp" if port else None)
    role = str(metadata.get(ROLE_METADATA_KEY) or "runtime")
    dispatch_status = _normalise_dispatch_status(metadata.get(DISPATCH_STATUS_METADATA_KEY))
    host_rpc_uri = _optional_text(metadata.get("host_rpc_uri"))
    host_rpc_scheme = _optional_text(metadata.get("host_rpc_scheme"))
    failure_stage = _optional_text(metadata.get("failure_stage"))
    failure_reason = _optional_text(metadata.get("failure_reason"))
    dispatch_ready = bool(dispatch_status == DISPATCH_STATUS_READY and mcp_url and runtime_alive is not False)
    dispatch = {
        "reported": dispatch_status is not None,
        "status": dispatch_status or "not_reported",
        "ready": dispatch_ready if dispatch_status is not None else None,
        "ready_at_unix": _optional_text(metadata.get("dispatch_ready_at_unix")),
        "host_rpc_uri": host_rpc_uri,
        "host_rpc_scheme": host_rpc_scheme,
        "failure_stage": failure_stage,
        "failure_reason": failure_reason,
    }

    install_roots = []
    for key in _INSTALL_ROOT_KEYS:
        value = metadata.get(key)
        if value:
            install_roots.append(str(value))
    versions = {
        "core": _metadata_value(metadata, "core"),
        "server": _metadata_value(metadata, "server"),
        "adapter": entry.get("adapter_version") or _metadata_value(metadata, "adapter"),
    }
    restart_command = _first_present(metadata, _RESTART_COMMAND_KEYS)
    launch_command = _first_present(metadata, _LAUNCH_COMMAND_KEYS)

    return {
        "dcc_type": entry.get("dcc_type"),
        "instance_id": entry.get("instance_id"),
        "display_name": entry.get("display_name"),
        "role": role,
        "status": entry.get("status", "available"),
        "host": host,
        "port": port,
        "mcp_url": mcp_url,
        "dispatch_status": dispatch_status,
        "dispatch_ready": dispatch_ready,
        "dispatch": dispatch,
        "host_rpc_uri": host_rpc_uri,
        "host_rpc_scheme": host_rpc_scheme,
        "failure_stage": failure_stage,
        "failure_reason": failure_reason,
        "gateway_runtime_mode": _optional_text(metadata.get("gateway_runtime_mode")),
        "gateway_guardian_enabled": _metadata_bool(metadata.get("gateway_guardian_enabled")),
        "parent_pid": parent_pid,
        "sidecar_pid": sidecar_pid,
        "runtime_pid": runtime_pid,
        "runtime_alive": runtime_alive,
        "sentinel_path": str(sentinel_path) if sentinel_path not in (None, "") else None,
        "version": entry.get("version"),
        "adapter_version": entry.get("adapter_version"),
        "adapter_dcc": entry.get("adapter_dcc") or metadata.get("adapter_dcc"),
        "versions": versions,
        "restartable": bool(sidecar_pid or restart_command or launch_command),
        "restart_command": restart_command,
        "launch_command": launch_command,
        "metadata": metadata,
        "install_roots": install_roots,
    }


def _to_path(path: Any) -> Optional[Path]:
    if path in (None, ""):
        return None
    try:
        return Path(str(path)).expanduser().resolve()
    except OSError:
        return Path(str(path)).expanduser().absolute()


def _path_under(path: Optional[Path], root: Optional[Path]) -> bool:
    if path is None or root is None:
        return False
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True


def _metadata_value(metadata: Dict[str, Any], component: str) -> Any:
    return _first_present(metadata, _VERSION_KEYS.get(component, ()))


def _first_present(metadata: Dict[str, Any], keys: Iterable[str]) -> Any:
    for key in keys:
        value = metadata.get(key)
        if value not in (None, ""):
            return value
    return None


def _optional_text(value: Any) -> Optional[str]:
    if value in (None, ""):
        return None
    return str(value)


def _normalise_dispatch_status(value: Any) -> Optional[str]:
    text = _optional_text(value)
    if text is None:
        return None
    status = text.strip().lower()
    return status or None


def _metadata_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value in (None, ""):
        return False
    return str(value).strip().lower() in {"true", "1", "yes"}


def _parse_int(value: Any) -> Optional[int]:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed > 0 else None


def _read_registry_entries(registry_dir: Optional[Any] = None) -> List[Dict[str, Any]]:
    base = _to_path(registry_dir) or Path(default_registry_dir())
    path = base / REGISTRY_FILE
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, list):
        return []
    return [item for item in data if isinstance(item, dict)]
