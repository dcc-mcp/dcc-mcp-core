"""Command-line entry point for the ``dcc-mcp-core`` console script.

``dcc_mcp_core`` resolves as a *library*: Rez, pip and venv all put it on the
import path, but the package historically shipped no executable, so a resolved
environment could not answer "how do I start this MCP server?". This module
closes that gap:

* ``dcc-mcp-core --version`` reports which build is resolved.
* ``dcc-mcp-core info`` prints the documented launch contract.
* ``dcc-mcp-core serve`` starts a Skills-First MCP server for one DCC.

Keep this module import-light: ``--version`` and ``info`` must keep working in
the Python 3.7 pure-Python wheel, which has no compiled ``_core`` extension.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from typing import Any
from typing import Optional
from typing import Sequence

from dcc_mcp_core import __version__ as _VERSION

_SHUTDOWN_POLL_SECS = 0.5


def _launch_contract(version: str) -> str:
    """Return the human-readable launch contract for this package."""
    python = "{}.{}.{}".format(*sys.version_info[:3])
    return "\n".join(
        [
            f"dcc-mcp-core {version} (python {python})",
            "",
            "Executable entry points",
            "  dcc-mcp-core --version",
            "  dcc-mcp-core info",
            "  dcc-mcp-core serve --dcc <name> [--host HOST] [--port PORT]",
            "",
            "Python API",
            "  from dcc_mcp_core import McpHttpConfig, create_skill_server",
            "",
            '  server = create_skill_server("maya", McpHttpConfig(port=8765))',
            "  handle = server.start()",
            "  print(handle.mcp_url())",
            "",
            "Rez package.py",
            '  name = "dcc_mcp_core"',
            f'  version = "{version}"',
            "",
            '  tools = ["dcc-mcp-core"]',
            "",
            "  def commands():",
            '      env.PYTHONPATH.prepend("{root}/site-packages")',
        ]
    )


def _build_parser() -> argparse.ArgumentParser:
    """Build the ``dcc-mcp-core`` argument parser."""
    parser = argparse.ArgumentParser(
        prog="dcc-mcp-core",
        description="Package-level entry point for dcc-mcp-core.",
    )
    parser.add_argument("--version", action="version", version=f"dcc-mcp-core {_VERSION}")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("info", help="Print the documented launch contract for this package.")

    serve = sub.add_parser("serve", help="Start a Skills-First MCP server for one DCC.")
    serve.add_argument("--dcc", required=True, help="DCC name, for example maya or blender.")
    serve.add_argument("--host", default="127.0.0.1", help="Bind address (default: 127.0.0.1).")
    serve.add_argument("--port", type=int, default=0, help="Bind port (default: 0, OS-assigned).")
    serve.add_argument("--skill-path", action="append", metavar="PATH", help="Extra skill directory; repeatable.")
    serve.add_argument(
        "--no-gateway",
        action="store_true",
        default=False,
        help="Skip gateway election for this server (gateway_port=0).",
    )
    serve.add_argument(
        "--max-run-secs",
        type=float,
        default=None,
        help="Stop automatically after this many seconds (default: run until interrupted).",
    )
    serve.add_argument("--json", action="store_true", default=False, help="Emit the endpoint as one JSON object.")
    return parser


def _build_config(args: argparse.Namespace) -> Any:
    """Build an ``McpHttpConfig`` for the ``serve`` command.

    ``host`` and ``gateway_port`` are assigned after construction because the
    PyO3 ``McpHttpConfig.__new__`` signature does not expose them as keyword
    arguments, while the pure-Python fallback accepts both.
    """
    from dcc_mcp_core.runtime import McpHttpConfig

    config = McpHttpConfig(port=args.port)
    config.host = args.host
    if args.no_gateway:
        config.gateway_port = 0
    return config


def _emit_ready(handle: Any, dcc_name: str, *, as_json: bool) -> None:
    """Print the resolved MCP endpoint so callers can script against it."""
    payload = {
        "dcc": dcc_name,
        "mcp_url": handle.mcp_url(),
        "port": int(getattr(handle, "port", 0) or 0),
        "gateway": bool(getattr(handle, "is_gateway", False)),
        "instance_id": getattr(handle, "instance_id", None),
    }
    if as_json:
        print(json.dumps(payload, sort_keys=True))
    else:
        print("dcc-mcp-core serving {} at {}".format(dcc_name, payload["mcp_url"]))
        print(
            "  port={} gateway={} instance_id={}".format(
                payload["port"],
                payload["gateway"],
                payload["instance_id"],
            )
        )
    sys.stdout.flush()


def _block_until_deadline(max_run_secs: Optional[float]) -> None:
    """Sleep until the run deadline expires or the caller interrupts."""
    deadline = None if max_run_secs is None else time.monotonic() + max_run_secs
    while deadline is None or time.monotonic() < deadline:
        time.sleep(_SHUTDOWN_POLL_SECS)


def _shutdown(handle: Any) -> None:
    """Stop the server, tolerating handles whose runtime already went away."""
    shutdown = getattr(handle, "shutdown", None)
    if callable(shutdown):
        shutdown()


def _cmd_info() -> int:
    """Print the documented launch contract."""
    print(_launch_contract(_VERSION))
    return 0


def _cmd_serve(args: argparse.Namespace) -> int:
    """Start a Skills-First MCP server and block until it is stopped."""
    from dcc_mcp_core.server_base import create_skill_server

    try:
        server = create_skill_server(args.dcc, _build_config(args), extra_paths=list(args.skill_path or []))
        handle = server.start()
    except Exception as exc:
        print(f"dcc-mcp-core serve failed: {exc}", file=sys.stderr)
        return 1

    _emit_ready(handle, args.dcc, as_json=args.json)
    try:
        _block_until_deadline(args.max_run_secs)
    except KeyboardInterrupt:
        pass
    finally:
        _shutdown(handle)
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Run the ``dcc-mcp-core`` command."""
    args = _build_parser().parse_args(argv)
    if args.command == "info":
        return _cmd_info()
    return _cmd_serve(args)


if __name__ == "__main__":
    sys.exit(main())
