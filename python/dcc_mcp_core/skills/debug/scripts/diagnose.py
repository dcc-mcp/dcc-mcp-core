"""diagnose — Run the full diagnostic pipeline on a failed tool call.

Composes dcc_diagnostics__error_report + audit_log + tool_metrics + screenshot.
"""
from __future__ import annotations

import json
import subprocess
import sys
from typing import Any


def _run_diagnostics_tool(instance: str, tool: str, args: dict, timeout: int = 30) -> dict[str, Any] | None:
    """Call a dcc_diagnostics tool via CLI."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", f"{instance}.{tool}", "--json", json.dumps(args), "--output", "json"],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
        return {"error": result.stderr}
    except subprocess.TimeoutExpired:
        return {"error": "timeout"}
    except Exception as e:
        return {"error": str(e)}


def _resolve_instance(dcc_name: str | None = None) -> str | None:
    """Resolve the first ready instance for a DCC type."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "list", "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        for inst in data.get("instances", []):
            if dcc_name and inst.get("dcc_type") != dcc_name:
                continue
            if inst.get("direct_control", {}).get("ready"):
                return inst.get("instance_short") or inst.get("instance_id")
        # Fallback to first instance
        if data.get("instances"):
            return data["instances"][0].get("instance_short")
    except Exception:
        pass
    return None


def diagnose(
    dcc_name: str | None = None,
    failed_action: str | None = None,
    error_message: str | None = None,
    tail_lines: int = 200,
    include_screenshot: bool = True,
) -> dict[str, Any]:
    """Run full diagnostic pipeline.

    Args:
        dcc_name: DCC name (e.g. 'maya').
        failed_action: The tool that failed.
        error_message: The error message.
        tail_lines: Log tail lines.
        include_screenshot: Whether to capture screenshot.

    Returns:
        Structured diagnosis report.
    """
    instance = _resolve_instance(dcc_name)
    if not instance:
        return {
            "success": False,
            "error": "No DCC instance found. Start a DCC adapter first.",
            "steps": {},
        }

    steps: dict[str, Any] = {}

    # Step 1: Error report
    error_args: dict[str, Any] = {"tail_lines": tail_lines}
    if dcc_name:
        error_args["dcc_name"] = dcc_name
    steps["error_report"] = _run_diagnostics_tool(instance, "dcc_diagnostics__error_report", error_args) or {}

    # Step 2: Audit log
    audit_args: dict[str, Any] = {"filter": "all", "limit": 50}
    steps["audit_log"] = _run_diagnostics_tool(instance, "dcc_diagnostics__audit_log", audit_args) or {}

    # Step 3: Tool metrics
    steps["tool_metrics"] = _run_diagnostics_tool(
        instance, "dcc_diagnostics__tool_metrics",
        {"sort_by": "failure_rate", "limit": 10},
    ) or {}

    # Step 4: Screenshot (optional)
    if include_screenshot:
        steps["screenshot"] = _run_diagnostics_tool(
            instance, "dcc_diagnostics__screenshot",
            {"format": "png"},
        ) or {}

    # Synthesize hints
    hints: list[str] = []
    er = steps.get("error_report", {})
    if er:
        log_errors = er.get("log_errors", er.get("errors", []))
        if log_errors:
            hints.append("Log file contains errors — check error_report for details.")
        failed_jobs = er.get("failed_jobs", [])
        if failed_jobs:
            hints.append(f"{len(failed_jobs)} recent failed jobs found.")

    al = steps.get("audit_log", {})
    if al:
        denied = [e for e in al.get("entries", []) if e.get("outcome") == "denied"]
        if denied:
            hints.append(f"{len(denied)} sandbox denial(s) detected. Check audit_log.")

    if failed_action:
        hints.insert(0, f"Diagnosing failure of '{failed_action}': {error_message or 'no error detail'}")

    if not hints:
        hints.append("No obvious issues found. Try trace_tool for deeper investigation.")

    return {
        "success": True,
        "dcc_name": dcc_name or "auto-detected",
        "instance": instance,
        "failed_action": failed_action,
        "steps": steps,
        "diagnosis_hints": hints,
        "recommended_next": "If root cause unclear, use inspect_state or trace_tool for deeper analysis.",
    }
