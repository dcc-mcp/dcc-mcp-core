"""Check DCC process health via dcc_mcp_core.PyProcessMonitor.

Reports which PIDs are being tracked, whether they are alive,
and whether the background monitor loop is running.
"""

from __future__ import annotations

import argparse
import json
import os
import sys


def _pid_alive(pid: int) -> bool:
    """Return True if the process with the given PID is running."""
    if pid <= 0:
        return False
    try:
        # On Unix, signal 0 checks existence without sending a signal.
        os.kill(pid, 0)
        return True
    except (ProcessLookupError, PermissionError):
        # ProcessLookupError → process does not exist
        # PermissionError → process exists but we cannot signal it (still alive)
        return isinstance(sys.exc_info()[1], PermissionError)
    except OSError:
        return False


def main() -> None:
    parser = argparse.ArgumentParser(description="Check DCC process health.")
    parser.add_argument("--pid", type=int, default=None, help="Check a specific PID.")
    args = parser.parse_args()

    try:
        from dcc_mcp_core import PyProcessMonitor
    except ImportError:
        print(json.dumps({"success": False, "message": "dcc_mcp_core not available. Install the package first."}))
        sys.exit(1)

    try:
        monitor = PyProcessMonitor()
        is_running = monitor.is_running()
        tracked_count = monitor.tracked_count()
    except Exception as exc:
        print(json.dumps({"success": False, "message": f"Failed to initialise process monitor: {exc}"}))
        sys.exit(1)

    if args.pid is not None:
        alive = _pid_alive(args.pid)
        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"PID {args.pid} is {'alive' if alive else 'not running'}.",
                    "prompt": (
                        "If the process is not running but should be, use your DCC launcher "
                        "to restart it. You can also call dcc_diagnostics__audit_log to see "
                        "the last actions before the crash."
                    ),
                    "context": {
                        "pid": args.pid,
                        "alive": alive,
                        "monitor_running": is_running,
                        "tracked_pids": tracked_count,
                    },
                }
            )
        )
        return

    # General status — report current process and monitor state
    current_pid = os.getpid()
    print(
        json.dumps(
            {
                "success": True,
                "message": (
                    f"Process monitor {'running' if is_running else 'idle'}, "
                    f"{tracked_count} PID(s) tracked. "
                    f"Current process: PID {current_pid}."
                ),
                "prompt": (
                    "Process status retrieved. To check a specific DCC process, "
                    "call this tool again with --pid <pid>. "
                    "If a DCC is unresponsive, use dcc_diagnostics__screenshot to capture "
                    "the current screen state before restarting."
                ),
                "context": {
                    "current_pid": current_pid,
                    "monitor_running": is_running,
                    "tracked_pids": tracked_count,
                    "platform": sys.platform,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
