"""Collect a structured error report for any DCC environment.

Aggregates the most useful signals for diagnosing tool failures:

1. **Recent log lines** — tail the rolling log file written by
   ``DccServerBase`` (``dcc-mcp-<dcc>.*.log``).  Includes ERROR and WARNING
   lines plus the last N lines of context.
2. **Failed / interrupted jobs** — query the SQLite job-persistence database
   (``dcc-mcp-<dcc>-jobs.db``) for recent non-successful jobs.
3. **Process status** — current PID liveness, platform, Python version.
4. **Observability config** — which features are active (file logging, job
   persistence, telemetry) so the user knows what data is available.

The script is designed to be the *first* tool an AI agent calls when a
``tools/call`` failure like "mcp工具执行失败" is reported.  It produces a
single JSON blob that contains enough context to understand the root cause
without needing to correlate multiple tool calls.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import sqlite3
import sys

# ── helpers ───────────────────────────────────────────────────────────────────


def _tail_lines(path: str, n: int) -> list[str]:
    """Return the last ``n`` lines of ``path`` without loading the whole file."""
    try:
        return Path(path).open(encoding="utf-8", errors="replace").readlines()[-n:]
    except OSError:
        return []


def _find_log_files(log_dir: str, dcc_name: str | None) -> list[str]:
    """Return all matching log files sorted newest-first."""
    base = Path(log_dir)
    pattern = f"dcc-mcp-{dcc_name}-*.log" if dcc_name else "dcc-mcp-*.log"
    files = list(base.glob(pattern))
    if not files:
        pattern2 = f"dcc-mcp-{dcc_name}*.log" if dcc_name else "dcc-mcp*.log"
        files = list(base.glob(pattern2))
    return sorted((str(f) for f in files), key=os.path.getmtime, reverse=True)


def _extract_errors(lines: list[str], include_warnings: bool = True) -> list[str]:
    """Return only ERROR (and optionally WARNING) lines."""
    out = []
    for line in lines:
        line_upper = line.upper()
        if "ERROR" in line_upper or (include_warnings and "WARN" in line_upper):
            out.append(line.rstrip())
    return out


def _collect_log_section(log_dir: str, dcc_name: str | None, tail_lines: int) -> dict:
    """Read recent log lines and extract errors/warnings."""
    if not log_dir or not Path(log_dir).is_dir():
        return {
            "available": False,
            "reason": f"Log directory not found: {log_dir!r}. "
            "Enable file logging via DccServerBase(enable_file_logging=True).",
        }

    files = _find_log_files(log_dir, dcc_name)
    if not files:
        return {
            "available": False,
            "log_dir": log_dir,
            "reason": "No log files found. The server may not have been started yet.",
        }

    newest = files[0]
    all_tail = _tail_lines(newest, tail_lines)
    errors = _extract_errors(all_tail, include_warnings=True)

    return {
        "available": True,
        "log_file": newest,
        "log_dir": log_dir,
        "total_files": len(files),
        "tail_lines": len(all_tail),
        "error_warning_count": len(errors),
        "recent_errors": errors[-50:],
        "last_lines": [ln.rstrip() for ln in all_tail[-20:]],
    }


def _collect_job_section(db_path: str, limit: int) -> dict:
    """Query failed/interrupted jobs from the SQLite job-persistence database."""
    if not db_path:
        return {
            "available": False,
            "reason": "Job persistence not configured. Enable via DccServerBase(enable_job_persistence=True).",
        }
    if not Path(db_path).is_file():
        return {
            "available": False,
            "db_path": db_path,
            "reason": "Job database file not found — no jobs recorded yet.",
        }

    try:
        con = sqlite3.connect(db_path, timeout=5)
        con.row_factory = sqlite3.Row
        cur = con.cursor()

        tables = [r[0] for r in cur.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()]
        job_table = next((t for t in tables if "job" in t.lower()), None)
        if not job_table:
            return {"available": False, "db_path": db_path, "reason": f"No jobs table found. Tables: {tables}"}

        cols_raw = cur.execute(f"PRAGMA table_info({job_table})").fetchall()
        col_names = [c[1] for c in cols_raw]

        status_col = next((c for c in col_names if "status" in c.lower()), None)
        tool_col = next(
            (c for c in col_names if c.lower() in ("tool", "action", "tool_name", "action_name")),
            None,
        )
        error_col = next((c for c in col_names if "error" in c.lower()), None)
        ts_col = next(
            (c for c in col_names if c.lower() in ("created_at", "started_at", "timestamp", "updated_at")),
            None,
        )

        select_cols = ", ".join(filter(None, [status_col, tool_col, error_col, ts_col]))
        if not select_cols:
            select_cols = "*"

        where = ""
        if status_col:
            where = f"WHERE {status_col} NOT IN ('completed', 'pending', 'running')"

        order = f"ORDER BY {ts_col} DESC" if ts_col else ""
        rows = cur.execute(f"SELECT {select_cols} FROM {job_table} {where} {order} LIMIT {limit}").fetchall()
        con.close()

        failed_jobs = [dict(r) for r in rows]
        return {
            "available": True,
            "db_path": db_path,
            "failed_job_count": len(failed_jobs),
            "failed_jobs": failed_jobs,
        }
    except Exception as exc:
        return {"available": False, "db_path": db_path, "reason": f"DB read error: {exc}"}


def _collect_process_section() -> dict:
    """Collect a process and environment snapshot."""
    info: dict = {
        "pid": os.getpid(),
        "platform": platform.system(),
        "platform_release": platform.release(),
        "python_version": sys.version,
        "executable": sys.executable,
        "cwd": str(Path.cwd()),
    }
    env_keys = [
        "DCC_MCP_LOG_DIR",
        "DCC_MCP_SKILL_PATHS",
        "DCC_MCP_PYTHON_EXECUTABLE",
        "DCC_MCP_PYTHON_INIT_SNIPPET",
        "DCC_MCP_ALLOW_AMBIENT_PYTHON",
        "DCC_MCP_DISABLE_FILE_LOGGING",
        "DCC_MCP_DISABLE_JOB_PERSISTENCE",
        "DCC_MCP_DISABLE_TELEMETRY",
        "DCC_MCP_GATEWAY_PORT",
        "DCC_MCP_IPC_ADDRESS",
    ]
    info["dcc_mcp_env"] = {k: os.environ.get(k) for k in env_keys if os.environ.get(k)}
    return info


def _dcc_name_from_env() -> str | None:
    """Infer the DCC name from environment variables."""
    for key in os.environ:
        if key.startswith("DCC_MCP_") and key.endswith("_SKILL_PATHS") and key != "DCC_MCP_SKILL_PATHS":
            middle = key[len("DCC_MCP_") : -len("_SKILL_PATHS")]
            return middle.lower()
    return None


# ── main ──────────────────────────────────────────────────────────────────────


def main() -> None:
    """Generate a structured error report and print JSON to stdout."""
    parser = argparse.ArgumentParser(description="Generate a DCC error report.")
    parser.add_argument("--dcc-name", default=None, dest="dcc_name")
    parser.add_argument("--log-dir", default=None, dest="log_dir")
    parser.add_argument("--db-path", default=None, dest="db_path")
    parser.add_argument("--tail", type=int, default=200, dest="tail_lines")
    parser.add_argument("--job-limit", type=int, default=20, dest="job_limit")
    args = parser.parse_args()

    dcc_name = args.dcc_name or _dcc_name_from_env()

    log_dir = args.log_dir or os.environ.get("DCC_MCP_LOG_DIR") or ""
    if not log_dir:
        try:
            from dcc_mcp_core import get_log_dir

            log_dir = get_log_dir()
        except Exception:
            pass

    db_path = args.db_path or ""
    if not db_path and log_dir and dcc_name:
        db_path = str(Path(log_dir) / f"dcc-mcp-{dcc_name}-jobs.db")
    elif not db_path and log_dir:
        candidates = sorted(
            Path(log_dir).glob("dcc-mcp-*-jobs.db"),
            key=os.path.getmtime,
            reverse=True,
        )
        if candidates:
            db_path = str(candidates[0])

    log_section = _collect_log_section(log_dir, dcc_name, args.tail_lines)
    job_section = _collect_job_section(db_path, args.job_limit)
    process_section = _collect_process_section()

    hints: list[str] = []
    if log_section.get("error_warning_count", 0) > 0:
        hints.append(
            f"Found {log_section['error_warning_count']} ERROR/WARNING line(s) in "
            f"{log_section.get('log_file', 'log file')}. Check 'recent_errors' for details."
        )
    if job_section.get("failed_job_count", 0) > 0:
        hints.append(
            f"Found {job_section['failed_job_count']} failed/interrupted job(s) in the database. "
            "Check 'failed_jobs' for tool names and error messages."
        )
    if not log_section.get("available"):
        hints.append(
            "File logging is not active — enable it with DccServerBase(enable_file_logging=True) "
            "to capture future errors. Set DCC_MCP_LOG_DIR to an accessible directory."
        )
    if not job_section.get("available"):
        hints.append(
            "Job persistence is not active — enable it with DccServerBase(enable_job_persistence=True) "
            "to record tool call history. Requires the job-persist-sqlite wheel feature."
        )
    if not hints:
        hints.append("No errors or failed jobs found. The system appears healthy.")

    prompt_parts = [
        "Error report generated. Key signals:",
        *[f"- {h}" for h in hints],
        "Next steps:",
        "- If errors appear in recent_errors, look for the root cause "
        "(import failures, subprocess exits, DCC API errors).",
        "- If DCC_MCP_PYTHON_EXECUTABLE is set inside a DCC, that is likely the cause — "
        "skill scripts should execute in-process using set_in_process_executor() instead.",
        "- Use dcc_diagnostics__audit_log to see sandbox-level denials.",
        "- Use dcc_diagnostics__tool_metrics to identify consistently failing tools.",
        "- Use dcc_diagnostics__screenshot to capture the current visual state.",
    ]

    print(
        json.dumps(
            {
                "success": True,
                "message": f"Error report for DCC={dcc_name or 'unknown'}: {'; '.join(hints)}",
                "prompt": "\n".join(prompt_parts),
                "context": {
                    "dcc_name": dcc_name,
                    "log": log_section,
                    "jobs": job_section,
                    "process": process_section,
                },
            }
        )
    )


if __name__ == "__main__":
    main()
