"""Inspect per-tool performance metrics.

Data source priority:
1. IPC callback — if DCC_MCP_IPC_ADDRESS is set, connect to the running DCC
   server and call 'get_tool_metrics' to get live recorder data.
2. Local ToolRecorder — creates a fresh (empty) recorder as a fallback,
   useful for testing without a running DCC server.
"""

from __future__ import annotations

import argparse
import json
import os
import sys

_SORT_KEYS = {
    "name": lambda m: m["action_name"],
    "invocations": lambda m: -m["invocation_count"],
    "avg_ms": lambda m: -m["avg_duration_ms"],
    "p95_ms": lambda m: -m["p95_duration_ms"],
    "failure_rate": lambda m: -(1.0 - m["success_rate"]),
}


def _fetch_via_ipc(ipc_address: str, action_name: str | None) -> dict | None:
    """Connect to the DCC server via IPC and call 'get_tool_metrics'."""
    try:
        from dcc_mcp_core import TransportAddress
        from dcc_mcp_core import connect_ipc
    except ImportError:
        return None

    try:
        addr = TransportAddress.parse(ipc_address)
        channel = connect_ipc(addr, timeout_ms=5000)
    except Exception as exc:
        return {"success": False, "message": f"IPC connect failed ({ipc_address}): {exc}"}

    try:
        params = json.dumps({"action_name": action_name}).encode()
        result = channel.call("get_tool_metrics", params, timeout_ms=10000)
        channel.shutdown()

        if not result.get("success"):
            return {"success": False, "message": f"IPC call failed: {result.get('error')}"}

        payload = result.get("payload", b"[]")
        if isinstance(payload, (bytes, bytearray)):
            return {"success": True, "metrics": json.loads(payload.decode()), "source": "ipc"}
        data = json.loads(payload) if isinstance(payload, str) else payload
        return {"success": True, "metrics": data, "source": "ipc"}
    except Exception as exc:
        return {"success": False, "message": f"IPC call error: {exc}"}


def _fetch_local(action_name: str | None) -> dict:
    """Read from a local ToolRecorder (fallback; typically empty in subprocess)."""
    try:
        from dcc_mcp_core import ToolRecorder
    except ImportError:
        return {"success": False, "message": "dcc_mcp_core not available. Install the package first."}

    try:
        recorder = ToolRecorder("dcc-diagnostics")
        if action_name:
            metric = recorder.metrics(action_name)
            metrics_list = [_metric_to_dict(metric)] if metric else []
        else:
            metrics_list = [_metric_to_dict(m) for m in recorder.all_metrics()]
        return {"success": True, "metrics": metrics_list, "source": "local"}
    except Exception as exc:
        return {"success": False, "message": f"Failed to read metrics: {exc}"}


def _metric_to_dict(metric) -> dict:
    return {
        "action_name": metric.action_name,
        "invocation_count": metric.invocation_count,
        "success_count": metric.success_count,
        "failure_count": metric.failure_count,
        "success_rate": round(metric.success_rate(), 4),
        "avg_duration_ms": round(metric.avg_duration_ms, 2),
        "p95_duration_ms": round(metric.p95_duration_ms, 2),
        "p99_duration_ms": round(metric.p99_duration_ms, 2),
    }


def main() -> None:
    """Show action performance metrics and print JSON result to stdout."""
    parser = argparse.ArgumentParser(description="Show action performance metrics.")
    parser.add_argument("--action-name", default=None, dest="action_name")
    parser.add_argument("--sort-by", default="invocations", choices=list(_SORT_KEYS), dest="sort_by")
    parser.add_argument("--limit", type=int, default=20)
    args = parser.parse_args()

    ipc_address = os.environ.get("DCC_MCP_IPC_ADDRESS")

    data = _fetch_via_ipc(ipc_address, args.action_name) if ipc_address else None

    if data is None:
        data = _fetch_local(args.action_name)

    if not data.get("success", True):
        print(json.dumps({"success": False, "message": data.get("message", "Unknown error")}))
        sys.exit(1)

    metrics = data.get("metrics", [])
    source = data.get("source", "ipc" if ipc_address else "local")
    total = len(metrics)

    # Sort
    sort_fn = _SORT_KEYS.get(args.sort_by, _SORT_KEYS["invocations"])
    metrics.sort(key=sort_fn)
    metrics = metrics[: args.limit]

    empty_note = ""
    if total == 0 and source == "local":
        empty_note = " No metrics recorded — set DCC_MCP_IPC_ADDRESS env var to query the live DCC server recorder."

    summary = (
        f"{total} action(s) tracked (source={source})"
        if total > 0
        else f"No metrics recorded yet (source={source}).{empty_note}"
    )

    print(
        json.dumps(
            {
                "success": True,
                "message": summary + (f", showing top {args.limit}" if total > args.limit else ""),
                "prompt": (
                    "Metrics retrieved. High failure_rate or p95_ms values indicate problematic tools. "
                    "Use dcc_diagnostics__audit_log to see recent invocations, or "
                    "dcc_diagnostics__screenshot to capture the current state."
                ),
                "context": {
                    "total_tracked": total,
                    "sort_by": args.sort_by,
                    "source": source,
                    "ipc_address": ipc_address,
                    "metrics": metrics,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
