"""Execute a sequence of dcc-mcp-core actions in order (action chain).

Dispatch strategy:
1. IPC dispatch — if DCC_MCP_IPC_ADDRESS is set, each step is sent to the
   running DCC server via FramedChannel.call("dispatch_tool", ...).
   This is the production path (Maya, Blender, Unreal, etc.).
2. Local ToolDispatcher — fallback for testing without a live DCC server.
   Only actions registered in the local process are available.

Context propagation:
- Each step's output "context" dict is merged into the shared context.
- Use {key} placeholders in params to inject context values.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from typing import Any


def _interpolate(value: Any, context: dict) -> Any:
    """Recursively interpolate {key} placeholders using context."""
    if isinstance(value, str):

        def replace(match: re.Match) -> str:
            key = match.group(1)
            return str(context.get(key, match.group(0)))

        return re.sub(r"\{(\w+)\}", replace, value)
    if isinstance(value, dict):
        return {k: _interpolate(v, context) for k, v in value.items()}
    if isinstance(value, list):
        return [_interpolate(item, context) for item in value]
    return value


def _build_ipc_dispatcher(ipc_address: str):
    """Return a callable that dispatches actions via IPC to the DCC server."""
    from dcc_mcp_core import TransportAddress
    from dcc_mcp_core import connect_ipc

    addr = TransportAddress.parse(ipc_address)

    def dispatch(action_name: str, params: dict) -> dict:
        # Open a fresh channel per call (stateless, safe for sequential chains)
        channel = connect_ipc(addr, timeout_ms=5000)
        try:
            payload = json.dumps({"action": action_name, "params": params}).encode()
            result = channel.call("dispatch_tool", payload, timeout_ms=30000)
            if not result.get("success"):
                return {"success": False, "message": f"IPC dispatch error: {result.get('error')}"}
            raw = result.get("payload", b"{}")
            if isinstance(raw, (bytes, bytearray)):
                return json.loads(raw.decode())
            return json.loads(raw) if isinstance(raw, str) else raw
        finally:
            channel.shutdown()

    return dispatch


def _build_local_dispatcher():
    """Return a callable that dispatches via a local ToolDispatcher."""
    from dcc_mcp_core import ToolDispatcher
    from dcc_mcp_core import ToolRegistry

    registry = ToolRegistry()
    dispatcher = ToolDispatcher(registry)

    def dispatch(action_name: str, params: dict) -> dict:
        params_json = json.dumps(params)
        raw = dispatcher.dispatch(action_name, params_json)
        output = raw.get("output", "{}")
        if isinstance(output, str):
            try:
                return json.loads(output)
            except json.JSONDecodeError:
                return {"success": False, "message": output}
        return output if isinstance(output, dict) else {"success": False, "message": str(output)}

    return dispatch


def main() -> None:
    """Run a sequence of actions as a chain and print JSON result to stdout."""
    parser = argparse.ArgumentParser(description="Run a sequence of actions as a chain.")
    parser.add_argument("--steps", default=None, help="JSON array of step definitions.")
    parser.add_argument("--context", default=None, help="JSON object with initial context values.")
    args = parser.parse_args()

    # Prefer CLI args; fall back to reading the full params JSON from stdin.
    # dcc-mcp-core execute_script writes params JSON to stdin alongside CLI flags,
    # so complex values (arrays, nested objects) arrive via stdin.
    if args.steps is None:
        try:
            raw = sys.stdin.read()
            stdin_params = json.loads(raw) if raw.strip() else {}
        except Exception:
            stdin_params = {}
        steps_str = json.dumps(stdin_params.get("steps", []))
        context_str = json.dumps(stdin_params.get("context", {}))
    else:
        steps_str = args.steps
        context_str = args.context or "{}"

    try:
        steps = json.loads(steps_str)
        context: dict = json.loads(context_str)
    except json.JSONDecodeError as exc:
        print(json.dumps({"success": False, "message": f"Invalid JSON input: {exc}"}))
        sys.exit(1)

    if not isinstance(steps, list) or not steps:
        print(json.dumps({"success": False, "message": "'steps' must be a non-empty JSON array."}))
        sys.exit(1)

    try:
        from dcc_mcp_core import ToolDispatcher  # noqa: F401 — just check import
    except ImportError:
        print(json.dumps({"success": False, "message": "dcc_mcp_core not available. Install the package first."}))
        sys.exit(1)

    # Choose dispatch strategy
    ipc_address = os.environ.get("DCC_MCP_IPC_ADDRESS")
    source = "ipc"
    try:
        if ipc_address:
            dispatch = _build_ipc_dispatcher(ipc_address)
        else:
            dispatch = _build_local_dispatcher()
            source = "local"
    except Exception as exc:
        print(json.dumps({"success": False, "message": f"Failed to initialise dispatcher: {exc}"}))
        sys.exit(1)

    results = []
    chain_success = True
    aborted_at: int | None = None

    for idx, step in enumerate(steps):
        action = step.get("action", "")
        label = step.get("label") or action
        stop_on_failure = step.get("stop_on_failure", True)
        raw_params = step.get("params") or {}

        if not action:
            results.append(
                {
                    "step": idx,
                    "label": label,
                    "action": "(missing)",
                    "success": False,
                    "message": "Step is missing 'action' field.",
                    "duration_ms": 0,
                }
            )
            if stop_on_failure:
                chain_success = False
                aborted_at = idx
                break
            continue

        params = _interpolate(raw_params, context)

        start = time.monotonic()
        try:
            result = dispatch(action, params)
        except Exception as exc:
            result = {"success": False, "message": f"Dispatch error: {exc}"}
        duration_ms = round((time.monotonic() - start) * 1000, 1)

        step_success = bool(result.get("success", False))
        step_entry = {
            "step": idx,
            "label": label,
            "action": action,
            "success": step_success,
            "message": result.get("message", ""),
            "duration_ms": duration_ms,
        }
        if result.get("context"):
            step_entry["output_context"] = result["context"]

        results.append(step_entry)

        # Propagate step output into shared context
        if isinstance(result.get("context"), dict):
            context.update(result["context"])

        if not step_success:
            chain_success = False
            if stop_on_failure:
                aborted_at = idx
                break

    completed = len(results)
    total = len(steps)
    failed_steps = [r for r in results if not r["success"]]

    if chain_success:
        message = f"Chain completed: {completed}/{total} steps succeeded (dispatch={source})."
        prompt = (
            "All steps completed successfully. "
            "Check 'results' for per-step output and accumulated context. "
            "You can proceed to the next task or run another chain."
        )
    else:
        if aborted_at is not None:
            step_label = results[aborted_at]["label"]
            message = f"Chain aborted at step {aborted_at} ({step_label!r}): {results[aborted_at]['message']}"
            prompt = (
                f"Chain failed at step {aborted_at} ('{step_label}'). "
                "Use dcc_diagnostics__screenshot to capture the current state, "
                "dcc_diagnostics__audit_log to inspect recent action history, "
                "or fix the failing step and re-run the chain."
            )
        else:
            failed_count = len(failed_steps)
            message = (
                f"Chain completed with {failed_count}/{total} failed step(s) (dispatch={source}). "
                "All steps ran (stop_on_failure=False)."
            )
            prompt = (
                f"{failed_count} step(s) failed but the chain ran to completion. "
                "Use dcc_diagnostics__audit_log to inspect recent action history "
                "or fix the failing steps and re-run the chain."
            )

    print(
        json.dumps(
            {
                "success": chain_success,
                "message": message,
                "prompt": prompt,
                "context": {
                    "completed_steps": completed,
                    "total_steps": total,
                    "aborted_at": aborted_at,
                    "failed_count": len(failed_steps),
                    "dispatch_source": source,
                    "ipc_address": ipc_address,
                    "accumulated_context": context,
                    "results": results,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
