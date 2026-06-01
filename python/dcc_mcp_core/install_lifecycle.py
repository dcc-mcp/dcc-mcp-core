"""Import-light install and uninstall lifecycle helpers for DCC adapters.

This module intentionally uses only the Python standard library and never
imports :mod:`dcc_mcp_core._core`. Adapter installers can import it from the
same package directory they are about to remove without locking the native
extension as a side effect.
"""

from __future__ import annotations

import argparse
import importlib.machinery
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Tuple
from typing import Union

from ._install_lifecycle_process import entry_runtime_alive as _entry_runtime_alive
from ._install_lifecycle_process import is_windows_lock_error as _is_windows_lock_error
from ._install_lifecycle_process import terminate_pid as _terminate_pid
from ._install_lifecycle_sidecar import build_sidecar_command
from ._install_lifecycle_sidecar import launch_sidecar

ROLE_METADATA_KEY = "dcc_mcp_role"
ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"
REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"
REGISTRY_FILE = "services.json"
REZ_CACHE_ROOT_ENV = "DCC_MCP_REZ_LOCAL_CACHE_ROOT"
DEPLOYMENT_MODE_ENV = "DCC_MCP_DEPLOYMENT_MODE"

DEFAULT_DEPLOYMENT_PACKAGES = ("dcc_mcp_core", "dcc_mcp_server")
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

_NATIVE_SUFFIXES = tuple(
    sorted(
        set(importlib.machinery.EXTENSION_SUFFIXES) | {".dll", ".dylib", ".pyd", ".so"},
        key=len,
        reverse=True,
    )
)

__all__ = [
    "DEPLOYMENT_MODE_ENV",
    "REGISTRY_ENV",
    "ROLE_METADATA_KEY",
    "ROLE_PER_DCC_SIDECAR",
    "build_sidecar_command",
    "default_registry_dir",
    "inspect_install_root",
    "launch_sidecar",
    "main",
    "plan_runtime_updates",
    "query_runtime_state",
    "resolve_deployment_layout",
    "safe_remove_tree",
    "safe_replace_tree",
    "stop_runtime_entries",
]


def default_registry_dir() -> str:
    """Return the shared FileRegistry directory without importing ``_core``."""
    return os.environ.get(REGISTRY_ENV) or str(Path(tempfile.gettempdir()) / "dcc-mcp-registry")


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


def _metadata_value(metadata: Dict[str, Any], component: str) -> Any:
    return _first_present(metadata, _VERSION_KEYS.get(component, ()))


def _first_present(metadata: Dict[str, Any], keys: Iterable[str]) -> Any:
    for key in keys:
        value = metadata.get(key)
        if value not in (None, ""):
            return value
    return None


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


