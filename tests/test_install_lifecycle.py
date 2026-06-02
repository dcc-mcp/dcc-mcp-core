"""Tests for import-light adapter install lifecycle helpers."""

from __future__ import annotations

import errno
from http.server import BaseHTTPRequestHandler
from http.server import HTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import types

import pytest

import dcc_mcp_core._install_lifecycle_readiness as readiness_lifecycle
import dcc_mcp_core._install_lifecycle_runtime as runtime_lifecycle
import dcc_mcp_core._install_lifecycle_sidecar as sidecar_lifecycle
import dcc_mcp_core.install_lifecycle as lifecycle

REPO_ROOT = Path(__file__).resolve().parent.parent


def _start_probe_server(response_payload: dict) -> tuple[HTTPServer, str, list[dict]]:
    requests: list[dict] = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            length = int(self.headers.get("content-length", "0"))
            requests.append(json.loads(self.rfile.read(length).decode("utf-8")))
            body = json.dumps(response_payload).encode("utf-8")
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *args: object) -> None:
            return

    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{server.server_port}/mcp"
    return server, url, requests


def _write_ready_sidecar_registry(tmp_path: Path) -> Path:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
                    "host": "127.0.0.1",
                    "port": 18812,
                    "pid": os.getpid(),
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": str(os.getpid()),
                        "mcp_url": "http://127.0.0.1:18812/mcp",
                        "host_rpc_uri": "commandport://127.0.0.1:6000",
                        "dispatch_status": "ready",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )
    return registry


def test_package_import_does_not_load_core_in_fresh_process() -> None:
    script = """
import json
import sys

import dcc_mcp_core
print(json.dumps({"after_package": "dcc_mcp_core._core" in sys.modules}))

import dcc_mcp_core.install_lifecycle
print(json.dumps({"after_lifecycle": "dcc_mcp_core._core" in sys.modules}))
"""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    assert rows == [{"after_package": False}, {"after_lifecycle": False}]


def test_top_level_lifecycle_export_does_not_load_core_in_fresh_process() -> None:
    script = """
import json
import sys

from dcc_mcp_core import inspect_install_root
from dcc_mcp_core import sidecar_host_rpc_dispatch_contract

print(json.dumps({
    "core_loaded": "dcc_mcp_core._core" in sys.modules,
    "module": inspect_install_root.__module__,
    "sidecar_contract_module": sidecar_host_rpc_dispatch_contract.__module__,
    "sidecar_contract_status": sidecar_host_rpc_dispatch_contract("stub://localhost:0")["status"],
}))
"""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [sys.executable, "-c", script],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )

    assert json.loads(result.stdout) == {
        "core_loaded": False,
        "module": "dcc_mcp_core.install_lifecycle",
        "sidecar_contract_module": "dcc_mcp_core._install_lifecycle_sidecar",
        "sidecar_contract_status": "test_only",
    }


def test_module_cli_inspect_returns_json_without_loading_core(tmp_path: Path) -> None:
    install_root = tmp_path / "adapter"
    install_root.mkdir()
    script = """
import json
import runpy
import sys

sys.argv = [
    "dcc_mcp_core.install_lifecycle",
    "inspect",
    sys.argv[1],
]
try:
    runpy.run_module("dcc_mcp_core.install_lifecycle", run_name="__main__")
except SystemExit as exc:
    code = exc.code
else:
    code = 0
print(json.dumps({"core_loaded": "dcc_mcp_core._core" in sys.modules, "exit_code": code}))
"""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [sys.executable, "-c", script, str(install_root)],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    payload, end = json.JSONDecoder().raw_decode(result.stdout)
    trailer = json.loads(result.stdout[end:].strip())

    assert payload["success"] is True
    assert payload["status"] == "ok"
    assert trailer == {"core_loaded": False, "exit_code": 0}


def test_inspect_install_root_reports_loaded_native_artifact(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    native = tmp_path / "dcc_mcp_core" / "_core.pyd"
    native.parent.mkdir()
    native.write_bytes(b"placeholder")
    fake_core = types.ModuleType("dcc_mcp_core._core")
    fake_core.__file__ = str(native)
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)

    result = lifecycle.inspect_install_root(tmp_path)

    assert result["status"] == "requires_restart"
    assert result["requires_restart"] is True
    assert result["locked_path"] == str(native.resolve())
    assert result["loaded_native_artifacts"] == [{"module": "dcc_mcp_core._core", "path": str(native.resolve())}]


