"""collect_evidence — Bundle logs, screenshots, and metrics for bug reports."""
from __future__ import annotations

import json
import os
import subprocess
import time
from typing import Any


def collect_evidence(
    dcc_name: str | None = None,
    session_id: str | None = None,
    include_screenshot: bool = True,
    tail_lines: int = 300,
    job_limit: int = 30,
    output_dir: str | None = None,
) -> dict[str, Any]:
    """Collect comprehensive evidence bundle.

    Args:
        dcc_name: DCC name.
        session_id: Gateway session ID for stats.
        include_screenshot: Include visual evidence.
        tail_lines: Log lines to capture.
        job_limit: Max recent jobs.
        output_dir: Directory for output files.

    Returns:
        Evidence bundle metadata.
    """
    evidence: dict[str, Any] = {
        "collected_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "dcc_name": dcc_name or "auto",
        "session_id": session_id,
    }

    files: list[str] = []
    output_base = output_dir or os.path.join(os.environ.get("TEMP", "/tmp"), "dcc-mcp-evidence")

    # Collect error report
    try:
        args = ["dcc-mcp-cli", "call"]
        if dcc_name:
            args.extend([f"<instance>.dcc_diagnostics__error_report", "--json",
                        json.dumps({"dcc_name": dcc_name, "tail_lines": tail_lines})])
        result = subprocess.run(args + ["--output", "json"], capture_output=True, text=True, timeout=30)
        evidence["error_report"] = json.loads(result.stdout) if result.returncode == 0 else {"error": result.stderr}
    except Exception as e:
        evidence["error_report"] = {"error": str(e)}

    # Collect tool metrics
    try:
        args = ["dcc-mcp-cli", "call"]
        result = subprocess.run(
            args + ["<instance>.dcc_diagnostics__tool_metrics",
                    "--json", json.dumps({"sort_by": "failure_rate", "limit": 10}),
                    "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        evidence["tool_metrics"] = json.loads(result.stdout) if result.returncode == 0 else {}
    except Exception:
        evidence["tool_metrics"] = {}

    # Collect screenshot
    if include_screenshot:
        try:
            save_path = os.path.join(output_base, f"screenshot-{dcc_name or 'dcc'}-{int(time.time())}.png")
            os.makedirs(output_base, exist_ok=True)
            args = ["dcc-mcp-cli", "call"]
            result = subprocess.run(
                args + ["<instance>.dcc_diagnostics__screenshot",
                        "--json", json.dumps({"format": "png", "save_path": save_path}),
                        "--output", "json"],
                capture_output=True, text=True, timeout=15,
            )
            if result.returncode == 0 and os.path.exists(save_path):
                files.append(save_path)
                evidence["screenshot"] = save_path
            else:
                evidence["screenshot"] = "unavailable"
        except Exception:
            evidence["screenshot"] = "unavailable"

    # Stats via CLI
    if session_id:
        try:
            result = subprocess.run(
                ["dcc-mcp-cli", "stats", "--range", "24h", "--session-id", session_id, "--output", "json"],
                capture_output=True, text=True, timeout=15,
            )
            evidence["stats"] = json.loads(result.stdout) if result.returncode == 0 else {}
        except Exception:
            evidence["stats"] = {}

    # Inventory snapshot
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "list", "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        evidence["inventory"] = json.loads(result.stdout) if result.returncode == 0 else {}
    except Exception:
        evidence["inventory"] = {}

    evidence["files"] = files
    evidence["saved_to"] = output_base if files else None
    evidence["success"] = True
    return evidence
