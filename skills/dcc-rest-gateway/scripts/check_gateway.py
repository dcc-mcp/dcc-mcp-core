"""Probe gateway health and instances; print one-line JSON to stdout."""

from __future__ import annotations

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


def main() -> None:
    """Print probe result as a single JSON line on stdout."""
    print(json.dumps(probe(), separators=(",", ":")))


if __name__ == "__main__":
    main()
