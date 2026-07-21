"""Run the stateful UI Control skill on a persistent Python MCP server."""

from __future__ import annotations

import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import sys
import threading
from typing import Optional
from typing import Sequence

from dcc_mcp_core import HostExecutionBridge
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server


def _parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run ui-control with a persistent in-process executor.",
    )
    parser.add_argument("--process-id", required=True, type=int)
    parser.add_argument("--window-handle", required=True, type=int)
    parser.add_argument("--host-exe", required=True, type=Path)
    parser.add_argument("--skill-root", required=True, type=Path)
    parser.add_argument("--registry-dir", required=True, type=Path)
    parser.add_argument("--ready-file", required=True, type=Path)
    parser.add_argument("--backend-port", type=int, default=0)
    parser.add_argument("--gateway-port", required=True, type=int)
    parser.add_argument("--allow-raw-input", action="store_true")
    return parser.parse_args(argv)


def _validate_target(process_id: int, window_handle: int) -> str:
    if sys.platform != "win32":
        raise RuntimeError("UI Control target validation requires Windows")

    if process_id <= 0 or window_handle <= 0:
        raise ValueError("process-id and window-handle must be positive")

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    hwnd = ctypes.c_void_p(window_handle)
    if not user32.IsWindow(hwnd):
        raise RuntimeError("window-handle is not a live native window")

    actual_process_id = ctypes.c_uint32()
    if not user32.GetWindowThreadProcessId(hwnd, ctypes.byref(actual_process_id)):
        raise RuntimeError("could not resolve the window owner process")
    if actual_process_id.value != process_id:
        raise RuntimeError(
            f"window target mismatch: expected PID {process_id}, resolved PID {actual_process_id.value}",
        )

    length = user32.GetWindowTextLengthW(hwnd)
    title_buffer = ctypes.create_unicode_buffer(length + 1)
    user32.GetWindowTextW(hwnd, title_buffer, len(title_buffer))
    return title_buffer.value


def _validate_host_executable(path: Path) -> None:
    if not path.is_file():
        raise FileNotFoundError(f"UI Control Host not found: {path}")
    with path.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError(f"UI Control Host is not a Windows PE executable: {path}")


def _write_ready(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    temporary.write_text(
        json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")),
        encoding="utf-8",
    )
    temporary.replace(path)


def _only_ui_control(metadata):
    if metadata.name != "ui-control":
        raise RuntimeError("This dedicated server may load only the ui-control skill.")
    return metadata


def _shutdown(bridge, handle) -> None:
    """Close admission, stop HTTP, then always release owned Host state."""
    try:
        bridge.close_script_admission()
    finally:
        try:
            handle.shutdown()
        finally:
            bridge.clear_script_packages()


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Run until SIGINT or SIGTERM requests an orderly shutdown."""
    args = _parse_args(argv)
    host_exe = args.host_exe.resolve()
    skill_root = args.skill_root.resolve()
    registry_dir = args.registry_dir.resolve()
    ready_file = args.ready_file.resolve()
    _validate_host_executable(host_exe)
    if not skill_root.is_dir():
        raise FileNotFoundError(f"Skill root not found: {skill_root}")
    target_title = _validate_target(args.process_id, args.window_handle)

    os.environ["DCC_MCP_UI_CONTROL_BACKEND"] = "windows-uia"
    os.environ["DCC_MCP_UI_CONTROL_HOST"] = str(host_exe)
    os.environ["DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE"] = str(args.window_handle)
    os.environ.pop("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID", None)
    os.environ["DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS"] = "1"
    os.environ["DCC_MCP_DISABLE_ACCUMULATED_SKILLS"] = "1"
    if args.allow_raw_input:
        os.environ["DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT"] = "true"
    else:
        os.environ.pop("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", None)

    registry_dir.mkdir(parents=True, exist_ok=True)
    config = McpHttpConfig(
        port=args.backend_port,
        server_name="ui-control-server",
        shutdown_on_drop=True,
    )
    config.dcc_type = "python"
    config.adapter_dcc = "ui-control"
    config.gateway_port = args.gateway_port
    config.registry_dir = str(registry_dir)
    config.heartbeat_secs = 1
    config.stale_timeout_secs = 10
    config.allowed_skill_names = ["ui-control"]

    bridge = HostExecutionBridge(dispatcher=None)
    server = create_skill_server(
        "python",
        config,
        extra_paths=[str(skill_root)],
        accumulated=False,
    )
    server.set_skill_load_transform(_only_ui_control)
    server.set_in_process_executor(bridge.as_inprocess_executor())
    loaded_actions = server.load_skill("ui-control")
    handle = server.start()

    try:
        stop_event = threading.Event()

        def _request_stop(_signum, _frame) -> None:
            stop_event.set()

        signal.signal(signal.SIGINT, _request_stop)
        signal.signal(signal.SIGTERM, _request_stop)

        payload = {
            "status": "ready",
            "launcher_pid": os.getpid(),
            "target_process_id": args.process_id,
            "target_window_handle": args.window_handle,
            "target_window_title": target_title,
            "backend_port": handle.port,
            "backend_mcp_url": handle.mcp_url(),
            "gateway_port": args.gateway_port,
            "gateway_mcp_url": f"http://127.0.0.1:{args.gateway_port}/mcp",
            "is_gateway": handle.is_gateway,
            "loaded_actions": list(loaded_actions),
            "host_exe": str(host_exe),
            "raw_input_enabled": bool(args.allow_raw_input),
        }
        _write_ready(ready_file, payload)
        print(json.dumps(payload, ensure_ascii=False, sort_keys=True), flush=True)

        while not stop_event.wait(0.25):
            pass
    finally:
        _shutdown(bridge, handle)
    return 0


def cli() -> int:
    """Console-script entry point with a machine-readable startup error."""
    try:
        return main()
    except Exception as exc:
        print(
            json.dumps(
                {"status": "error", "error": type(exc).__name__, "message": str(exc)},
                ensure_ascii=False,
                sort_keys=True,
            ),
            file=sys.stderr,
            flush=True,
        )
        raise


if __name__ == "__main__":
    raise SystemExit(cli())