def resolve_deployment_layout(
    cache_root: Optional[Any] = None,
    *,
    packages: Optional[Iterable[str]] = None,
    adapter_package: Optional[str] = None,
    package_roots: Optional[Dict[str, Any]] = None,
    env: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    r"""Resolve Rez-style or filesystem package roots without importing ``_core``.

    Pipeline teams can call this from bootstrap scripts before packages are
    formally built. Explicit roots win, then ``REZ_<PACKAGE>_ROOT`` variables,
    then a shared local cache root such as ``G:\\_thm\\rez_local_cache\\ext``.
    """
    environment = dict(os.environ if env is None else env)
    package_names = _unique_strings(packages or DEFAULT_DEPLOYMENT_PACKAGES)
    if adapter_package:
        package_names = _unique_strings([*package_names, adapter_package])
    explicit_roots = dict(package_roots or {})
    cache = _to_path(
        cache_root
        or environment.get(REZ_CACHE_ROOT_ENV)
        or environment.get("REZ_LOCAL_CACHE_ROOT")
        or environment.get("REZ_LOCAL_PACKAGES_PATH")
    )

    resolved = []
    for package in package_names:
        root, source = _resolve_package_root(package, explicit_roots, environment, cache)
        exists = bool(root and root.exists())
        resolved.append(
            {
                "name": package,
                "root": str(root) if root else None,
                "source": source,
                "exists": exists,
                "env_var": _package_env_var(package),
            }
        )

    prepend_python = []
    prepend_path = []
    for item in resolved:
        if not item["root"] or not item["exists"]:
            continue
        root = Path(str(item["root"]))
        _extend_unique(prepend_python, _package_python_paths(root))
        _extend_unique(prepend_path, _package_path_entries(root))

    mode = _deployment_mode(environment, resolved)
    return {
        "success": True,
        "mode": mode,
        "cache_root": str(cache) if cache else None,
        "packages": resolved,
        "missing_packages": [item["name"] for item in resolved if not item["exists"]],
        "environment": {
            "prepend": {
                "PYTHONPATH": prepend_python,
                "PATH": prepend_path,
            },
            "set": {
                DEPLOYMENT_MODE_ENV: mode,
            },
        },
    }


def plan_runtime_updates(
    runtime_state: Optional[Union[Dict[str, Any], List[Dict[str, Any]]]] = None,
    *,
    registry_dir: Optional[Any] = None,
    dcc_type: Optional[str] = None,
    role: Optional[str] = None,
    target_versions: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    """Plan restart actions for mixed-version runtime rows.

    The returned data is intentionally JSON-shaped so installer scripts and
    Admin UI endpoints can render the same decision without importing native
    code or guessing which DCC process owns a sidecar.
    """
    state = (
        runtime_state
        if runtime_state is not None
        else query_runtime_state(registry_dir, dcc_type=dcc_type, role=role, include_dead=True)
    )
    entries = state.get("entries", []) if isinstance(state, dict) else state
    targets = _normalise_target_versions(target_versions or {})
    plans = []

    for entry in entries:
        component_status = {}
        for component, target in targets.items():
            current = _entry_version(entry, component)
            component_status[component] = {
                "current": current,
                "target": target,
                "status": _compare_version(current, target),
            }

        stale_components = [name for name, item in component_status.items() if item["status"] == "older"]
        unknown_components = [name for name, item in component_status.items() if item["status"] == "unknown"]
        action, restart_scope = _restart_action(entry, stale_components, unknown_components)
        plans.append(
            {
                "instance_id": entry.get("instance_id"),
                "dcc_type": entry.get("dcc_type"),
                "display_name": entry.get("display_name"),
                "mcp_url": entry.get("mcp_url"),
                "runtime_alive": entry.get("runtime_alive"),
                "role": entry.get("role"),
                "versions": component_status,
                "stale_components": stale_components,
                "unknown_components": unknown_components,
                "action": action,
                "restart_scope": restart_scope,
                "restartable": action in {"restart_sidecar", "restart_via_command"},
                "sidecar_pid": entry.get("sidecar_pid"),
                "parent_pid": entry.get("parent_pid"),
                "restart_command": entry.get("restart_command"),
                "launch_command": entry.get("launch_command"),
                "recommended_next_action": _recommended_update_action(action),
            }
        )

    return {
        "success": True,
        "target_versions": targets,
        "total": len(plans),
        "restart_required_count": sum(1 for item in plans if item["stale_components"]),
        "verification_required_count": sum(1 for item in plans if item["action"] == "verify_runtime_metadata"),
        "manual_restart_count": sum(1 for item in plans if item["action"] == "manual_restart_required"),
        "plans": plans,
    }


def stop_runtime_entries(
    registry_dir: Optional[Any] = None,
    *,
    dcc_type: Optional[str] = None,
    role: Optional[str] = ROLE_PER_DCC_SIDECAR,
    install_root: Optional[Any] = None,
    timeout_secs: float = 5.0,
    include_host_processes: bool = False,
) -> Dict[str, Any]:
    """Request stop for registered sidecars without importing native core code.

    By default this only targets rows that expose ``metadata.sidecar_pid``. It
    will not terminate the parent DCC process unless ``include_host_processes``
    is set, which keeps adapter uninstallers from closing a user's scene host.
    """
    state = query_runtime_state(
        registry_dir,
        dcc_type=dcc_type,
        role=role,
        install_root=install_root,
        include_dead=True,
    )
    results = []
    for entry in state["entries"]:
        sidecar_pid = entry.get("sidecar_pid")
        target_pid = sidecar_pid
        target_kind = "sidecar"
        if target_pid is None and include_host_processes:
            target_pid = entry.get("parent_pid")
            target_kind = "host"

        if target_pid is None:
            results.append(
                {
                    "instance_id": entry.get("instance_id"),
                    "status": "unsupported",
                    "message": "No sidecar_pid is registered; host process termination is disabled.",
                }
            )
            continue

        if entry.get("runtime_alive") is False:
            results.append(
                {
                    "pid": int(target_pid),
                    "target": target_kind,
                    "status": "already_stopped",
                    "message": "Registry owner sentinel/PID is dead; not terminating a potentially reused PID.",
                }
            )
            continue

        results.append(_terminate_pid(int(target_pid), timeout_secs, target_kind))

    return {
        "success": all(item["status"] in {"stopped", "already_stopped"} for item in results),
        "registry_dir": state["registry_dir"],
        "total": len(results),
        "results": results,
    }


def inspect_install_root(install_root: Any) -> Dict[str, Any]:
    """Inspect whether native artifacts under an install root are loaded now."""
    root = _to_path(install_root)
    loaded = _loaded_native_artifacts(root)
    requires_restart = bool(loaded)
    locked_path = loaded[0]["path"] if loaded else None
    return {
        "success": True,
        "status": "requires_restart" if requires_restart else "ok",
        "requires_restart": requires_restart,
        "install_root": str(root) if root else str(install_root),
        "locked_path": locked_path,
        "loaded_native_artifacts": loaded,
        "recommended_next_action": (
            "Defer cleanup until the DCC host restarts, then remove or replace the install root."
            if requires_restart
            else "Immediate cleanup can be attempted."
        ),
    }


def safe_remove_tree(path: Any) -> Dict[str, Any]:
    """Remove a tree or return a structured restart-required diagnostic."""
    root = _to_path(path)
    if root is None:
        return _failed("invalid_path", "Path is required.", None)
    if not root.exists():
        return {
            "success": True,
            "status": "skipped",
            "requires_restart": False,
            "path": str(root),
            "message": "Path does not exist.",
        }

    preflight = inspect_install_root(root)
    if preflight["requires_restart"]:
        return _requires_restart(
            root,
            preflight["locked_path"],
            "native_artifact_loaded",
            "A native extension under this install root is loaded in the current process.",
            {"operation": "remove_tree", "path": str(root)},
        )

    try:
        shutil.rmtree(str(root))
    except OSError as exc:
        return _classify_remove_error(root, exc)

    return {
        "success": True,
        "status": "removed",
        "requires_restart": False,
        "path": str(root),
    }


def safe_replace_tree(source: Any, destination: Any) -> Dict[str, Any]:
    """Replace a destination tree, classifying native-file lock failures."""
    src = _to_path(source)
    dst = _to_path(destination)
    if src is None or dst is None:
        return _failed("invalid_path", "Source and destination are required.", None)
    if not src.exists():
        return _failed("missing_source", f"Source path does not exist: {src}", dst)

    if dst.exists():
        removed = safe_remove_tree(dst)
        if not removed.get("success"):
            removed["deferred_operation"] = {
                "operation": "replace_tree",
                "source": str(src),
                "destination": str(dst),
            }
            return removed

    try:
        shutil.copytree(str(src), str(dst))
    except OSError as exc:
        return _classify_remove_error(
            dst,
            exc,
            deferred={"operation": "replace_tree", "source": str(src), "destination": str(dst)},
        )

    return {
        "success": True,
        "status": "replaced",
        "requires_restart": False,
        "source": str(src),
        "destination": str(dst),
    }


def _resolve_package_root(
    package: str,
    explicit_roots: Dict[str, Any],
    env: Dict[str, str],
    cache_root: Optional[Path],
) -> Tuple[Optional[Path], str]:
    if package in explicit_roots:
        return _to_path(explicit_roots[package]), "explicit"
    env_var = _package_env_var(package)
    if env.get(env_var):
        return _to_path(env[env_var]), "rez-env"
    if cache_root is not None:
        return cache_root / package, "cache-root"
    return None, "missing"


def _deployment_mode(env: Dict[str, str], packages: List[Dict[str, Any]]) -> str:
    sources = {item["source"] for item in packages if item["exists"]}
    if "rez-env" in sources and sources - {"rez-env"}:
        return "mixed"
    if "rez-env" in sources or env.get("REZ_USED_RESOLVE"):
        return "rez"
    if sources:
        return "filesystem"
    return "unknown"


def _package_env_var(package: str) -> str:
    token = re.sub(r"[^A-Za-z0-9]+", "_", package).strip("_").upper()
    return f"REZ_{token}_ROOT"


def _package_python_paths(root: Path) -> List[str]:
    candidates = [root / "python", root / "lib"]
    existing = [str(path) for path in candidates if path.exists()]
    if existing:
        return existing
    has_python_payload = (root / "__init__.py").exists() or any(root.glob("*.py"))
    return [str(root)] if has_python_payload else []


def _package_path_entries(root: Path) -> List[str]:
    candidates = [root / "bin", root / "Scripts"]
    return [str(path) for path in candidates if path.exists()]


def _unique_strings(values: Iterable[str]) -> List[str]:
    result = []
    for value in values:
        text = str(value).strip()
        if text and text not in result:
            result.append(text)
    return result


def _extend_unique(items: List[str], values: Iterable[str]) -> None:
    for value in values:
        if value not in items:
            items.append(value)


def _normalise_target_versions(target_versions: Dict[str, str]) -> Dict[str, str]:
    result = {}
    for key, value in target_versions.items():
        if value in (None, ""):
            continue
        component = _target_component(str(key))
        result[component] = str(value)
    return result


def _target_component(key: str) -> str:
    token = key.replace("-", "_").lower()
    if token in {"core", "dcc_mcp_core"}:
        return "core"
    if token in {"server", "dcc_mcp_server"}:
        return "server"
    return "adapter"


def _entry_version(entry: Dict[str, Any], component: str) -> Optional[str]:
    versions = entry.get("versions") if isinstance(entry.get("versions"), dict) else {}
    value = versions.get(component)
    if value not in (None, ""):
        return str(value)
    if component == "adapter" and entry.get("adapter_version") not in (None, ""):
        return str(entry.get("adapter_version"))
    metadata = entry.get("metadata") if isinstance(entry.get("metadata"), dict) else {}
    value = _metadata_value(metadata, component)
    return str(value) if value not in (None, "") else None


def _compare_version(current: Optional[str], target: Optional[str]) -> str:
    if not target:
        return "unknown"
    if not current:
        return "unknown"
    current_semver = _parse_semver(current)
    target_semver = _parse_semver(target)
    if current_semver is None or target_semver is None:
        return "equal" if str(current) == str(target) else "unknown"
    if current_semver < target_semver:
        return "older"
    if current_semver > target_semver:
        return "newer"
    return "equal"


def _parse_semver(value: str) -> Optional[Tuple[int, int, int]]:
    text = str(value).strip().lstrip("vV")
    text = re.split(r"[-+]", text, maxsplit=1)[0]
    parts = text.split(".")
    if not parts or any(not part.isdigit() for part in parts[:3]):
        return None
    padded = [int(part) for part in parts[:3]]
    while len(padded) < 3:
        padded.append(0)
    return tuple(padded[:3])


def _restart_action(
    entry: Dict[str, Any],
    stale_components: List[str],
    unknown_components: List[str],
) -> Tuple[str, str]:
    if not stale_components:
        if unknown_components:
            return "verify_runtime_metadata", "metadata"
        return "keep", "none"
    if entry.get("sidecar_pid"):
        return "restart_sidecar", "sidecar"
    if entry.get("restart_command") or entry.get("launch_command"):
        return "restart_via_command", "configured-command"
    return "manual_restart_required", "host-process"


def _recommended_update_action(action: str) -> str:
    if action == "keep":
        return "Keep the runtime registered; no version drift was detected."
    if action == "restart_sidecar":
        return "Stop the registered sidecar, restart it from the target deployment, then re-run MCP readiness."
    if action == "restart_via_command":
        return "Run the configured restart command, then verify /mcp initialize and reset flows."
    if action == "verify_runtime_metadata":
        return (
            "Runtime version metadata is incomplete; verify the runtime or restart it before assuming "
            "it uses the target deployment."
        )
    return "Restart the owning DCC host before using reset or MCP calls against this instance."


def _loaded_native_artifacts(root: Optional[Path]) -> List[Dict[str, str]]:
    loaded = []
    for name, module in list(sys.modules.items()):
        filename = getattr(module, "__file__", None)
        path = _to_path(filename)
        if path is None or not _path_under(path, root):
            continue
        if not _is_native_artifact(path):
            continue
        loaded.append({"module": name, "path": str(path)})
    return loaded


def _is_native_artifact(path: Path) -> bool:
    text = str(path).lower()
    return any(text.endswith(suffix.lower()) for suffix in _NATIVE_SUFFIXES)


def _classify_remove_error(
    root: Path,
    exc: OSError,
    deferred: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    locked_path = _exception_path(exc) or root
    if _is_windows_lock_error(exc):
        return _requires_restart(
            root,
            str(locked_path),
            "windows_file_lock",
            str(exc),
            deferred or {"operation": "remove_tree", "path": str(root)},
        )
    return _failed("remove_failed", str(exc), root)


def _exception_path(exc: OSError) -> Optional[Path]:
    for value in (getattr(exc, "filename", None), getattr(exc, "filename2", None)):
        path = _to_path(value)
        if path is not None:
            return path
    return None


def _requires_restart(
    root: Path,
    locked_path: Optional[str],
    reason: str,
    message: str,
    deferred: Dict[str, Any],
) -> Dict[str, Any]:
    return {
        "success": False,
        "status": "requires_restart",
        "requires_restart": True,
        "path": str(root),
        "locked_path": locked_path,
        "reason": reason,
        "message": message,
        "recommended_next_action": (
            "Schedule the deferred operation and complete it on next DCC startup before importing dcc_mcp_core."
        ),
        "deferred_operation": deferred,
    }


def _failed(reason: str, message: str, path: Optional[Path]) -> Dict[str, Any]:
    return {
        "success": False,
        "status": "failed",
        "requires_restart": False,
        "path": str(path) if path else None,
        "reason": reason,
        "message": message,
    }


def _print_json(value: Dict[str, Any]) -> int:
    print(json.dumps(value, indent=2, sort_keys=True))
    return 0 if value.get("success") else 1


def _parse_target_versions(values: Optional[Iterable[str]]) -> Dict[str, str]:
    result = {}
    for value in values or []:
        if "=" not in value:
            raise ValueError(f"Expected KEY=VERSION target, got: {value}")
        key, version = value.split("=", 1)
        result[key.strip()] = version.strip()
    return result


def main(argv: Optional[Iterable[str]] = None) -> int:
    """Command-line entry point for ``python -m dcc_mcp_core.install_lifecycle``."""
    parser = argparse.ArgumentParser(description="Import-light DCC-MCP install lifecycle helpers.")
    sub = parser.add_subparsers(dest="command", required=True)

    query = sub.add_parser("query", help="Read registered runtimes from services.json.")
    query.add_argument("--registry-dir")
    query.add_argument("--dcc-type")
    query.add_argument("--role")
    query.add_argument("--install-root")
    query.add_argument("--include-dead", action="store_true", default=False)

    stop = sub.add_parser("stop", help="Stop registered sidecars without importing _core.")
    stop.add_argument("--registry-dir")
    stop.add_argument("--dcc-type")
    stop.add_argument("--role", default=ROLE_PER_DCC_SIDECAR)
    stop.add_argument("--install-root")
    stop.add_argument("--timeout-secs", type=float, default=5.0)
    stop.add_argument("--include-host-processes", action="store_true")

    layout = sub.add_parser("layout", help="Resolve Rez or filesystem deployment roots.")
    layout.add_argument("--cache-root")
    layout.add_argument("--package", action="append", dest="packages")
    layout.add_argument("--adapter-package")

    sidecar_command = sub.add_parser("sidecar-command", help="Build a dcc-mcp-server sidecar argv.")
    _add_sidecar_launch_args(sidecar_command)

    launch = sub.add_parser("launch-sidecar", help="Start a sidecar without importing _core.")
    _add_sidecar_launch_args(launch)
    launch.add_argument("--foreground", action="store_true", help="Do not detach the sidecar on Windows.")

    plan = sub.add_parser("plan-update", help="Plan restart actions for mixed runtime versions.")
    plan.add_argument("--registry-dir")
    plan.add_argument("--dcc-type")
    plan.add_argument("--role")
    plan.add_argument(
        "--target-version",
        action="append",
        default=[],
        help="Target version as KEY=VERSION, for example core=0.17.21.",
    )

    inspect = sub.add_parser("inspect", help="Inspect an install root for loaded native artifacts.")
    inspect.add_argument("install_root")

    remove = sub.add_parser("remove", help="Remove a tree or classify lock failures.")
    remove.add_argument("path")

    replace = sub.add_parser("replace", help="Replace a tree or classify lock failures.")
    replace.add_argument("source")
    replace.add_argument("destination")

    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.command == "query":
        return _print_json(
            query_runtime_state(
                args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                install_root=args.install_root,
                include_dead=args.include_dead,
            )
        )
    if args.command == "stop":
        return _print_json(
            stop_runtime_entries(
                args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                install_root=args.install_root,
                timeout_secs=args.timeout_secs,
                include_host_processes=args.include_host_processes,
            )
        )
    if args.command == "layout":
        return _print_json(
            resolve_deployment_layout(
                args.cache_root,
                packages=args.packages,
                adapter_package=args.adapter_package,
            )
        )
    if args.command == "sidecar-command":
        return _print_json(build_sidecar_command(**_sidecar_launch_kwargs(args)))
    if args.command == "launch-sidecar":
        return _print_json(
            launch_sidecar(
                **_sidecar_launch_kwargs(args),
                detached=not args.foreground,
            )
        )
    if args.command == "plan-update":
        try:
            target_versions = _parse_target_versions(args.target_version)
        except ValueError as exc:
            return _print_json(_failed("invalid_target_version", str(exc), None))
        return _print_json(
            plan_runtime_updates(
                registry_dir=args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                target_versions=target_versions,
            )
        )
    if args.command == "inspect":
        return _print_json(inspect_install_root(args.install_root))
    if args.command == "remove":
        return _print_json(safe_remove_tree(args.path))
    if args.command == "replace":
        return _print_json(safe_replace_tree(args.source, args.destination))
    parser.error("unknown command")
    return 2


def _add_sidecar_launch_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--dcc-type", "--dcc", dest="dcc_type", required=True)
    parser.add_argument("--host-rpc", required=True)
    parser.add_argument("--watch-pid", type=int, required=True)
    parser.add_argument("--registry-dir")
    parser.add_argument("--server-bin")
    parser.add_argument("--instance-id")
    parser.add_argument("--display-name")
    parser.add_argument("--adapter-version")
    parser.add_argument("--gateway-port", type=int)
    parser.add_argument("--gateway-host")
    parser.add_argument("--gateway-name")
    parser.add_argument("--gateway-remote-host")
    parser.add_argument("--gateway-remote-port", type=int)
    parser.add_argument("--connect-timeout-secs", type=int)
    parser.add_argument("--no-ensure-gateway", action="store_true")
    parser.add_argument("--legacy-gateway-election", action="store_true")


def _sidecar_launch_kwargs(args: argparse.Namespace) -> Dict[str, Any]:
    return {
        "dcc_type": args.dcc_type,
        "host_rpc": args.host_rpc,
        "watch_pid": args.watch_pid,
        "registry_dir": args.registry_dir,
        "server_bin": args.server_bin,
        "instance_id": args.instance_id,
        "display_name": args.display_name,
        "adapter_version": args.adapter_version,
        "gateway_port": args.gateway_port,
        "gateway_host": args.gateway_host,
        "gateway_name": args.gateway_name,
        "gateway_remote_host": args.gateway_remote_host,
        "gateway_remote_port": args.gateway_remote_port,
        "connect_timeout_secs": args.connect_timeout_secs,
        "no_ensure_gateway": args.no_ensure_gateway,
        "legacy_gateway_election": args.legacy_gateway_election,
    }


if __name__ == "__main__":
    raise SystemExit(main())
