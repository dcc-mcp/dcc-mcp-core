"""Connect an existing official local MCP host or project-bound Framer SDK.

The optional MCP client uses the official SDK's maintained 1.x API. No
installer, OAuth provider, account creation, remote endpoint, or retry is used.
Run without ``--call`` to inspect the live catalogue only.
"""

from __future__ import annotations

import argparse
import asyncio
from contextlib import asynccontextmanager
import json
from pathlib import Path
import sys
from urllib.parse import urlsplit

from dcc_mcp_core.design_bridge import DesignBridgeError
from dcc_mcp_core.design_bridge import DesignSessionBridge
from dcc_mcp_core.design_bridge import FramerApiFacade


def validate_local_url(provider, url):
    """Keep this executable trial on the operator's local official host."""
    parsed = urlsplit(url)
    if (
        parsed.scheme != "http"
        or parsed.hostname not in {"localhost", "127.0.0.1", "::1"}
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("Use a credential-free loopback HTTP endpoint from the official host.")
    if provider == "figma" and (parsed.port != 3845 or parsed.path != "/mcp"):
        raise ValueError("The direct Figma trial only supports its documented desktop endpoint.")
    if provider == "sketch" and (parsed.port != 31126 or parsed.path != "/mcp"):
        raise ValueError("Use the official Sketch local MCP endpoint.")
    if provider not in {"figma", "penpot", "sketch"}:
        raise ValueError("This provider does not document a loopback HTTP MCP endpoint.")
    return url


@asynccontextmanager
async def upstream_session(args):
    """Own one SDK session for the complete trial, closing it on every exit."""
    if args.provider == "framer":
        from framer_adapter import FramerNodeSession

        if not args.project or args.url or args.command:
            raise ValueError("Framer requires --project and an existing SDK key supplied by the operator.")
        async with FramerNodeSession(project=args.project) as session:
            yield FramerApiFacade(list_tools=session.list_tools, call_tool=session.call_tool)
        return
    if args.provider == "sketch" and sys.platform != "darwin":
        raise DesignBridgeError("unsupported_host")
    if sys.version_info < (3, 10):
        raise ValueError("The optional official MCP SDK runner requires Python 3.10+.")

    from mcp import ClientSession
    from mcp import StdioServerParameters
    from mcp.client.stdio import stdio_client
    from mcp.client.streamable_http import streamable_http_client

    if bool(args.url) == bool(args.command):
        raise ValueError("Choose --url for a local host or --command for its existing official stdio entry.")
    if args.command:
        if args.provider != "pencil":
            raise ValueError("Only Pencil uses the existing official stdio entry in this trial.")
        # Use the supplied installed executable; never guess an install command.
        # Nested contexts keep this optional example parseable on Python 3.7.
        async with stdio_client(StdioServerParameters(command=args.command, args=args.arg)) as (read, write):  # noqa: SIM117
            async with ClientSession(read, write) as session:
                await session.initialize()
                yield session
    else:
        url = validate_local_url(args.provider, args.url)
        async with streamable_http_client(url) as (read, write, _session_id):  # noqa: SIM117
            async with ClientSession(read, write) as session:
                await session.initialize()
                yield session


async def run(args):
    """Discover first, optionally call one explicitly selected live tool."""
    bridge = DesignSessionBridge(args.provider)
    async with upstream_session(args) as session:
        await bridge.connect(session)
        try:
            if args.serve:
                from facade import serve_bridge

                await serve_bridge(bridge, port=args.port, registry_dir=args.registry_dir)
                return
            if args.call:
                arguments = {}
                if args.arguments_file:
                    arguments = json.loads(Path(args.arguments_file).read_text(encoding="utf-8"))
                result = await bridge.call_tool(
                    args.call,
                    arguments,
                    expected_generation=bridge.status()["generation"],
                )
                output = result.to_dict()
            else:
                output = bridge.capabilities()
            # Upstream payloads are project-private: stdout is never PR evidence.
            print(json.dumps(output, ensure_ascii=False, indent=2))
        finally:
            bridge.disconnect()


def parser():
    """Build a CLI that performs discovery unless a tool is explicitly named."""
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--provider", required=True, choices=("figma", "penpot", "pencil", "sketch", "framer"))
    cli.add_argument("--url", help="Official local HTTP MCP endpoint; no remote or credential-bearing URLs.")
    cli.add_argument("--command", help="Existing official Pencil stdio executable from its installed client entry.")
    cli.add_argument("--arg", action="append", default=[], help="One argument for that existing executable.")
    cli.add_argument("--project", help="Existing authorized Framer project ID or URL.")
    cli.add_argument("--call", help="Exact live tool name to invoke once; never replayed automatically.")
    cli.add_argument("--arguments-file", help="JSON object of arguments matching the observed upstream schema.")
    cli.add_argument("--serve", action="store_true", help="Expose the bound session through a local Core facade.")
    cli.add_argument("--port", type=int, default=0, help="Facade port; default requests an OS-assigned port.")
    cli.add_argument(
        "--registry-dir", help="Operator-approved registry for facade discovery; default is a temporary trial registry."
    )
    return cli


def main():
    """Report a bounded failure without printing upstream exception details."""
    args = parser().parse_args()
    if args.serve and args.call:
        raise SystemExit("Choose either --serve or --call.")
    if args.arguments_file and not args.call:
        raise SystemExit("--arguments-file requires --call.")
    try:
        asyncio.run(run(args))
    except DesignBridgeError as exc:
        print(json.dumps(exc.to_dict()), file=sys.stderr)
        raise SystemExit(1) from None
    except (ValueError, ImportError):
        print(json.dumps({"success": False, "error": "configuration_or_dependency_unavailable"}), file=sys.stderr)
        raise SystemExit(1) from None
    except Exception as exc:
        # Only numeric error status is consulted; never inspect exception text.
        code = {401: "auth_required", 403: "permission_denied"}.get(
            getattr(exc, "status_code", None), "upstream_unavailable"
        )
        print(json.dumps({"success": False, "error": code, "outcome": "unverified"}), file=sys.stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
