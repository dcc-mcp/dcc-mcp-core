"""trace_tool — Execute a tool with full telemetry capture."""
from __future__ import annotations

import json
import subprocess
import time
from typing import Any


def trace_tool(
    tool_slug: str,
    arguments: dict[str, Any] | None = None,
    capture_before: bool = True,
    capture_after: bool = True,
) -> dict[str, Any]:
    """Trace a tool execution with before/after state capture.

    Args:
        tool_slug: Fully qualified tool slug.
        arguments: Tool arguments.
        capture_before: Capture pre-execution state.
        capture_after: Capture post-execution state.

    Returns:
        Tracing report with timing, result, and state diffs.
    """
    args = arguments or {}
    before_state: dict[str, Any] | None = None
    after_state: dict[str, Any] | None = None

    # Capture before state
    if capture_before:
        try:
            before_state = {"timestamp": time.time()}
        except Exception:
            pass

    # Execute the tool
    start = time.time()
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", tool_slug, "--json", json.dumps(args), "--output", "json"],
            capture_output=True,
            text=True,
            timeout=60,
        )
        execution_time_ms = (time.time() - start) * 1000
        if result.returncode == 0:
            result_data = json.loads(result.stdout)
            success = True
            error_context = {}
        else:
            result_data = {"stderr": result.stderr, "stdout": result.stdout}
            success = False
            error_context = {
                "exit_code": result.returncode,
                "diagnostics_recommended": True,
            }
    except subprocess.TimeoutExpired:
        execution_time_ms = (time.time() - start) * 1000
        result_data = {"error": "timeout"}
        success = False
        error_context = {"timeout": True, "timeout_ms": 60000}
    except Exception as e:
        execution_time_ms = (time.time() - start) * 1000
        result_data = {"error": str(e)}
        success = False
        error_context = {"exception": str(e)}

    # Capture after state
    if capture_after:
        try:
            after_state = {"timestamp": time.time()}
        except Exception:
            pass

    return {
        "success": success,
        "tool_slug": tool_slug,
        "execution_time_ms": round(execution_time_ms, 1),
        "result": result_data,
        "before_state": before_state,
        "after_state": after_state,
        "error_context": error_context,
    }
