"""Cross-platform gateway helper with CLI download and Python REST fallback."""

from __future__ import annotations

import argparse
import contextlib
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import tempfile
from typing import Any
import urllib.error
import urllib.request

DEFAULT_BASE_URL = "http://127.0.0.1:9765"
DEFAULT_REPO = "dcc-mcp/dcc-mcp-core"
DEFAULT_VERSION = "latest"


def _json_dumps(payload: Any, *, pretty: bool = False) -> str:
    return json.dumps(payload, indent=2 if pretty else None, sort_keys=pretty)


def _run_json(argv: list[str]) -> tuple[bool, dict[str, Any]]:
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=60,
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


def _request_json(base_url: str, method: str, path: str, body: dict[str, Any] | None = None) -> dict[str, Any]:
    url = f"{base_url.rstrip('/')}{path}"
    data = None if body is None else json.dumps(body).encode("utf-8")
    request = urllib.request.Request(url, data=data, method=method)
    request.add_header("Accept", "application/json")
    if body is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            text = response.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        return {"success": False, "error": "http-error", "status": exc.code, "detail": detail}
    except (urllib.error.URLError, OSError) as exc:
        return {"success": False, "error": "connection-error", "detail": str(exc)}
    if not text:
        return {}
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        return {"success": False, "error": "invalid-json", "detail": str(exc), "body": text}


def _asset_name() -> str | None:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "windows" and machine in {"amd64", "x86_64"}:
        return "dcc-mcp-cli-windows-x86_64.exe"
    if system == "linux" and machine in {"amd64", "x86_64"}:
        return "dcc-mcp-cli-linux-x86_64"
    if system == "darwin":
        return "dcc-mcp-cli-macos-universal2"
    return None


def _download_url(repo: str, version: str, asset: str) -> str:
    if version == "latest":
        return f"https://github.com/{repo}/releases/latest/download/{asset}"
    return f"https://github.com/{repo}/releases/download/{version}/{asset}"


