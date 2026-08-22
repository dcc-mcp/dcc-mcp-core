"""Command-line interface for import-light adapter lifecycle operations."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any
from typing import Dict
from typing import Iterable
from typing import Optional

from dcc_mcp_core.install_lifecycle import ROLE_PER_DCC_SIDECAR
from dcc_mcp_core.install_lifecycle import build_sidecar_command
from dcc_mcp_core.install_lifecycle import inspect_install_root
from dcc_mcp_core.install_lifecycle import launch_sidecar
from dcc_mcp_core.install_lifecycle import plan_runtime_updates
from dcc_mcp_core.install_lifecycle import query_runtime_state
from dcc_mcp_core.install_lifecycle import resolve_deployment_layout
from dcc_mcp_core.install_lifecycle import safe_remove_tree
from dcc_mcp_core.install_lifecycle import safe_replace_tree
from dcc_mcp_core.install_lifecycle import sidecar_readiness_status
from dcc_mcp_core.install_lifecycle import stop_runtime_entries
from dcc_mcp_core.install_lifecycle import wait_for_sidecar_ready

DEFAULT_SIDECAR_LIVENESS_CHECK_SECS = 1.0


def _failed(reason: str, message: str, path: Optional[Path]) -> Dict[str, Any]:
    return {
        "success": False,
        "status": "failed",
        "requires_restart": False,
        "path": str(path) if path else None,
        "reason": reason,
        "message": message,
    }


def _print_json(value: Dict[str, Any]) -> int:
    print(json.dumps(value, indent=2, sort_keys=True))
    return 0 if value.get("success") else 1


def _parse_target_versions(values: Optional[Iterable[str]]) -> Dict[str, str]:
    result = {}
    for value in values or []:
        if "=" not in value:
            raise ValueError(f"Expected KEY=VERSION target, got: {value}")
        key, version = value.split("=", 1)
        result[key.strip()] = version.strip()
    return result


def main(argv: Optional[Iterable[str]] = None) -> int:
    """Run the ``dcc-mcp-install-lifecycle`` command."""
    parser = argparse.ArgumentParser(description="Import-light DCC-MCP install lifecycle helpers.")
    sub = parser.add_subparsers(dest="command", required=True)

    query = sub.add_parser("query", help="Read registered runtimes from services.json.")
    query.add_argument("--registry-dir")
    query.add_argument("--dcc-type")
    query.add_argument("--role")
    query.add_argument("--install-root")
    query.add_argument("--include-dead", action="store_true", default=False)

    stop = sub.add_parser("stop", help="Stop registered sidecars without importing _core.")
    stop.add_argument("--registry-dir")
    stop.add_argument("--dcc-type")
    stop.add_argument("--role", default=ROLE_PER_DCC_SIDECAR)
    stop.add_argument("--install-root")
    stop.add_argument("--timeout-secs", type=float, default=5.0)
    stop.add_argument("--include-host-processes", action="store_true")

    layout = sub.add_parser("layout", help="Resolve Rez or filesystem deployment roots.")
    layout.add_argument("--cache-root")
    layout.add_argument("--package", action="append", dest="packages")
    layout.add_argument("--adapter-package")

    sidecar_command = sub.add_parser("sidecar-command", help="Build a dcc-mcp-server sidecar argv.")
    _add_sidecar_launch_args(sidecar_command)

    launch = sub.add_parser("launch-sidecar", help="Start a sidecar without importing _core.")
    _add_sidecar_launch_args(launch)
    launch.add_argument("--foreground", action="store_true", help="Do not detach the sidecar on Windows.")
    launch.add_argument("--poll-interval-secs", type=float, default=0.25)
    launch.add_argument("--stdio-log-dir", help="Directory for sidecar stdout/stderr logs.")
    launch.add_argument("--no-stdio-log", action="store_true", help="Discard sidecar stdout/stderr.")
    launch.add_argument(
        "--liveness-check-secs",
        type=float,
        default=DEFAULT_SIDECAR_LIVENESS_CHECK_SECS,
        help=(
            "Wait briefly after spawn and fail if the sidecar exits immediately. "
            "Pass 0 to preserve the raw non-blocking API behavior."
        ),
    )
    _add_sidecar_probe_args(launch)
    launch.add_argument(
        "--wait-ready-timeout-secs",
        type=float,
        help="Optionally wait for dispatch readiness after spawning the sidecar.",
    )

    ready = sub.add_parser("sidecar-ready", help="Check per-DCC sidecar dispatch readiness.")
    _add_sidecar_ready_args(ready)

    plan = sub.add_parser("plan-update", help="Plan restart actions for mixed runtime versions.")
    plan.add_argument("--registry-dir")
    plan.add_argument("--dcc-type")
    plan.add_argument("--role")
    plan.add_argument(
        "--target-version",
        action="append",
        default=[],
        help="Target version as KEY=VERSION, for example core=0.17.21.",
    )

    inspect = sub.add_parser("inspect", help="Inspect an install root for loaded native artifacts.")
    inspect.add_argument("install_root")

    remove = sub.add_parser("remove", help="Remove a tree or classify lock failures.")
    remove.add_argument("path")

    replace = sub.add_parser("replace", help="Replace a tree or classify lock failures.")
    replace.add_argument("source")
    replace.add_argument("destination")

    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.command == "query":
        return _print_json(
            query_runtime_state(
                args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                install_root=args.install_root,
                include_dead=args.include_dead,
            )
        )
    if args.command == "stop":
        return _print_json(
            stop_runtime_entries(
                args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                install_root=args.install_root,
                timeout_secs=args.timeout_secs,
                include_host_processes=args.include_host_processes,
            )
        )
    if args.command == "layout":
        return _print_json(
            resolve_deployment_layout(
                args.cache_root,
                packages=args.packages,
                adapter_package=args.adapter_package,
            )
        )
    if args.command == "sidecar-command":
        return _print_json(build_sidecar_command(**_sidecar_launch_kwargs(args)))
    if args.command == "launch-sidecar":
        try:
            probe_arguments = _parse_probe_args(args.probe_args_json)
        except ValueError as exc:
            return _print_json(_failed("invalid_probe_args", str(exc), None))
        return _print_json(
            launch_sidecar(
                **_sidecar_launch_kwargs(args),
                detached=not args.foreground,
                wait_ready_timeout_secs=args.wait_ready_timeout_secs,
                poll_interval_secs=args.poll_interval_secs,
                probe_tool=args.probe_tool,
                probe_arguments=probe_arguments,
                probe_timeout_secs=args.probe_timeout_secs,
                stdio_log_dir=args.stdio_log_dir,
                capture_stdio=not args.no_stdio_log,
                liveness_check_secs=args.liveness_check_secs,
            )
        )
    if args.command == "sidecar-ready":
        try:
            probe_arguments = _parse_probe_args(args.probe_args_json)
        except ValueError as exc:
            return _print_json(_failed("invalid_probe_args", str(exc), None))
        if args.timeout_secs > 0:
            return _print_json(
                wait_for_sidecar_ready(
                    args.registry_dir,
                    dcc_type=args.dcc_type,
                    instance_id=args.instance_id,
                    host_rpc=args.host_rpc,
                    timeout_secs=args.timeout_secs,
                    poll_interval_secs=args.poll_interval_secs,
                    probe_tool=args.probe_tool,
                    probe_arguments=probe_arguments,
                    probe_timeout_secs=args.probe_timeout_secs,
                )
            )
        return _print_json(
            sidecar_readiness_status(
                args.registry_dir,
                dcc_type=args.dcc_type,
                instance_id=args.instance_id,
                host_rpc=args.host_rpc,
                probe_tool=args.probe_tool,
                probe_arguments=probe_arguments,
                probe_timeout_secs=args.probe_timeout_secs,
            )
        )
    if args.command == "plan-update":
        try:
            target_versions = _parse_target_versions(args.target_version)
        except ValueError as exc:
            return _print_json(_failed("invalid_target_version", str(exc), None))
        return _print_json(
            plan_runtime_updates(
                registry_dir=args.registry_dir,
                dcc_type=args.dcc_type,
                role=args.role,
                target_versions=target_versions,
            )
        )
    if args.command == "inspect":
        return _print_json(inspect_install_root(args.install_root))
    if args.command == "remove":
        return _print_json(safe_remove_tree(args.path))
    if args.command == "replace":
        return _print_json(safe_replace_tree(args.source, args.destination))
    parser.error("unknown command")
    return 2


def _add_sidecar_launch_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--dcc-type", "--dcc", dest="dcc_type", required=True)
    parser.add_argument("--host-rpc", required=True)
    parser.add_argument("--watch-pid", type=int, required=True)
    parser.add_argument("--registry-dir")
    parser.add_argument("--server-bin")
    parser.add_argument("--instance-id")
    parser.add_argument("--display-name")
    parser.add_argument("--adapter-version")
    parser.add_argument("--discovery-mcp-url")
    parser.add_argument("--gateway-port", type=int)
    parser.add_argument("--gateway-host")
    parser.add_argument("--gateway-name")
    parser.add_argument("--gateway-remote-host")
    parser.add_argument("--gateway-remote-port", type=int)
    parser.add_argument("--connect-timeout-secs", type=int)
    parser.add_argument("--no-ensure-gateway", action="store_true")
    parser.add_argument("--legacy-gateway-election", action="store_true")
    parser.add_argument(
        "--require-dispatch-capable",
        action="store_true",
        help="Fail if --host-rpc cannot prove production sidecar tool dispatch.",
    )
    parser.add_argument(
        "--extra-sidecar-arg",
        action="append",
        dest="extra_args",
        help="Append a raw argument to the dcc-mcp-server sidecar argv.",
    )


def _add_sidecar_ready_args(parser: argparse.ArgumentParser, *, include_timeout: bool = True) -> None:
    parser.add_argument("--registry-dir")
    parser.add_argument("--dcc-type", "--dcc", dest="dcc_type")
    parser.add_argument("--instance-id")
    parser.add_argument("--host-rpc")
    if include_timeout:
        parser.add_argument("--timeout-secs", type=float, default=0.0)
    parser.add_argument("--poll-interval-secs", type=float, default=0.25)
    _add_sidecar_probe_args(parser)


def _add_sidecar_probe_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--probe-tool", help="Optional read-only tool slug to call before reporting ready.")
    parser.add_argument("--probe-args-json", help="JSON object arguments for --probe-tool.")
    parser.add_argument("--probe-timeout-secs", type=float, default=3.0)


def _sidecar_launch_kwargs(args: argparse.Namespace) -> Dict[str, Any]:
    return {
        "dcc_type": args.dcc_type,
        "host_rpc": args.host_rpc,
        "watch_pid": args.watch_pid,
        "registry_dir": args.registry_dir,
        "server_bin": args.server_bin,
        "instance_id": args.instance_id,
        "display_name": args.display_name,
        "adapter_version": args.adapter_version,
        "discovery_mcp_url": args.discovery_mcp_url,
        "gateway_port": args.gateway_port,
        "gateway_host": args.gateway_host,
        "gateway_name": args.gateway_name,
        "gateway_remote_host": args.gateway_remote_host,
        "gateway_remote_port": args.gateway_remote_port,
        "connect_timeout_secs": args.connect_timeout_secs,
        "no_ensure_gateway": args.no_ensure_gateway,
        "legacy_gateway_election": args.legacy_gateway_election,
        "require_dispatch_capable": args.require_dispatch_capable,
        "extra_args": args.extra_args,
    }


def _parse_probe_args(value: Optional[str]) -> Optional[Dict[str, Any]]:
    if not value:
        return None
    probe_arguments = json.loads(value)
    if not isinstance(probe_arguments, dict):
        raise ValueError("--probe-args-json must decode to a JSON object")
    return probe_arguments


if __name__ == "__main__":
    raise SystemExit(main())