def test_safe_remove_tree_classifies_windows_permission_error(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    install_root = tmp_path / "adapter"
    locked = install_root / "dcc_mcp_core" / "_core.pyd"
    locked.parent.mkdir(parents=True)
    locked.write_bytes(b"placeholder")

    def deny_remove(_path: str) -> None:
        raise PermissionError(errno.EACCES, "Access is denied", str(locked))

    monkeypatch.setattr(lifecycle.shutil, "rmtree", deny_remove)
    monkeypatch.setattr(lifecycle, "_is_windows_lock_error", lambda _exc: True)

    result = lifecycle.safe_remove_tree(install_root)

    assert result["status"] == "requires_restart"
    assert result["requires_restart"] is True
    assert result["reason"] == "windows_file_lock"
    assert result["locked_path"] == str(locked.resolve())
    assert result["deferred_operation"] == {
        "operation": "remove_tree",
        "path": str(install_root.resolve()),
    }


def test_windows_lock_classifier_ignores_posix_permission_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(lifecycle.os, "name", "posix")

    assert lifecycle._is_windows_lock_error(PermissionError(errno.EACCES, "Permission denied")) is False


def test_windows_lock_classifier_accepts_windows_permission_error(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(lifecycle.os, "name", "nt")

    assert lifecycle._is_windows_lock_error(PermissionError(errno.EACCES, "Access is denied")) is True


def test_safe_replace_tree_copies_after_remove(tmp_path: Path) -> None:
    source = tmp_path / "new"
    destination = tmp_path / "installed"
    source.mkdir()
    (source / "module.py").write_text("VALUE = 1\n", encoding="utf-8")
    destination.mkdir()
    (destination / "old.py").write_text("OLD = 1\n", encoding="utf-8")

    result = lifecycle.safe_replace_tree(source, destination)

    assert result["status"] == "replaced"
    assert (destination / "module.py").read_text(encoding="utf-8") == "VALUE = 1\n"
    assert not (destination / "old.py").exists()


def test_query_runtime_state_reads_sidecar_pid(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "11111111-1111-1111-1111-111111111111",
                    "host": "127.0.0.1",
                    "port": 18812,
                    "pid": os.getpid(),
                    "status": "available",
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": str(os.getpid()),
                        "mcp_url": "http://127.0.0.1:18812/mcp",
                        "host_rpc_uri": "commandport://127.0.0.1:6000",
                        "host_rpc_scheme": "commandport",
                        "dispatch_status": "ready",
                        "dispatch_ready_at_unix": "1800000000",
                        "gateway_runtime_mode": "daemon-backed",
                        "gateway_guardian_enabled": "true",
                    },
                },
                {
                    "dcc_type": "photoshop",
                    "instance_id": "22222222-2222-2222-2222-222222222222",
                    "host": "127.0.0.1",
                    "port": 18813,
                    "pid": 3456,
                    "metadata": {},
                },
            ]
        ),
        encoding="utf-8",
    )

    result = lifecycle.query_runtime_state(registry, dcc_type="maya", role="per-dcc-sidecar")

    assert result["total"] == 1
    assert result["entries"][0]["dcc_type"] == "maya"
    assert result["entries"][0]["parent_pid"] == os.getpid()
    assert result["entries"][0]["sidecar_pid"] == os.getpid()
    assert result["entries"][0]["runtime_pid"] == os.getpid()
    assert result["entries"][0]["mcp_url"] == "http://127.0.0.1:18812/mcp"
    assert result["entries"][0]["host_rpc_uri"] == "commandport://127.0.0.1:6000"
    assert result["entries"][0]["host_rpc_scheme"] == "commandport"
    assert result["entries"][0]["dispatch_status"] == "ready"
    assert result["entries"][0]["dispatch_ready"] is True
    assert result["entries"][0]["gateway_runtime_mode"] == "daemon-backed"
    assert result["entries"][0]["gateway_guardian_enabled"] is True
    assert result["entries"][0]["dispatch"] == {
        "reported": True,
        "status": "ready",
        "ready": True,
        "ready_at_unix": "1800000000",
        "host_rpc_uri": "commandport://127.0.0.1:6000",
        "host_rpc_scheme": "commandport",
        "failure_stage": None,
        "failure_reason": None,
    }


def test_query_runtime_state_marks_missing_dispatch_not_reported(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "photoshop",
                    "instance_id": "22222222-2222-2222-2222-222222222222",
                    "host": "127.0.0.1",
                    "port": 18813,
                    "pid": os.getpid(),
                    "metadata": {},
                }
            ]
        ),
        encoding="utf-8",
    )

    result = lifecycle.query_runtime_state(registry, dcc_type="photoshop")

    assert result["total"] == 1
    assert result["entries"][0]["dispatch_status"] is None
    assert result["entries"][0]["dispatch_ready"] is False
    assert result["entries"][0]["dispatch"] == {
        "reported": False,
        "status": "not_reported",
        "ready": None,
        "ready_at_unix": None,
        "host_rpc_uri": None,
        "host_rpc_scheme": None,
        "failure_stage": None,
        "failure_reason": None,
    }


def test_query_runtime_state_surfaces_unavailable_sidecar_dispatch(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "11111111-1111-1111-1111-111111111111",
                    "host": "127.0.0.1",
                    "port": 0,
                    "pid": 1234,
                    "status": "booting",
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": "2345",
                        "host_rpc_uri": "commandport://127.0.0.1:6000",
                        "host_rpc_scheme": "commandport",
                        "dispatch_status": "unavailable",
                        "failure_stage": "host-rpc-connect",
                        "failure_reason": "host-rpc connect failed",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )

    result = lifecycle.query_runtime_state(registry, dcc_type="maya", role="per-dcc-sidecar")

    entry = result["entries"][0]
    assert entry["dispatch_status"] == "unavailable"
    assert entry["dispatch_ready"] is False
    assert entry["mcp_url"] is None
    assert entry["failure_stage"] == "host-rpc-connect"
    assert entry["failure_reason"] == "host-rpc connect failed"
    assert entry["dispatch"] == {
        "reported": True,
        "status": "unavailable",
        "ready": False,
        "ready_at_unix": None,
        "host_rpc_uri": "commandport://127.0.0.1:6000",
        "host_rpc_scheme": "commandport",
        "failure_stage": "host-rpc-connect",
        "failure_reason": "host-rpc connect failed",
    }