def install_cli(
    *,
    install_dir: Path,
    repo: str = DEFAULT_REPO,
    version: str = DEFAULT_VERSION,
) -> tuple[bool, str, str | None]:
    """Download dcc-mcp-cli for the current platform."""
    asset = _asset_name()
    if asset is None:
        return False, "unsupported platform for release asset", None

    install_dir.mkdir(parents=True, exist_ok=True)
    executable_name = "dcc-mcp-cli.exe" if platform.system().lower() == "windows" else "dcc-mcp-cli"
    target = install_dir / executable_name
    url = _download_url(repo, version, asset)

    fd, tmp_name = tempfile.mkstemp(prefix="dcc-mcp-cli-", suffix=target.suffix)
    os.close(fd)
    tmp_path = Path(tmp_name)
    try:
        urllib.request.urlretrieve(url, tmp_path)
        if platform.system().lower() != "windows":
            tmp_path.chmod(tmp_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        shutil.move(str(tmp_path), str(target))
    except (urllib.error.URLError, OSError) as exc:
        with contextlib.suppress(OSError):
            tmp_path.unlink(missing_ok=True)
        return False, f"download failed: {exc}", url
    return True, str(target), url


def resolve_cli(args: argparse.Namespace) -> tuple[str | None, dict[str, Any]]:
    """Find the CLI; optionally install it from GitHub releases."""
    found = shutil.which(args.cli)
    details: dict[str, Any] = {"cli": args.cli, "cli_path": found, "installed": False}
    if found:
        return found, details

    if not args.ensure_cli:
        return None, details

    install_dir = Path(args.install_dir).expanduser()
    ok, message, url = install_cli(install_dir=install_dir, repo=args.repo, version=args.version)
    details.update({"install_attempted": True, "install_ok": ok, "install_message": message, "download_url": url})
    if not ok:
        return None, details
    details["installed"] = True
    return message, details


def cli_args_for(command: str, args: argparse.Namespace) -> list[str]:
    """Build dcc-mcp-cli argv for a command."""
    argv = [args.cli_path, "--base-url", args.base_url, command]
    if command == "search":
        if args.query:
            argv.extend(["--query", args.query])
        if args.dcc_type:
            argv.extend(["--dcc-type", args.dcc_type])
        if args.limit is not None:
            argv.extend(["--limit", str(args.limit)])
    elif command == "describe":
        argv.append(args.tool_slug)
    elif command == "call":
        argv.extend([args.tool_slug, "--json", args.json])
        if args.meta_json:
            argv.extend(["--meta-json", args.meta_json])
    return argv


def python_fallback(command: str, args: argparse.Namespace) -> dict[str, Any]:
    """Execute the gateway workflow via Python stdlib REST calls."""
    if command == "health":
        return _request_json(args.base_url, "GET", "/v1/healthz")
    if command == "list":
        return _request_json(args.base_url, "GET", "/v1/instances")
    if command == "search":
        body: dict[str, Any] = {}
        if args.query:
            body["query"] = args.query
        if args.dcc_type:
            body["dcc_type"] = args.dcc_type
        if args.limit is not None:
            body["limit"] = args.limit
        return _request_json(args.base_url, "POST", "/v1/search", body)
    if command == "describe":
        return _request_json(
            args.base_url,
            "POST",
            "/v1/describe",
            {"tool_slug": args.tool_slug, "include_schema": True},
        )
    if command == "call":
        try:
            arguments = json.loads(args.json)
        except json.JSONDecodeError as exc:
            return {"success": False, "error": "--json must be valid JSON", "detail": str(exc)}
        body = {"tool_slug": args.tool_slug, "arguments": arguments}
        if args.meta_json:
            try:
                body["meta"] = json.loads(args.meta_json)
            except json.JSONDecodeError as exc:
                return {"success": False, "error": "--meta-json must be valid JSON", "detail": str(exc)}
        return _request_json(args.base_url, "POST", "/v1/call", body)
    raise ValueError(f"unsupported command: {command}")


def run_command(command: str, args: argparse.Namespace) -> dict[str, Any]:
    """Prefer dcc-mcp-cli, optionally install it, then fall back to Python REST."""
    cli_path, cli_details = resolve_cli(args)
    if cli_path:
        args.cli_path = cli_path
        ok, payload = _run_json(cli_args_for(command, args))
        if ok:
            return payload
        cli_details["cli_error"] = payload

    fallback_payload = python_fallback(command, args)
    if isinstance(fallback_payload, dict):
        fallback_payload.setdefault("_transport", "python-stdlib-rest")
        fallback_payload.setdefault("_cli", cli_details)
    return fallback_payload


def build_parser() -> argparse.ArgumentParser:
    """Create the helper CLI parser."""
    parser = argparse.ArgumentParser(description="DCC-MCP gateway helper with CLI-first execution.")
    parser.add_argument("--base-url", default=os.environ.get("DCC_MCP_BASE_URL") or DEFAULT_BASE_URL)
    parser.add_argument("--cli", default="dcc-mcp-cli")
    parser.add_argument("--ensure-cli", action="store_true", help="Download dcc-mcp-cli from GitHub if it is missing")
    parser.add_argument(
        "--install-dir",
        default=os.environ.get("DCC_MCP_INSTALL_DIR") or str(Path.home() / ".local" / "bin"),
    )
    parser.add_argument("--repo", default=os.environ.get("DCC_MCP_REPO") or DEFAULT_REPO)
    parser.add_argument("--version", default=os.environ.get("DCC_MCP_VERSION") or DEFAULT_VERSION)
    parser.add_argument("--pretty", action="store_true")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("health")
    sub.add_parser("list")

    search = sub.add_parser("search")
    search.add_argument("--query")
    search.add_argument("--dcc-type")
    search.add_argument("--limit", type=int)

    describe = sub.add_parser("describe")
    describe.add_argument("tool_slug")

    call = sub.add_parser("call")
    call.add_argument("tool_slug")
    call.add_argument("--json", default="{}")
    call.add_argument("--meta-json")
    return parser


def main() -> int:
    """CLI entry point."""
    args = build_parser().parse_args()
    payload = run_command(args.command, args)
    print(_json_dumps(payload, pretty=args.pretty))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
