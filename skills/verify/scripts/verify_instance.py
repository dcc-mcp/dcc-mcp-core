"""verify_instance — Check if a DCC instance is dispatch-ready.

Composes dcc-mcp-cli list + doctor to produce a structured verification report.
"""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _run_cli(*args: str, timeout: int = 15) -> dict[str, Any] | None:
    """Run dcc-mcp-cli and return parsed JSON, or None on failure."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", *args, "--output", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if result.returncode != 0:
            return None
        return json.loads(result.stdout)
    except Exception:
        return None


def verify_instance(
    dcc_type: str | None = None,
    instance_id: str | None = None,
    timeout_secs: int = 10,
) -> dict[str, Any]:
    """Check DCC instance readiness.

    Args:
        dcc_type: Filter by DCC type (e.g. 'maya', 'blender').
        instance_id: Filter by specific instance ID.
        timeout_secs: Maximum seconds to wait.

    Returns:
        Structured verification report.
    """
    # Step 1: Get inventory
    inventory = _run_cli("list") or {}
    instances = inventory.get("instances", [])

    if not instances:
        return {
            "success": True,
            "ready": False,
            "instances": [],
            "diagnostics": {
                "failure_stage": "inventory_empty",
                "failure_reason": "No DCC instances registered. Start a DCC adapter first.",
                "recommended_action": "See dcc-mcp/references/ZERO_INSTANCES_CLI.md for setup guidance.",
            },
        }

    # Filter
    if dcc_type:
        instances = [i for i in instances if i.get("dcc_type") == dcc_type]
    if instance_id:
        instances = [i for i in instances if i.get("instance_id") == instance_id or i.get("instance_short") == instance_id]

    # Step 2: Evaluate readiness
    results = []
    any_ready = False
    for inst in instances:
        dc = inst.get("direct_control", {})
        ready = bool(dc.get("ready"))
        if ready:
            any_ready = True
        results.append({
            "instance_id": inst.get("instance_id", inst.get("instance_short", "unknown")),
            "dcc_type": inst.get("dcc_type", "unknown"),
            "ready": ready,
            "dispatch_status": dc.get("dispatch_status", "unknown"),
            "recommended_next_action": dc.get("recommended_next_action", ""),
        })

    diagnostics = {}
    if not any_ready and results:
        # Get doctor output for deeper diagnostics
        doctor = _run_cli("doctor") or {}
        not_ready = doctor.get("local", {}).get("inventory", {}).get("direct_control", {}).get("not_ready_instances", [])
        diagnostics = {
            "failure_stage": "dispatch_not_ready",
            "failure_reason": "Instances exist but none are dispatch-ready.",
            "not_ready_count": len(not_ready),
            "doctor_summary": {
                "profile": doctor.get("profile", {}).get("selected", {}).get("mode", "unknown"),
                "registry_dir": str(doctor.get("local", {}).get("registry_dir", "")),
            },
        }

    return {
        "success": True,
        "ready": any_ready,
        "instances": results,
        "diagnostics": diagnostics,
    }