def test_sidecar_readiness_status_reports_ready_entry(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
                    "host": "127.0.0.1",
                    "port": 18812,
                    "pid": os.getpid(),
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": str(os.getpid()),
                        "mcp_url": "http://127.0.0.1:18812/mcp",
                        "host_rpc_uri": "commandport://127.0.0.1:6000",
                        "dispatch_status": "ready",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )

    result = lifecycle.sidecar_readiness_status(
        registry,
        dcc_type="maya",
        instance_id="aaaaaaaa",
        host_rpc="commandport://127.0.0.1:6000",
    )

    assert result["success"] is True
    assert result["status"] == "ready"
    assert result["ready"] is True
    assert result["entry"]["mcp_url"] == "http://127.0.0.1:18812/mcp"


def test_probe_sidecar_tool_posts_jsonrpc_tools_call() -> None:
    server, url, requests = _start_probe_server({"jsonrpc": "2.0", "id": "ignored", "result": {"success": True}})
    try:
        result = lifecycle.probe_sidecar_tool(
            url,
            "maya_diagnostics__ping",
            {"level": "quick"},
            timeout_secs=2.0,
        )
    finally:
        server.shutdown()
        server.server_close()

    assert result["success"] is True
    assert result["status"] == "probe_ok"
    assert result["result"] == {"success": True}
    assert requests[0]["method"] == "tools/call"
    assert requests[0]["params"] == {
        "name": "maya_diagnostics__ping",
        "arguments": {"level": "quick"},
    }


def test_probe_sidecar_tool_reports_jsonrpc_error() -> None:
    server, url, _requests = _start_probe_server(
        {
            "jsonrpc": "2.0",
            "id": "ignored",
            "error": {
                "code": -32000,
                "message": "sidecar-dispatcher-unavailable",
                "data": {"kind": "backend-error"},
            },
        }
    )
    try:
        result = lifecycle.probe_sidecar_tool(url, "maya_diagnostics__ping", timeout_secs=2.0)
    finally:
        server.shutdown()
        server.server_close()

    assert result["success"] is False
    assert result["status"] == "probe_failed"
    assert result["error"]["message"] == "sidecar-dispatcher-unavailable"


def test_probe_sidecar_tool_reports_mcp_error_result() -> None:
    server, url, _requests = _start_probe_server(
        {
            "jsonrpc": "2.0",
            "id": "ignored",
            "result": {
                "isError": True,
                "content": [{"type": "text", "text": "dispatcher unavailable"}],
            },
        }
    )
    try:
        result = lifecycle.probe_sidecar_tool(url, "maya_diagnostics__ping", timeout_secs=2.0)
    finally:
        server.shutdown()
        server.server_close()

    assert result["success"] is False
    assert result["status"] == "probe_failed"
    assert result["result"]["isError"] is True


