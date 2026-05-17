r"""Probe DCC-MCP gateway health and instances; print one-line JSON to stdout.

Cross-platform: run with any Python 3.7+ on Windows, macOS, or Linux:

    python scripts/check_gateway.py
    py -3 scripts\check_gateway.py

Optional: bash scripts/check_gateway.sh  |  pwsh scripts/check_gateway.ps1
"""

from __future__ import annotations

import argparse
import json
import os
import urllib.error
import urllib.request


def probe(gateway: str | None = None) -> dict[str, object]:
    """Return gateway liveness, readiness, instance total, and per-dcc_type counts."""
    base = (gateway or os.environ.get("DCC_MCP_GATEWAY_URL") or "http://127.0.0.1:9765").rstrip("/")
    out: dict[str, object] = {
        "gateway_url": base,
        "gateway_ok": False,
        "ready": False,
        "total": 0,
        "by_dcc_type": {},
    }

    def ok(path: str) -> bool:
        try:
            with urllib.request.urlopen(f"{base}{path}", timeout=5) as resp:
                return 200 <= resp.status < 300
        except (urllib.error.URLError, OSError):
            return False

    out["gateway_ok"] = ok("/v1/healthz")
    out["ready"] = ok("/v1/readyz")

    try:
        with urllib.request.urlopen(f"{base}/v1/instances", timeout=10) as resp:
            data = json.loads(resp.read().decode())
        instances = data.get("instances") or []
        out["total"] = int(data.get("total", len(instances)))
        counts: dict[str, int] = {}
        for row in instances:
            dcc = str(row.get("dcc_type") or "unknown")
            counts[dcc] = counts.get(dcc, 0) + 1
        out["by_dcc_type"] = counts
    except (urllib.error.URLError, OSError, json.JSONDecodeError, ValueError, TypeError):
        pass

    return out


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Build CLI argument parser for gateway probe."""
    parser = argparse.ArgumentParser(description="Probe DCC-MCP gateway and print JSON summary.")
    parser.add_argument(
        "--gateway",
        "-g",
        default=None,
        help="Gateway base URL (default: DCC_MCP_GATEWAY_URL or http://127.0.0.1:9765)",
    )
    parser.add_argument(
        "--pretty",
        action="store_true",
        help="Pretty-print JSON instead of a single line",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Print probe result as JSON on stdout; return 0 on success."""
    args = parse_args(argv)
    payload = probe(args.gateway)
    if args.pretty:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(json.dumps(payload, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
