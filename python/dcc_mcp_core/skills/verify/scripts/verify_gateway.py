"""verify_gateway — Check gateway connectivity and health."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def verify_gateway(profile: str | None = None) -> dict[str, Any]:
    """Check gateway health and profile configuration.

    Args:
        profile: Gateway profile name to check.

    Returns:
        Structured gateway connectivity report.
    """
    reachable = False
    health_status = "unknown"
    active_profile = "unknown"
    registry_entries = 0
    version = "unknown"

    # Try health check
    try:
        args = ["dcc-mcp-cli", "health", "--output", "json"]
        result = subprocess.run(args, capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            data = json.loads(result.stdout)
            reachable = data.get("status") == "ok"
            health_status = data.get("status", "unknown")
            version = data.get("version", "unknown")
    except Exception:
        reachable = False

    # Try doctor for profile info
    try:
        args = ["dcc-mcp-cli", "doctor", "--output", "json"]
        result = subprocess.run(args, capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            data = json.loads(result.stdout)
            active_profile = data.get("profile", {}).get("selected", {}).get("mode", "unknown")
    except Exception:
        pass

    # Try list for registry entries
    try:
        args = ["dcc-mcp-cli", "list", "--output", "json"]
        result = subprocess.run(args, capture_output=True, text=True, timeout=10)
        if result.returncode == 0:
            data = json.loads(result.stdout)
            registry_entries = data.get("total", 0)
    except Exception:
        pass

    return {
        "success": True,
        "reachable": reachable,
        "health_status": health_status,
        "active_profile": active_profile,
        "registry_entries": registry_entries,
        "version": version,
    }
