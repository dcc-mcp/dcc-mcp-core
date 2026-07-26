"""check_sandbox — Audit sandbox permissions for denied calls."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def check_sandbox(
    action_name: str | None = None,
    dcc_name: str | None = None,
    recent_limit: int = 50,
) -> dict[str, Any]:
    """Audit sandbox permissions.

    Args:
        action_name: Specific action to check.
        dcc_name: DCC name filter.
        recent_limit: Number of recent entries to return.

    Returns:
        Sandbox audit report.
    """
    # Query audit log for denials
    try:
        args = ["dcc-mcp-cli", "call"]
        filter_type = "denied"
        audit_args: dict[str, Any] = {"filter": filter_type, "limit": recent_limit}
        if action_name:
            audit_args["action_name"] = action_name

        result = subprocess.run(
            args + ["<instance>.dcc_diagnostics__audit_log",
                    "--json", json.dumps(audit_args), "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0:
            audit_data = json.loads(result.stdout)
        else:
            audit_data = {"error": result.stderr}
    except Exception as e:
        audit_data = {"error": str(e)}

    # Also get all recent entries for context
    try:
        args = ["dcc-mcp-cli", "call"]
        result = subprocess.run(
            args + ["<instance>.dcc_diagnostics__audit_log",
                    "--json", json.dumps({"filter": "all", "limit": min(recent_limit, 20)}),
                    "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        all_entries = json.loads(result.stdout) if result.returncode == 0 else {}
    except Exception:
        all_entries = {}

    denied_count = len(audit_data.get("entries", [])) if "entries" in audit_data else 0

    recommendations: list[str] = []
    if denied_count > 0 and action_name:
        recommendations.append(
            f"'{action_name}' has {denied_count} recent denial(s). "
            "Check sandbox policy and consider adding an allow rule."
        )
    elif denied_count > 0:
        recommendations.append(
            f"{denied_count} denied action(s) found. Review audit_log for details."
        )
    else:
        recommendations.append("No recent sandbox denials found.")

    return {
        "success": True,
        "action_name": action_name,
        "denied_count": denied_count,
        "denials": audit_data.get("entries", [])[:recent_limit] if "entries" in audit_data else [],
        "recent_activity": all_entries.get("entries", [])[:10] if "entries" in all_entries else [],
        "recommendations": recommendations,
    }
