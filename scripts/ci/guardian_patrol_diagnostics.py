"""Bounded, nonblocking failure snapshots for guardian patrol tests."""

from __future__ import annotations

import json
from pathlib import Path
import sys
import threading


def _stack_rows(frame):
    rows = []
    for _ in range(8):
        if frame is None:
            break
        rows.append(
            {"file": Path(frame.f_code.co_filename).name, "line": frame.f_lineno, "function": frame.f_code.co_name}
        )
        frame = frame.f_back
    return rows


def guardian_snapshot(guardian, calls, foreign_calls, mocked_module):
    """Record thread/probe ownership without invoking callbacks or acquiring a busy lock."""
    package = sys.modules.get("dcc_mcp_core._server")
    snapshot = {
        "probe_calls": len(calls),
        "foreign_probes": len(foreign_calls),
        "foreign_threads": [{"name": name, "ident": ident} for name, ident in list(dict.fromkeys(foreign_calls))[:8]],
        "crash_count": guardian._crash_count,
        "stop_requested": guardian._stop.is_set(),
        "callback_present": guardian.status_callback is not None,
        "mock_is_loaded_module": sys.modules.get(mocked_module.__name__) is mocked_module,
        "mock_is_package_module": package is not None and vars(package).get("gateway_guardian") is mocked_module,
    }
    if guardian._lock.acquire(blocking=False):
        try:
            snapshot["status"] = {
                key: guardian._last_status[key]
                for key in ("ok", "guardian_running", "consecutive_failures", "restart_attempts", "crash_count")
                if type(guardian._last_status.get(key)) in (bool, int)
            }
            snapshot["status_lock"] = "available"
        finally:
            guardian._lock.release()
    else:
        snapshot["status_lock"] = "busy"
    frames = sys._current_frames()
    owned = guardian._thread
    threads = ([owned] if owned is not None else []) + [
        thread
        for thread in threading.enumerate()
        if thread is not owned and thread.name.startswith("dcc-mcp-gateway-guardian-")
    ]
    snapshot["threads"] = [
        {
            "name": thread.name,
            "ident": thread.ident,
            "owned": thread is owned,
            "alive": thread.is_alive(),
            "stack": _stack_rows(frames[thread.ident]) if thread.is_alive() and thread.ident in frames else [],
        }
        for thread in threads[:8]
    ]
    return json.dumps(snapshot, sort_keys=True)