def test_sidecar_readiness_status_accepts_probe_success(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    registry = _write_ready_sidecar_registry(tmp_path)
    monkeypatch.setattr(
        readiness_lifecycle,
        "probe_sidecar_tool",
        lambda *args, **kwargs: {"success": True, "status": "probe_ok", "tool_name": args[1]},
    )

    result = lifecycle.sidecar_readiness_status(
        registry,
        dcc_type="maya",
        probe_tool="maya_diagnostics__ping",
        probe_arguments={"level": "quick"},
    )

    assert result["success"] is True
    assert result["status"] == "ready"
    assert result["probe"]["status"] == "probe_ok"


def test_sidecar_readiness_status_reports_probe_failure(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    registry = _write_ready_sidecar_registry(tmp_path)
    monkeypatch.setattr(
        readiness_lifecycle,
        "probe_sidecar_tool",
        lambda *args, **kwargs: {
            "success": False,
            "status": "probe_failed",
            "message": "sidecar-dispatcher-unavailable",
        },
    )

    result = lifecycle.sidecar_readiness_status(registry, dcc_type="maya", probe_tool="maya_diagnostics__ping")

    assert result["success"] is False
    assert result["ready"] is False
    assert result["status"] == "probe_failed"
    assert result["probe"]["message"] == "sidecar-dispatcher-unavailable"
    assert "dispatcher" in result["recommended_next_action"]


def test_sidecar_readiness_status_reports_unavailable_failure(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
                    "host": "127.0.0.1",
                    "port": 0,
                    "pid": os.getpid(),
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": str(os.getpid()),
                        "host_rpc_uri": "commandport://127.0.0.1:6000",
                        "dispatch_status": "unavailable",
                        "failure_stage": "host-rpc-connect",
                        "failure_reason": "connection refused",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )

    result = lifecycle.sidecar_readiness_status(registry, dcc_type="maya")

    assert result["success"] is False
    assert result["status"] == "unavailable"
    assert result["failure_stage"] == "host-rpc-connect"
    assert result["failure_reason"] == "connection refused"
    assert "host RPC bridge" in result["recommended_next_action"]


def test_sidecar_readiness_status_reports_missing_selector(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text("[]", encoding="utf-8")

    result = lifecycle.sidecar_readiness_status(registry, dcc_type="houdini")

    assert result["success"] is False
    assert result["status"] == "missing"
    assert result["selector"]["dcc_type"] == "houdini"
    assert result["entries"] == []


def test_wait_for_sidecar_ready_polls_until_ready(monkeypatch: pytest.MonkeyPatch) -> None:
    responses = iter(
        [
            {"success": False, "status": "missing", "ready": False},
            {"success": False, "status": "booting", "ready": False},
            {"success": True, "status": "ready", "ready": True},
        ]
    )
    monkeypatch.setattr(readiness_lifecycle, "sidecar_readiness_status", lambda *args, **kwargs: next(responses))
    monkeypatch.setattr(readiness_lifecycle.time, "sleep", lambda _secs: None)

    result = lifecycle.wait_for_sidecar_ready(timeout_secs=5.0, poll_interval_secs=0.05)

    assert result["success"] is True
    assert result["status"] == "ready"
    assert result["elapsed_secs"] >= 0


def test_wait_for_sidecar_ready_polls_retryable_unavailable_until_ready(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    responses = iter(
        [
            {
                "success": False,
                "status": "unavailable",
                "ready": False,
                "failure_stage": "host-rpc-connect",
                "entry": {"host_rpc_scheme": "commandport"},
            },
            {"success": True, "status": "ready", "ready": True},
        ]
    )
    monkeypatch.setattr(readiness_lifecycle, "sidecar_readiness_status", lambda *args, **kwargs: next(responses))
    monkeypatch.setattr(readiness_lifecycle.time, "sleep", lambda _secs: None)

    result = lifecycle.wait_for_sidecar_ready(timeout_secs=5.0, poll_interval_secs=0.05)

    assert result["success"] is True
    assert result["status"] == "ready"


def test_wait_for_sidecar_ready_returns_non_retryable_unavailable(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls = []

    def fake_status(*args: object, **kwargs: object) -> dict:
        calls.append((args, kwargs))
        return {
            "success": False,
            "status": "unavailable",
            "ready": False,
            "failure_stage": "host-rpc-scheme",
            "entry": {"host_rpc_scheme": "stub"},
        }

    monkeypatch.setattr(readiness_lifecycle, "sidecar_readiness_status", fake_status)
    monkeypatch.setattr(readiness_lifecycle.time, "sleep", lambda _secs: None)

    result = lifecycle.wait_for_sidecar_ready(timeout_secs=5.0, poll_interval_secs=0.05)

    assert result["status"] == "unavailable"
    assert len(calls) == 1


def test_wait_for_sidecar_ready_polls_probe_failure_until_success(monkeypatch: pytest.MonkeyPatch) -> None:
    responses = iter(
        [
            {"success": False, "status": "probe_failed", "ready": False},
            {"success": True, "status": "ready", "ready": True},
        ]
    )
    monkeypatch.setattr(readiness_lifecycle, "sidecar_readiness_status", lambda *args, **kwargs: next(responses))
    monkeypatch.setattr(readiness_lifecycle.time, "sleep", lambda _secs: None)

    result = lifecycle.wait_for_sidecar_ready(
        timeout_secs=5.0,
        poll_interval_secs=0.05,
        probe_tool="maya_diagnostics__ping",
    )

    assert result["success"] is True
    assert result["status"] == "ready"


def test_wait_for_sidecar_ready_returns_timeout(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        readiness_lifecycle,
        "sidecar_readiness_status",
        lambda *args, **kwargs: {"success": False, "status": "booting", "ready": False},
    )
    monkeypatch.setattr(readiness_lifecycle.time, "sleep", lambda _secs: None)

    result = lifecycle.wait_for_sidecar_ready(timeout_secs=0.0, poll_interval_secs=0.05)

    assert result["success"] is False
    assert result["status"] == "timeout"
    assert result["last_status"] == "booting"


def test_stop_runtime_entries_does_not_kill_host_by_default(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "zbrush",
                    "instance_id": "33333333-3333-3333-3333-333333333333",
                    "host": "127.0.0.1",
                    "port": 18814,
                    "pid": 999999,
                    "metadata": {"dcc_mcp_role": "per-dcc-sidecar"},
                }
            ]
        ),
        encoding="utf-8",
    )
    killed = []
    monkeypatch.setattr(runtime_lifecycle, "_entry_runtime_alive", lambda _sentinel, _pid: True)
    monkeypatch.setattr(lifecycle.os, "kill", lambda pid, sig: killed.append((pid, sig)))

    result = lifecycle.stop_runtime_entries(registry, dcc_type="zbrush")

    assert killed == []
    assert result["success"] is False
    assert result["results"][0]["status"] == "unsupported"


def test_stop_runtime_entries_respects_dead_sentinel_before_pid(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    registry = tmp_path / "registry"
    locks = registry / "locks"
    locks.mkdir(parents=True)
    sentinel = locks / "maya-33333333-3333-3333-3333-333333333333.lock"
    sentinel.write_bytes(b"")
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "33333333-3333-3333-3333-333333333333",
                    "host": "127.0.0.1",
                    "port": 18814,
                    "pid": 999999,
                    "sentinel_path": str(sentinel),
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": "888888",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )
    killed = []
    monkeypatch.setattr(lifecycle.os, "kill", lambda pid, sig: killed.append((pid, sig)))

    state = lifecycle.query_runtime_state(registry, dcc_type="maya", role="per-dcc-sidecar")
    result = lifecycle.stop_runtime_entries(registry, dcc_type="maya")

    assert state["entries"][0]["runtime_alive"] is False
    assert state["entries"][0]["sentinel_path"] == str(sentinel.resolve())
    assert killed == []
    assert result["success"] is True
    assert result["results"][0]["status"] == "already_stopped"


def test_resolve_deployment_layout_uses_rez_env_roots(tmp_path: Path) -> None:
    core_root = tmp_path / "dcc_mcp_core"
    server_root = tmp_path / "dcc_mcp_server"
    maya_root = tmp_path / "dcc_mcp_maya"
    (core_root / "python").mkdir(parents=True)
    (server_root / "bin").mkdir(parents=True)
    (maya_root / "python").mkdir(parents=True)
    env = {
        "REZ_USED_RESOLVE": "dcc_mcp_core dcc_mcp_server dcc_mcp_maya",
        "REZ_DCC_MCP_CORE_ROOT": str(core_root),
        "REZ_DCC_MCP_SERVER_ROOT": str(server_root),
        "REZ_DCC_MCP_MAYA_ROOT": str(maya_root),
    }

    result = lifecycle.resolve_deployment_layout(adapter_package="dcc_mcp_maya", env=env)

    assert result["mode"] == "rez"
    assert result["missing_packages"] == []
    assert result["environment"]["prepend"]["PYTHONPATH"] == [
        str((core_root / "python").resolve()),
        str((maya_root / "python").resolve()),
    ]
    assert result["environment"]["prepend"]["PATH"] == [str((server_root / "bin").resolve())]


def test_resolve_deployment_layout_uses_cache_root_before_packages_exist(tmp_path: Path) -> None:
    cache_root = tmp_path / "ext"
    (cache_root / "dcc_mcp_core" / "python").mkdir(parents=True)
    (cache_root / "dcc_mcp_server").mkdir(parents=True)

    result = lifecycle.resolve_deployment_layout(
        cache_root,
        adapter_package="dcc_mcp_3dsmax",
    )

    assert result["mode"] == "filesystem"
    assert result["missing_packages"] == ["dcc_mcp_3dsmax"]
    assert result["packages"][0]["source"] == "cache-root"
    assert result["packages"][0]["root"] == str((cache_root / "dcc_mcp_core").resolve())


def test_build_sidecar_command_uses_sidecar_cli_contract(tmp_path: Path) -> None:
    registry = tmp_path / "registry"

    result = lifecycle.build_sidecar_command(
        dcc_type="maya",
        host_rpc="commandport://127.0.0.1:6000",
        watch_pid=12345,
        registry_dir=registry,
        display_name="Maya-Anim",
        adapter_version="1.2.3",
        gateway_port=19765,
        gateway_host="127.0.0.1",
        server_bin="dcc-mcp-server-test",
    )

    assert result["success"] is True
    assert result["role"] == "per-dcc-sidecar"
    assert result["registry_dir"] == str(registry.resolve())
    assert result["environment"]["set"] == {
        "DCC_MCP_REGISTRY_DIR": str(registry.resolve()),
        "DCC_MCP_GATEWAY_PORT": "19765",
        "DCC_MCP_GATEWAY_HOST": "127.0.0.1",
    }
    assert result["command"] == [
        "dcc-mcp-server-test",
        "sidecar",
        "--dcc",
        "maya",
        "--host-rpc",
        "commandport://127.0.0.1:6000",
        "--watch-pid",
        "12345",
        "--registry-dir",
        str(registry.resolve()),
        "--gateway-port",
        "19765",
        "--display-name",
        "Maya-Anim",
        "--adapter-version",
        "1.2.3",
        "--gateway-host",
        "127.0.0.1",
    ]
    assert result["readiness_selector"] == {
        "dcc_type": "maya",
        "instance_id": None,
        "host_rpc": "commandport://127.0.0.1:6000",
    }
    assert result["readiness_argv"] == [
        "sidecar-ready",
        "--dcc",
        "maya",
        "--host-rpc",
        "commandport://127.0.0.1:6000",
        "--registry-dir",
        str(registry.resolve()),
    ]
    assert result["readiness_command"] == [
        sys.executable,
        "-m",
        "dcc_mcp_core.install_lifecycle",
        *result["readiness_argv"],
    ]
    assert result["dispatch_contract"] == {
        "host_rpc": "commandport://127.0.0.1:6000",
        "scheme": "commandport",
        "supported_schemes": ["commandport", "qtserver", "ws", "wss"],
        "test_only_schemes": ["stub"],
        "status": "dispatch_capable",
        "dispatch_ready_capable": True,
        "test_only": False,
        "reason": None,
        "message": "The sidecar can become dispatch-ready once the DCC host RPC bridge accepts a connection.",
    }


def test_build_sidecar_command_readiness_command_honors_python_env(tmp_path: Path) -> None:
    result = lifecycle.build_sidecar_command(
        dcc_type="houdini",
        host_rpc="qtserver://127.0.0.1:7001",
        watch_pid=12345,
        registry_dir=tmp_path / "registry",
        server_bin="dcc-mcp-server-test",
        env={"DCC_MCP_PYTHON_EXECUTABLE": r"C:\Houdini\bin\hython.exe"},
    )

    assert result["success"] is True
    assert result["readiness_command"][:3] == [
        r"C:\Houdini\bin\hython.exe",
        "-m",
        "dcc_mcp_core.install_lifecycle",
    ]


def test_build_sidecar_command_forwards_extra_sidecar_args(tmp_path: Path) -> None:
    result = lifecycle.build_sidecar_command(
        dcc_type="maya",
        host_rpc="stub://localhost:0",
        watch_pid=12345,
        registry_dir=tmp_path / "registry",
        server_bin="dcc-mcp-server-test",
        extra_args=["--allow-stub-dispatch-ready", "--ppid-poll-ms", 25],
    )

    assert result["success"] is True
    assert result["command"][-3:] == ["--allow-stub-dispatch-ready", "--ppid-poll-ms", "25"]
    assert result["dispatch_contract"]["status"] == "test_only"
    assert result["dispatch_contract"]["dispatch_ready_capable"] is False
    assert "diagnostic row" in result["recommended_next_action"]


def test_build_sidecar_command_can_require_dispatch_capable(tmp_path: Path) -> None:
    result = lifecycle.build_sidecar_command(
        dcc_type="maya",
        host_rpc="stub://localhost:0",
        watch_pid=12345,
        registry_dir=tmp_path / "registry",
        server_bin="dcc-mcp-server-test",
        require_dispatch_capable=True,
    )

    assert result["success"] is False
    assert result["reason"] == "dispatch_not_capable"
    assert result["dispatch_contract"]["status"] == "test_only"
    assert result["dispatch_contract"]["dispatch_ready_capable"] is False


def test_sidecar_host_rpc_dispatch_contract_reports_unsupported_scheme() -> None:
    result = lifecycle.sidecar_host_rpc_dispatch_contract("foo://127.0.0.1:6000")

    assert result["status"] == "unsupported"
    assert result["scheme"] == "foo"
    assert result["dispatch_ready_capable"] is False
    assert result["reason"] == "unsupported_host_rpc_scheme"


def test_build_sidecar_command_returns_structured_validation_error() -> None:
    result = lifecycle.build_sidecar_command(
        dcc_type="",
        host_rpc="commandport://127.0.0.1:6000",
        watch_pid=12345,
    )

    assert result["success"] is False
    assert result["reason"] == "invalid_dcc_type"


def test_launch_sidecar_uses_detached_popen_contract(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured = {}

    class FakePopen:
        pid = 4242

        def __init__(self, command, **kwargs):
            captured["command"] = command
            captured["kwargs"] = kwargs

    monkeypatch.setattr(sidecar_lifecycle.subprocess, "Popen", FakePopen)

    result = lifecycle.launch_sidecar(
        dcc_type="houdini",
        host_rpc="qtserver://127.0.0.1:7001",
        watch_pid=2468,
        registry_dir=tmp_path / "registry",
        server_bin="dcc-mcp-server-test",
        detached=True,
        extra_args=["--ppid-poll-ms", 50],
        env={"PATH": ""},
    )

    assert result["success"] is True
    assert result["status"] == "started"
    assert result["pid"] == 4242
    assert captured["command"] == result["command"]
    assert captured["command"][-2:] == ["--ppid-poll-ms", "50"]
    assert captured["kwargs"]["stdin"] == sidecar_lifecycle.subprocess.DEVNULL
    assert captured["kwargs"]["stdout"] == sidecar_lifecycle.subprocess.DEVNULL
    assert captured["kwargs"]["stderr"] == sidecar_lifecycle.subprocess.DEVNULL
    assert captured["kwargs"]["env"]["DCC_MCP_REGISTRY_DIR"] == str((tmp_path / "registry").resolve())
    assert captured["kwargs"]["env"]["DCC_MCP_GATEWAY_PORT"] == "9765"


def test_launch_sidecar_can_return_bounded_readiness_verdict(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured: dict[str, object] = {}

    class FakePopen:
        pid = 4343

        def __init__(self, command, **kwargs):
            captured["command"] = command
            captured["kwargs"] = kwargs

    def fake_check(**kwargs: object) -> dict:
        captured["readiness_kwargs"] = kwargs
        return {"success": True, "status": "ready", "ready": True}

    monkeypatch.setattr(sidecar_lifecycle.subprocess, "Popen", FakePopen)
    monkeypatch.setattr(sidecar_lifecycle, "_check_launch_readiness", fake_check)

    result = lifecycle.launch_sidecar(
        dcc_type="maya",
        host_rpc="commandport://127.0.0.1:6000",
        watch_pid=2468,
        registry_dir=tmp_path / "registry",
        instance_id="aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
        server_bin="dcc-mcp-server-test",
        wait_ready_timeout_secs=5.0,
        poll_interval_secs=0.1,
        probe_tool="maya_diagnostics__ping",
        probe_arguments={"level": "quick"},
        probe_timeout_secs=1.5,
        env={"PATH": ""},
    )

    assert result["success"] is True
    assert result["status"] == "started"
    assert result["ready"] is True
    assert result["readiness"] == {"success": True, "status": "ready", "ready": True}
    assert captured["readiness_kwargs"] == {
        "registry_dir": str((tmp_path / "registry").resolve()),
        "dcc_type": "maya",
        "instance_id": "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
        "host_rpc": "commandport://127.0.0.1:6000",
        "timeout_secs": 5.0,
        "poll_interval_secs": 0.1,
        "probe_tool": "maya_diagnostics__ping",
        "probe_arguments": {"level": "quick"},
        "probe_timeout_secs": 1.5,
    }


def test_module_cli_sidecar_command_returns_json_without_loading_core(tmp_path: Path) -> None:
    script = """
import json
import runpy
import sys

sys.argv = [
    "dcc_mcp_core.install_lifecycle",
    "sidecar-command",
    "--dcc",
    "photoshop",
    "--host-rpc",
    "ws://127.0.0.1:9000",
    "--watch-pid",
    "34567",
    "--registry-dir",
    sys.argv[1],
    "--server-bin",
    "dcc-mcp-server-test",
]
try:
    runpy.run_module("dcc_mcp_core.install_lifecycle", run_name="__main__")
except SystemExit as exc:
    code = exc.code
else:
    code = 0
print(json.dumps({"core_loaded": "dcc_mcp_core._core" in sys.modules, "exit_code": code}))
"""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [sys.executable, "-c", script, str(tmp_path / "registry")],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    payload, end = json.JSONDecoder().raw_decode(result.stdout)
    trailer = json.loads(result.stdout[end:].strip())

    assert payload["success"] is True
    assert payload["command"][:2] == ["dcc-mcp-server-test", "sidecar"]
    assert payload["dcc_type"] == "photoshop"
    assert trailer == {"core_loaded": False, "exit_code": 0}


def test_module_cli_sidecar_ready_returns_json_without_loading_core(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb",
                    "host": "127.0.0.1",
                    "port": 18812,
                    "pid": os.getpid(),
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "sidecar_pid": str(os.getpid()),
                        "mcp_url": "http://127.0.0.1:18812/mcp",
                        "dispatch_status": "ready",
                    },
                }
            ]
        ),
        encoding="utf-8",
    )
    script = """
import json
import runpy
import sys

sys.argv = [
    "dcc_mcp_core.install_lifecycle",
    "sidecar-ready",
    "--dcc",
    "maya",
    "--registry-dir",
    sys.argv[1],
]
try:
    runpy.run_module("dcc_mcp_core.install_lifecycle", run_name="__main__")
except SystemExit as exc:
    code = exc.code
else:
    code = 0
print(json.dumps({"core_loaded": "dcc_mcp_core._core" in sys.modules, "exit_code": code}))
"""
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [sys.executable, "-c", script, str(registry)],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    payload, end = json.JSONDecoder().raw_decode(result.stdout)
    trailer = json.loads(result.stdout[end:].strip())

    assert payload["success"] is True
    assert payload["status"] == "ready"
    assert trailer == {"core_loaded": False, "exit_code": 0}


def test_cli_launch_sidecar_passes_readiness_and_extra_args(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    seen: dict[str, object] = {}

    def fake_launch(**kwargs: object) -> dict:
        seen.update(kwargs)
        return {"success": True, "status": "started", "pid": 4242}

    monkeypatch.setattr(lifecycle, "launch_sidecar", fake_launch)

    code = lifecycle.main(
        [
            "launch-sidecar",
            "--dcc",
            "maya",
            "--host-rpc",
            "commandport://127.0.0.1:6000",
            "--watch-pid",
            "2468",
            "--server-bin",
            "dcc-mcp-server-test",
            "--extra-sidecar-arg=--ppid-poll-ms",
            "--extra-sidecar-arg",
            "25",
            "--wait-ready-timeout-secs",
            "5",
            "--poll-interval-secs",
            "0.1",
            "--probe-tool",
            "maya_diagnostics__ping",
            "--probe-args-json",
            '{"level":"quick"}',
            "--probe-timeout-secs",
            "1.5",
        ]
    )

    assert code == 0
    assert json.loads(capsys.readouterr().out)["pid"] == 4242
    assert seen["extra_args"] == ["--ppid-poll-ms", "25"]
    assert seen["require_dispatch_capable"] is False
    assert seen["wait_ready_timeout_secs"] == 5.0
    assert seen["poll_interval_secs"] == 0.1
    assert seen["probe_tool"] == "maya_diagnostics__ping"
    assert seen["probe_arguments"] == {"level": "quick"}
    assert seen["probe_timeout_secs"] == 1.5


def test_cli_sidecar_command_can_require_dispatch_capable(capsys: pytest.CaptureFixture[str]) -> None:
    code = lifecycle.main(
        [
            "sidecar-command",
            "--dcc",
            "maya",
            "--host-rpc",
            "foo://127.0.0.1:6000",
            "--watch-pid",
            "2468",
            "--require-dispatch-capable",
        ]
    )

    assert code == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["reason"] == "dispatch_not_capable"
    assert payload["dispatch_contract"]["status"] == "unsupported"


def test_cli_sidecar_ready_passes_probe_arguments(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    registry = _write_ready_sidecar_registry(tmp_path)
    seen: dict[str, object] = {}

    def fake_status(*args: object, **kwargs: object) -> dict:
        seen.update(kwargs)
        return {"success": True, "status": "ready", "ready": True}

    monkeypatch.setattr(lifecycle, "sidecar_readiness_status", fake_status)

    code = lifecycle.main(
        [
            "sidecar-ready",
            "--dcc",
            "maya",
            "--registry-dir",
            str(registry),
            "--probe-tool",
            "maya_diagnostics__ping",
            "--probe-args-json",
            '{"level":"quick"}',
            "--probe-timeout-secs",
            "1.5",
        ]
    )

    assert code == 0
    assert json.loads(capsys.readouterr().out)["status"] == "ready"
    assert seen["probe_tool"] == "maya_diagnostics__ping"
    assert seen["probe_arguments"] == {"level": "quick"}
    assert seen["probe_timeout_secs"] == 1.5


def test_cli_sidecar_ready_rejects_non_object_probe_arguments(capsys: pytest.CaptureFixture[str]) -> None:
    code = lifecycle.main(
        [
            "sidecar-ready",
            "--probe-tool",
            "maya_diagnostics__ping",
            "--probe-args-json",
            '["not", "an", "object"]',
        ]
    )

    assert code == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["reason"] == "invalid_probe_args"
    assert "JSON object" in payload["message"]


def test_plan_runtime_updates_marks_old_sidecar_restartable(tmp_path: Path) -> None:
    registry = tmp_path / "registry"
    registry.mkdir()
    (registry / "services.json").write_text(
        json.dumps(
            [
                {
                    "dcc_type": "maya",
                    "instance_id": "44444444-4444-4444-4444-444444444444",
                    "host": "127.0.0.1",
                    "port": 18815,
                    "pid": 4444,
                    "version": "2026",
                    "adapter_version": "1.0.0",
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "dcc_mcp_core_version": "0.17.20",
                        "dcc_mcp_server_version": "0.17.20",
                        "sidecar_pid": "5555",
                    },
                },
                {
                    "dcc_type": "3dsmax",
                    "instance_id": "55555555-5555-5555-5555-555555555555",
                    "host": "127.0.0.1",
                    "port": 18816,
                    "pid": 5555,
                    "version": "2025",
                    "adapter_version": "1.2.0",
                    "metadata": {
                        "dcc_mcp_role": "per-dcc-sidecar",
                        "dcc_mcp_core_version": "0.17.21",
                        "dcc_mcp_server_version": "0.17.21",
                        "sidecar_pid": "6666",
                    },
                },
            ]
        ),
        encoding="utf-8",
    )
    state = lifecycle.query_runtime_state(registry)

    plan = lifecycle.plan_runtime_updates(
        state,
        target_versions={"core": "0.17.21", "server": "0.17.21", "adapter": "1.2.0"},
    )

    maya = next(item for item in plan["plans"] if item["dcc_type"] == "maya")
    max_entry = next(item for item in plan["plans"] if item["dcc_type"] == "3dsmax")
    assert maya["action"] == "restart_sidecar"
    assert maya["restartable"] is True
    assert maya["stale_components"] == ["core", "server", "adapter"]
    assert max_entry["action"] == "keep"
    assert max_entry["stale_components"] == []


def test_plan_runtime_updates_does_not_treat_dcc_version_as_core_version() -> None:
    plan = lifecycle.plan_runtime_updates(
        [
            {
                "dcc_type": "maya",
                "instance_id": "77777777-7777-7777-7777-777777777777",
                "version": "2026",
                "versions": {"core": None},
                "sidecar_pid": 1234,
            }
        ],
        target_versions={"core": "0.17.21"},
    )

    row = plan["plans"][0]
    assert row["versions"]["core"]["current"] is None
    assert row["versions"]["core"]["status"] == "unknown"
    assert row["unknown_components"] == ["core"]
    assert row["action"] == "verify_runtime_metadata"
    assert plan["verification_required_count"] == 1


def test_plan_runtime_updates_marks_host_only_runtime_manual() -> None:
    plan = lifecycle.plan_runtime_updates(
        [
            {
                "dcc_type": "photoshop",
                "instance_id": "66666666-6666-6666-6666-666666666666",
                "versions": {"core": "0.17.20"},
                "parent_pid": 7777,
                "sidecar_pid": None,
            }
        ],
        target_versions={"core": "0.17.21"},
    )

    assert plan["plans"][0]["action"] == "manual_restart_required"
    assert plan["plans"][0]["restart_scope"] == "host-process"
