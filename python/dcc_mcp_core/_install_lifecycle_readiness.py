"""Import-light sidecar dispatch readiness helpers."""

# ruff: noqa: UP006, UP045

from __future__ import annotations

import time
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"
DISPATCH_STATUS_BOOTING = "booting"
DISPATCH_STATUS_UNAVAILABLE = "unavailable"


def sidecar_readiness_status(
    registry_dir: Optional[Any] = None,
    *,
    dcc_type: Optional[str] = None,
    instance_id: Optional[str] = None,
    host_rpc: Optional[str] = None,
    include_dead: bool = True,
) -> Dict[str, Any]:
    """Return a one-shot, import-light sidecar dispatch-readiness verdict."""
    state = _query_runtime_state(
        registry_dir,
        dcc_type=dcc_type,
        role=ROLE_PER_DCC_SIDECAR,
        include_dead=include_dead,
    )
    entries = _filter_sidecar_readiness_entries(
        state.get("entries", []),
        instance_id=instance_id,
        host_rpc=host_rpc,
    )
    selector = {
        "dcc_type": dcc_type,
        "instance_id": instance_id,
        "host_rpc": host_rpc,
    }

    if not entries:
        return {
            "success": False,
            "status": "missing",
            "ready": False,
            "selector": selector,
            "entries": [],
            "message": "No matching per-DCC sidecar is registered.",
            "recommended_next_action": "Launch the sidecar from the DCC startup hook, then check readiness again.",
        }

    ready = [entry for entry in entries if entry.get("dispatch_ready") is True]
    if ready:
        return {
            "success": True,
            "status": "ready",
            "ready": True,
            "selector": selector,
            "entry": ready[0],
            "entries": entries,
            "message": "Sidecar dispatch is ready.",
            "recommended_next_action": "Use the shared gateway URL or the entry mcp_url for tool calls.",
        }

    unavailable = [entry for entry in entries if entry.get("dispatch_status") == DISPATCH_STATUS_UNAVAILABLE]
    if unavailable:
        entry = unavailable[0]
        return {
            "success": False,
            "status": "unavailable",
            "ready": False,
            "selector": selector,
            "entry": entry,
            "entries": entries,
            "failure_stage": entry.get("failure_stage"),
            "failure_reason": entry.get("failure_reason"),
            "message": "Sidecar registered, but host dispatch is unavailable.",
            "recommended_next_action": (
                "Fix the adapter host RPC bridge or dispatcher, restart the sidecar, then check readiness again."
            ),
        }

    alive = [entry for entry in entries if entry.get("runtime_alive") is not False]
    if alive:
        status = alive[0].get("dispatch_status") or DISPATCH_STATUS_BOOTING
        return {
            "success": False,
            "status": status,
            "ready": False,
            "selector": selector,
            "entry": alive[0],
            "entries": entries,
            "message": "Sidecar is registered but dispatch is not ready yet.",
            "recommended_next_action": (
                "Keep polling dispatch readiness or inspect failure metadata if it becomes unavailable."
            ),
        }

    return {
        "success": False,
        "status": "dead",
        "ready": False,
        "selector": selector,
        "entry": entries[0],
        "entries": entries,
        "message": "Matching sidecar rows are stale or their runtime process is not alive.",
        "recommended_next_action": "Restart the sidecar from the live DCC process.",
    }


def wait_for_sidecar_ready(
    registry_dir: Optional[Any] = None,
    *,
    dcc_type: Optional[str] = None,
    instance_id: Optional[str] = None,
    host_rpc: Optional[str] = None,
    timeout_secs: float = 10.0,
    poll_interval_secs: float = 0.25,
) -> Dict[str, Any]:
    """Poll sidecar readiness without importing native core code."""
    timeout = max(0.0, float(timeout_secs))
    poll_interval = max(0.05, float(poll_interval_secs))
    started = time.monotonic()
    deadline = started + timeout
    last = sidecar_readiness_status(
        registry_dir,
        dcc_type=dcc_type,
        instance_id=instance_id,
        host_rpc=host_rpc,
        include_dead=True,
    )

    while True:
        status = last.get("status")
        if last.get("success") or status == DISPATCH_STATUS_UNAVAILABLE:
            last["elapsed_secs"] = round(time.monotonic() - started, 3)
            return last
        if time.monotonic() >= deadline:
            return {
                **last,
                "success": False,
                "ready": False,
                "status": "timeout",
                "last_status": status,
                "elapsed_secs": round(time.monotonic() - started, 3),
                "message": "Timed out waiting for sidecar dispatch readiness.",
                "recommended_next_action": (
                    "Check the sidecar registry row, host RPC endpoint, and adapter dispatcher logs."
                ),
            }
        time.sleep(poll_interval)
        last = sidecar_readiness_status(
            registry_dir,
            dcc_type=dcc_type,
            instance_id=instance_id,
            host_rpc=host_rpc,
            include_dead=True,
        )


def _query_runtime_state(*args: Any, **kwargs: Any) -> Dict[str, Any]:
    from .install_lifecycle import query_runtime_state

    return query_runtime_state(*args, **kwargs)


def _filter_sidecar_readiness_entries(
    entries: Iterable[Dict[str, Any]],
    *,
    instance_id: Optional[str],
    host_rpc: Optional[str],
) -> List[Dict[str, Any]]:
    result = []
    instance_selector = str(instance_id).strip() if instance_id else None
    host_rpc_selector = str(host_rpc).strip() if host_rpc else None
    for entry in entries:
        if instance_selector and not _instance_id_matches(entry.get("instance_id"), instance_selector):
            continue
        if host_rpc_selector and entry.get("host_rpc_uri") != host_rpc_selector:
            continue
        result.append(entry)
    return result


def _instance_id_matches(value: Any, selector: str) -> bool:
    if value in (None, ""):
        return False
    text = str(value)
    return text == selector or text.startswith(selector)
