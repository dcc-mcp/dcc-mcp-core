"""Cross-platform gateway helper with verified CLI install and REST fallback."""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
from typing import Any
import urllib.error
import urllib.request

DEFAULT_BASE_URL = "http://127.0.0.1:9765"
OFFICIAL_REPO = "dcc-mcp/dcc-mcp-core"
DEFAULT_VERSION = "latest"
MAX_MANIFEST_BYTES = 64 * 1024
MAX_CLI_BYTES = 256 * 1024 * 1024
_VERSION_RE = re.compile(r"^(?:v)?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z][0-9A-Za-z.-]*)?)$")
_SHA256_RE = re.compile(r"^[0-9a-fA-F]{64}$")


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


def _release_assets() -> tuple[str, str] | None:
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "windows" and machine in {"amd64", "x86_64"}:
        return (
            "dcc-mcp-cli-windows-x86_64.exe",
            "dcc-mcp-update-manifest-windows-x86_64.json",
        )
    if system == "linux" and machine in {"amd64", "x86_64"}:
        return (
            "dcc-mcp-cli-linux-x86_64",
            "dcc-mcp-update-manifest-linux-x86_64.json",
        )
    if system == "darwin":
        return (
            "dcc-mcp-cli-macos-universal2",
            "dcc-mcp-update-manifest-macos-universal2.json",
        )
    return None


def _normalized_version(version: str) -> str | None:
    if version == "latest":
        return None
    match = _VERSION_RE.fullmatch(version)
    if match is None:
        raise ValueError("version must be 'latest' or a stable release such as v0.19.63")
    return match.group(1)


def _manifest_url(version: str, manifest_asset: str) -> str:
    normalized = _normalized_version(version)
    if normalized is None:
        return f"https://github.com/{OFFICIAL_REPO}/releases/latest/download/{manifest_asset}"
    return f"https://github.com/{OFFICIAL_REPO}/releases/download/v{normalized}/{manifest_asset}"


def _read_limited(response: Any, limit: int) -> bytes:
    payload = response.read(limit + 1)
    if len(payload) > limit:
        raise ValueError(f"response exceeds the {limit}-byte safety limit")
    return payload


def _verified_manifest_entry(
    payload: bytes,
    *,
    requested_version: str,
    asset: str,
) -> tuple[str, str]:
    try:
        manifest = json.loads(payload.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"invalid official release manifest: {exc}") from exc
    if not isinstance(manifest, dict):
        raise ValueError("invalid official release manifest: expected an object")
    entry = manifest.get("dcc-mcp-cli")
    if not isinstance(entry, dict):
        raise ValueError("invalid official release manifest: dcc-mcp-cli entry is missing")

    entry_version = entry.get("version")
    url = entry.get("url")
    digest = entry.get("sha256")
    if not isinstance(entry_version, str) or _VERSION_RE.fullmatch(entry_version) is None:
        raise ValueError("invalid official release manifest: CLI version is invalid")
    normalized_entry_version = _normalized_version(entry_version)
    normalized_requested_version = _normalized_version(requested_version)
    if normalized_requested_version is not None and normalized_entry_version != normalized_requested_version:
        raise ValueError("official release manifest version does not match the requested version")
    expected_url = f"https://github.com/{OFFICIAL_REPO}/releases/download/v{normalized_entry_version}/{asset}"
    if url != expected_url:
        raise ValueError("official release manifest points outside the expected official release asset")
    if not isinstance(digest, str) or _SHA256_RE.fullmatch(digest) is None:
        raise ValueError("invalid official release manifest: CLI SHA-256 is invalid")
    return url, digest.lower()


def install_cli(
    *,
    install_dir: Path,
    version: str = DEFAULT_VERSION,
) -> tuple[bool, str, str | None]:
    """Install dcc-mcp-cli only after its official manifest verifies SHA-256."""
    assets = _release_assets()
    if assets is None:
        return False, "unsupported platform for release asset", None
    asset, manifest_asset = assets

    try:
        manifest_url = _manifest_url(version, manifest_asset)
        with urllib.request.urlopen(manifest_url, timeout=60) as response:
            manifest_payload = _read_limited(response, MAX_MANIFEST_BYTES)
        url, expected_digest = _verified_manifest_entry(
            manifest_payload,
            requested_version=version,
            asset=asset,
        )
    except (urllib.error.URLError, OSError, ValueError) as exc:
        return False, f"verified install failed: {exc}", None

    executable_name = "dcc-mcp-cli.exe" if platform.system().lower() == "windows" else "dcc-mcp-cli"
    target = install_dir / executable_name

    try:
        install_dir.mkdir(parents=True, exist_ok=True)
        fd, tmp_name = tempfile.mkstemp(
            prefix=".dcc-mcp-cli-",
            suffix=target.suffix,
            dir=str(install_dir),
        )
        os.close(fd)
    except OSError as exc:
        return False, f"verified install failed: cannot prepare install directory: {exc}", url
    tmp_path = Path(tmp_name)
    try:
        digest = hashlib.sha256()
        total = 0
        with urllib.request.urlopen(url, timeout=60) as response, tmp_path.open("wb") as output:
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_CLI_BYTES:
                    raise ValueError(f"CLI asset exceeds the {MAX_CLI_BYTES}-byte safety limit")
                digest.update(chunk)
                output.write(chunk)
        if total == 0:
            raise ValueError("CLI asset is empty")
        actual_digest = digest.hexdigest()
        if actual_digest != expected_digest:
            raise ValueError(f"CLI SHA-256 mismatch: expected {expected_digest}, got {actual_digest}")
        if platform.system().lower() != "windows":
            tmp_path.chmod(0o755)
        tmp_path.replace(target)
    except (urllib.error.URLError, OSError, ValueError) as exc:
        with contextlib.suppress(OSError):
            tmp_path.unlink()
        return False, f"verified install failed: {exc}", url
    return True, str(target), url


def resolve_cli(args: argparse.Namespace) -> tuple[str | None, dict[str, Any]]:
    """Find the CLI; optionally install a verified official release."""
    found = shutil.which(args.cli)
    details: dict[str, Any] = {"cli": args.cli, "cli_path": found, "installed": False}
    if found:
        return found, details

    if not args.ensure_cli:
        return None, details

    install_dir = Path(args.install_dir).expanduser()
    ok, message, url = install_cli(install_dir=install_dir, version=args.version)
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
    parser.add_argument(
        "--ensure-cli",
        action="store_true",
        help="After explicit user consent, install dcc-mcp-cli from its verified official release manifest",
    )
    parser.add_argument(
        "--install-dir",
        default=os.environ.get("DCC_MCP_INSTALL_DIR") or str(Path.home() / ".local" / "bin"),
    )
    parser.add_argument("--version", default=DEFAULT_VERSION)
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
