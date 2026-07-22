"""Probe dcc-mcp-cli, optionally install it verified, and inspect inventory."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
from typing import Any

import dcc_gateway

DEFAULT_BASE_URL = "http://127.0.0.1:9765"


def _run_json(argv: list[str]) -> tuple[bool, dict[str, Any]]:
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except OSError as exc:
        return False, {"error": str(exc)}
    except subprocess.TimeoutExpired:
        return False, {"error": "command timed out"}

    if proc.returncode != 0:
        return False, {"returncode": proc.returncode, "stderr": proc.stderr.strip()}

    try:
        payload = json.loads(proc.stdout or "{}")
    except json.JSONDecodeError as exc:
        return False, {"error": f"invalid JSON output: {exc}", "stdout": proc.stdout}
    return True, payload


def probe(
    cli: str = "dcc-mcp-cli",
    base_url: str | None = None,
    *,
    ensure_cli: bool = False,
    install_dir: str | None = None,
) -> dict[str, Any]:
    """Return CLI availability and inventory, with opt-in verified install."""
    resolved = shutil.which(cli)
    url = base_url or os.environ.get("DCC_MCP_BASE_URL") or DEFAULT_BASE_URL
    result: dict[str, Any] = {
        "cli": cli,
        "cli_path": resolved,
        "base_url": url,
        "cli_ok": resolved is not None,
        "gateway_ok": False,
        "total": 0,
        "by_dcc_type": {},
    }
    if resolved is None and ensure_cli:
        default_install_dir = Path.home() / ".local" / "bin"
        resolved_install_dir = install_dir or os.environ.get("DCC_MCP_INSTALL_DIR") or str(default_install_dir)
        ok, message, download_url = dcc_gateway.install_cli(
            install_dir=Path(resolved_install_dir),
        )
        result["install_attempted"] = True
        result["install_ok"] = ok
        result["install_message"] = message
        result["download_url"] = download_url
        if ok:
            resolved = message
            result["cli_path"] = resolved
            result["cli_ok"] = True
    if resolved is None:
        fallback = dcc_gateway.python_fallback(
            "list",
            argparse.Namespace(base_url=url),
        )
        result["fallback"] = "python-stdlib-rest"
        if isinstance(fallback, dict) and "instances" in fallback:
            instances = fallback.get("instances") or []
            counts: dict[str, int] = {}
            for item in instances:
                if isinstance(item, dict):
                    dcc_type = str(item.get("dcc_type") or "unknown")
                    counts[dcc_type] = counts.get(dcc_type, 0) + 1
            result["gateway_ok"] = True
            result["total"] = int(fallback.get("total") or len(instances))
            result["by_dcc_type"] = counts
            result["list"] = fallback
        return result

    health_ok, health = _run_json([resolved, "--base-url", url, "health"])
    result["health"] = health
    result["gateway_ok"] = health_ok
    if not health_ok:
        return result

    list_ok, listing = _run_json([resolved, "--base-url", url, "list"])
    result["list"] = listing
    if not list_ok:
        return result

    instances = listing.get("instances") if isinstance(listing, dict) else None
    if not isinstance(instances, list):
        instances = []
    counts: dict[str, int] = {}
    for item in instances:
        if isinstance(item, dict):
            dcc_type = str(item.get("dcc_type") or "unknown")
            counts[dcc_type] = counts.get(dcc_type, 0) + 1
    result["total"] = int(listing.get("total") or len(instances))
    result["by_dcc_type"] = counts
    return result


def main() -> int:
    """CLI entry point."""
    parser = argparse.ArgumentParser(
        description="Probe dcc-mcp-cli and gateway inventory with optional verified installation"
    )
    parser.add_argument("--cli", default="dcc-mcp-cli")
    parser.add_argument("--base-url", default=os.environ.get("DCC_MCP_BASE_URL") or DEFAULT_BASE_URL)
    parser.add_argument(
        "--ensure-cli",
        action="store_true",
        help="After explicit user consent, install from the verified official release manifest",
    )
    parser.add_argument("--install-dir", default=os.environ.get("DCC_MCP_INSTALL_DIR"))
    parser.add_argument("--pretty", action="store_true")
    args = parser.parse_args()

    payload = probe(
        cli=args.cli,
        base_url=args.base_url,
        ensure_cli=args.ensure_cli,
        install_dir=args.install_dir,
    )
    print(json.dumps(payload, indent=2 if args.pretty else None, sort_keys=args.pretty))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
