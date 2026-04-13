"""Query the dcc-mcp-core sandbox audit log.

Data source priority:
1. IPC callback — if DCC_MCP_IPC_ADDRESS is set, connect to the running DCC
   server and call the 'get_audit_log' action to get live data.
2. Local SandboxContext — creates a fresh (empty) context as a fallback,
   useful for testing without a running DCC server.
"""

from __future__ import annotations

import argparse
import json
import os
import sys


def _fetch_via_ipc(ipc_address: str, filter_: str, action_name: str | None, limit: int) -> dict | None:
    """Connect to the DCC server via IPC and call 'get_audit_log'."""
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
        params = json.dumps(
            {
                "filter": filter_,
                "action_name": action_name,
                "limit": limit,
            }
        ).encode()
        result = channel.call("get_audit_log", params, timeout_ms=10000)
        channel.shutdown()

        if not result.get("success"):
            return {"success": False, "message": f"IPC call failed: {result.get('error')}"}

        payload = result.get("payload", b"{}")
        if isinstance(payload, (bytes, bytearray)):
            return json.loads(payload.decode())
        return json.loads(payload) if isinstance(payload, str) else payload
    except Exception as exc:
        return {"success": False, "message": f"IPC call error: {exc}"}


def _fetch_local(filter_: str, action_name: str | None, limit: int) -> dict:
    """Read from a local SandboxContext (fallback; typically empty in subprocess)."""
    try:
        from dcc_mcp_core import AuditLog
        from dcc_mcp_core import SandboxContext
        from dcc_mcp_core import SandboxPolicy
    except ImportError:
        return {"success": False, "message": "dcc_mcp_core not available. Install the package first."}

    try:
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        audit: AuditLog = ctx.audit_log
    except Exception as exc:
        return {"success": False, "message": f"Failed to access audit log: {exc}"}

    try:
        if action_name:
            entries = audit.entries_for_action(action_name)
        elif filter_ == "success":
            entries = audit.successes()
        elif filter_ == "denied":
            entries = audit.denials()
        else:
            entries = audit.entries()
    except Exception as exc:
        return {"success": False, "message": f"Failed to read audit entries: {exc}"}

    total = len(entries)
    serialized = []
    for entry in entries[:limit]:
        try:
            serialized.append(
                {
                    "action": entry.action,
                    "outcome": entry.outcome,
                    "timestamp_ms": getattr(entry, "timestamp_ms", None),
                    "details": getattr(entry, "details", None),
                }
            )
        except Exception:
            serialized.append(str(entry))

    return {
        "success": True,
        "total_entries": total,
        "entries": serialized,
        "source": "local",
    }


def main() -> None:
    """Query the sandbox audit log and print JSON result to stdout."""
    parser = argparse.ArgumentParser(description="Query the sandbox audit log.")
    parser.add_argument("--filter", default="all", choices=["all", "success", "denied", "error"])
    parser.add_argument("--action-name", default=None, dest="action_name")
    parser.add_argument("--limit", type=int, default=50)
    args = parser.parse_args()

    ipc_address = os.environ.get("DCC_MCP_IPC_ADDRESS")

    data = _fetch_via_ipc(ipc_address, args.filter, args.action_name, args.limit) if ipc_address else None

    if data is None:
        data = _fetch_local(args.filter, args.action_name, args.limit)

    if not data.get("success", True) and "message" in data:
        # Hard error
        print(json.dumps({"success": False, "message": data["message"]}))
        sys.exit(1)

    total = data.get("total_entries", 0)
    entries = data.get("entries", [])
    source = data.get("source", "ipc" if ipc_address else "local")
    filter_desc = f"action={args.action_name!r}" if args.action_name else f"filter={args.filter}"

    empty_note = ""
    if total == 0 and source == "local":
        empty_note = (
            " Log is empty — no sandbox context is active in this subprocess. "
            "Set DCC_MCP_IPC_ADDRESS env var to query the live DCC server."
        )

    print(
        json.dumps(
            {
                "success": True,
                "message": (
                    f"Found {total} audit entries ({filter_desc}, source={source})"
                    + (f", showing first {args.limit}" if total > args.limit else "")
                    + empty_note
                ),
                "prompt": (
                    "Audit log retrieved. If entries show 'denied' outcomes, check the SandboxPolicy. "
                    "Use dcc_diagnostics__action_metrics to see performance data, or "
                    "dcc_diagnostics__screenshot to capture what's currently visible."
                ),
                "context": {
                    "total_entries": total,
                    "filter": args.filter,
                    "action_name_filter": args.action_name,
                    "source": source,
                    "ipc_address": ipc_address,
                    "entries": entries,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
