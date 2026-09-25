"""Gateway registry state: directory resolution, sentinel writes, version reads.

Split out of ``gateway_guardian.py`` so the guardian keeps one responsibility:
deciding *whether* to (re)start the gateway. Reading and writing the file
registry rows that describe an already-running gateway is a separate concern
and lives here.
"""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import time
import uuid

from dcc_mcp_core.install_lifecycle import default_registry_dir


def _resolve_registry_dir(registry_dir: str | None) -> Path:
    if registry_dir:
        return Path(registry_dir).expanduser()
    return Path(default_registry_dir()).expanduser()


# ── Sentinel entry helper (for version-aware takeover) ──


@contextlib.contextmanager
def _registry_write_lock(path: Path):
    """Take the same first-byte lock covered by Rust ``services.lock``."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as handle:
        handle.seek(0)
        if os.name == "nt":
            import msvcrt

            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)

            def unlock():
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)

        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)

            def unlock():
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)

        try:
            yield
        finally:
            handle.seek(0)
            with contextlib.suppress(OSError):
                unlock()


def _write_sentinel_entry(
    registry_dir: str | None,
    *,
    gateway_host: str,
    gateway_port: int,
    crate_version: str,
    adapter_version: str | None = None,
    adapter_dcc: str | None = None,
) -> bool:
    """Write a sentinel entry to the file registry to trigger gateway yield.

    The running gateway's 15 s cleanup loop calls ``has_newer_sentinel`` and
    will voluntarily yield when a newer version sentinel is found.

    Returns True if the sentinel was written; False on error.
    """
    registry_path = _resolve_registry_dir(registry_dir)
    services_file = registry_path / "services.json"
    now = time.time()
    sentinel_entry: dict[str, object] = {
        "dcc_type": "__gateway__",
        "instance_id": str(uuid.uuid5(uuid.NAMESPACE_URL, f"dcc-mcp://gateway/{gateway_host}:{gateway_port}")),
        "host": gateway_host,
        "port": gateway_port,
        "version": crate_version,
        "registered_at": now,
        "last_heartbeat": now,
        "status": "available",
    }
    if adapter_version:
        sentinel_entry["adapter_version"] = adapter_version
    if adapter_dcc:
        sentinel_entry["adapter_dcc"] = adapter_dcc

    try:
        with _registry_write_lock(registry_path / "services.lock"):
            raw = services_file.read_text(encoding="utf-8") if services_file.exists() else ""
            data = json.loads(raw) if raw.strip() else []
            if isinstance(data, list):
                data = [e for e in data if not (isinstance(e, dict) and e.get("dcc_type") == "__gateway__")]
                data.append(sentinel_entry)
            elif isinstance(data, dict):
                data[f"__gateway__:{gateway_host}:{gateway_port}"] = sentinel_entry
            else:
                data = [sentinel_entry]
            temp_file = registry_path / f".tmp.{os.getpid()}.guardian.json"
            temp_file.write_text(json.dumps(data, indent=2), encoding="utf-8")
            temp_file.replace(services_file)
        return True
    except Exception:
        return False


def _read_gateway_version_from_registry(
    registry_dir: str | None,
    *,
    gateway_host: str,
    gateway_port: int,
) -> str | None:
    """Read the running gateway's version from the file registry sentinel entry.

    Returns the version string if found, or None.
    """
    try:
        registry_path = _resolve_registry_dir(registry_dir)
        services_file = registry_path / "services.json"
        if not services_file.exists():
            return None
        raw = services_file.read_text(encoding="utf-8")
        data = json.loads(raw) if raw.strip() else []
    except Exception:
        return None

    # The FileRegistry stores entries in either list or dict format.
    if isinstance(data, dict):
        sentinel_key = f"__gateway__:{gateway_host}:{gateway_port}"
        entry = data.get(sentinel_key)
        if isinstance(entry, dict):
            version = entry.get("version")
            if isinstance(version, str):
                return version
        return None

    if isinstance(data, list):
        for entry in data:
            if not isinstance(entry, dict):
                continue
            if entry.get("dcc_type") == "__gateway__":
                if entry.get("host") != gateway_host:
                    continue
                try:
                    if int(entry.get("port", 0)) != int(gateway_port):
                        continue
                except (TypeError, ValueError):
                    continue
                version = entry.get("version")
                if isinstance(version, str):
                    return version
    return None


def _read_managed_gateway_version_from_registry(
    registry_dir: str | None,
    *,
    gateway_host: str,
    gateway_port: int,
) -> str | None:
    """Return the version only for a process-owned Rust gateway sentinel."""
    try:
        services_file = _resolve_registry_dir(registry_dir) / "services.json"
        raw = services_file.read_text(encoding="utf-8")
        data = json.loads(raw) if raw.strip() else []
    except Exception:
        return None

    entries: list[object]
    if isinstance(data, dict):
        sentinel_key = f"__gateway__:{gateway_host}:{gateway_port}"
        entries = [data.get(sentinel_key)]
    elif isinstance(data, list):
        entries = data
    else:
        return None

    for entry in entries:
        if not isinstance(entry, dict) or entry.get("dcc_type") != "__gateway__":
            continue
        if entry.get("host") != gateway_host:
            continue
        try:
            if int(entry.get("port", 0)) != int(gateway_port):
                continue
            pid = int(entry.get("pid", 0))
        except (TypeError, ValueError):
            continue
        instance_id = entry.get("instance_id")
        version = entry.get("version")
        if pid > 0 and isinstance(instance_id, str) and instance_id and isinstance(version, str):
            return version
    return None
