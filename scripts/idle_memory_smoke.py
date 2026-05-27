#!/usr/bin/env python3
"""Sample idle memory for the standalone dcc-mcp-server binary (#1354).

The smoke run starts the server without a live DCC, gateway, admin UI, bridge,
or rotating file logs, samples RSS/working-set memory for a short window, then
terminates the process. Thresholds are intentionally loose by default so the
script is useful as a regression tripwire without becoming CI-flaky.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Any


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _default_server_exe() -> Path | None:
    exe_name = "dcc-mcp-server.exe" if os.name == "nt" else "dcc-mcp-server"
    root = _repo_root()
    candidates = [
        root / "target" / "debug" / exe_name,
        root / "target" / "release" / exe_name,
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


def _sample_memory(pid: int) -> dict[str, int | None]:
    if os.name == "nt":
        script = (
            f"$p = Get-Process -Id {pid}; "
            "[pscustomobject]@{rss=$p.WorkingSet64; private=$p.PrivateMemorySize64} | ConvertTo-Json -Compress"
        )
        raw = subprocess.check_output(
            ["powershell", "-NoProfile", "-Command", script],
            text=True,
            stderr=subprocess.DEVNULL,
        )
        data = json.loads(raw)
        return {"rss_bytes": int(data["rss"]), "private_bytes": int(data["private"])}

    raw = subprocess.check_output(
        ["ps", "-o", "rss=", "-p", str(pid)],
        text=True,
        stderr=subprocess.DEVNULL,
    ).strip()
    return {"rss_bytes": int(raw) * 1024, "private_bytes": None}


def _mb(value: int | None) -> float | None:
    if value is None:
        return None
    return value / (1024 * 1024)


def _build_command(args: argparse.Namespace) -> list[str]:
    server_exe = Path(args.server_exe) if args.server_exe else _default_server_exe()
    if server_exe is None:
        raise SystemExit("dcc-mcp-server binary not found; pass --server-exe or run `vx cargo build -p dcc-mcp-server`")
    return [
        str(server_exe),
        "--app",
        args.app,
        "--mcp-port",
        "0",
        "--gateway-port",
        "0",
        "--no-bridge",
        "--no-admin",
        "--no-log-file",
    ]


def _terminate(proc: subprocess.Popen[str]) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


def run(args: argparse.Namespace) -> dict[str, Any]:
    """Run the smoke test and return a JSON-serialisable report."""
    cmd = _build_command(args)
    env = os.environ.copy()
    env.setdefault("DCC_MCP_NO_LOG_FILE", "true")
    env.setdefault("DCC_MCP_NO_ADMIN", "true")
    env.setdefault("RUST_LOG", "warn")
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )

    samples: list[dict[str, Any]] = []
    started = time.monotonic()
    try:
        while time.monotonic() - started < args.duration_secs:
            time.sleep(args.interval_secs)
            if proc.poll() is not None:
                stdout, stderr = proc.communicate(timeout=1)
                raise SystemExit(
                    f"dcc-mcp-server exited early with code {proc.returncode}\n"
                    f"stdout:\n{stdout[-2000:]}\n"
                    f"stderr:\n{stderr[-2000:]}"
                )
            sample = _sample_memory(proc.pid)
            sample["elapsed_secs"] = round(time.monotonic() - started, 3)
            samples.append(sample)
    finally:
        _terminate(proc)

    max_rss = max((s["rss_bytes"] for s in samples if s["rss_bytes"] is not None), default=None)
    max_private = max(
        (s["private_bytes"] for s in samples if s["private_bytes"] is not None),
        default=None,
    )
    return {
        "command": cmd,
        "duration_secs": args.duration_secs,
        "interval_secs": args.interval_secs,
        "sample_count": len(samples),
        "max_rss_mb": _mb(max_rss),
        "max_private_mb": _mb(max_private),
        "threshold_rss_mb": args.threshold_rss_mb,
        "threshold_private_mb": args.threshold_private_mb,
        "samples": samples,
    }


def main() -> int:
    """Parse CLI arguments and enforce memory thresholds."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server-exe", help="Path to a built dcc-mcp-server binary")
    parser.add_argument("--app", default="python", help="DCC app name to advertise")
    parser.add_argument("--duration-secs", type=float, default=10.0)
    parser.add_argument("--interval-secs", type=float, default=1.0)
    parser.add_argument("--threshold-rss-mb", type=float, default=256.0)
    parser.add_argument("--threshold-private-mb", type=float, default=256.0)
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON only")
    args = parser.parse_args()

    report = run(args)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            "idle memory: "
            f"max_rss={report['max_rss_mb']:.1f} MiB, "
            f"max_private={report['max_private_mb'] if report['max_private_mb'] is not None else 'n/a'} MiB"
        )
        print(json.dumps(report, indent=2, sort_keys=True))

    rss = report["max_rss_mb"]
    private = report["max_private_mb"]
    if rss is not None and rss > args.threshold_rss_mb:
        print(f"RSS threshold exceeded: {rss:.1f} > {args.threshold_rss_mb:.1f} MiB", file=sys.stderr)
        return 2
    if private is not None and private > args.threshold_private_mb:
        print(
            f"private-memory threshold exceeded: {private:.1f} > {args.threshold_private_mb:.1f} MiB",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
