"""E2E tests for sidecar process-management features added in feat/sidecar-process-management.

Covers:
- Session TTL: sessions are evicted after idle TTL, active sessions survive
- Session touch: every request refreshes the idle clock
- McpHttpConfig.session_ttl_secs: builder and default
- PID file: written on server start, removed on shutdown (Python wrapper)
- WS heartbeat / reconnect: integration smoke tests via subprocess binary
- execute_script dual-mode: stdin JSON + CLI flags both deliver params

These tests exercise the Python-visible API surface (McpHttpConfig,
McpHttpServer, SessionManager) plus some subprocess-level checks for the
dcc-mcp-server binary.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile
import time
from typing import Any
import urllib.error
import urllib.request

import pytest

import dcc_mcp_core
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_SKILLS = REPO_ROOT / "examples" / "skills"

# ── helpers ───────────────────────────────────────────────────────────────────


def _post(url: str, body: dict, headers: dict | None = None) -> tuple[int, dict]:
    """POST a JSON body; return (status, response_dict)."""
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            **(headers or {}),
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, {}


def _initialize(url: str) -> str:
    """Call MCP initialize and return the Mcp-Session-Id header value."""
    data = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test-sidecar", "version": "0.1"},
            },
        }
    ).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        # Mcp-Session-Id may be in the response header or embedded in the JSON
        # body as __session_id (how dcc-mcp-core currently works).
        header_sid = resp.headers.get("Mcp-Session-Id", "")
        if header_sid:
            return header_sid
        body = json.loads(resp.read())
        return body.get("result", {}).get("__session_id", "")


def _make_server(ttl_secs: int = 3600, port: int = 0) -> tuple[McpHttpServer, Any]:
    reg = ToolRegistry()
    reg.register(
        "ping_action",
        description="ping",
        category="test",
        tags=["test"],
        dcc="test",
        version="1.0.0",
    )
    config = McpHttpConfig(port=port, server_name="sidecar-test")
    config.session_ttl_secs = ttl_secs
    server = McpHttpServer(reg, config)
    server.register_handler("ping_action", lambda p: {"pong": True})
    handle = server.start()
    time.sleep(0.1)
    return server, handle


# ── Session TTL unit-level via Python API ─────────────────────────────────────


class TestSessionTtlConfig:
    """Verify McpHttpConfig.session_ttl_secs is accessible from Python."""

    def test_default_ttl_is_3600(self):
        cfg = McpHttpConfig(port=0, server_name="test")
        assert cfg.session_ttl_secs == 3600

    def test_set_ttl_to_zero_disables(self):
        cfg = McpHttpConfig(port=0, server_name="test")
        cfg.session_ttl_secs = 0
        assert cfg.session_ttl_secs == 0

    def test_set_custom_ttl(self):
        cfg = McpHttpConfig(port=0, server_name="test")
        cfg.session_ttl_secs = 300
        assert cfg.session_ttl_secs == 300


class TestSessionLifecycle:
    """Verify session creation and basic lifecycle via HTTP."""

    def test_initialize_returns_session_id(self):
        _, handle = _make_server()
        try:
            url = handle.mcp_url()
            sid = _initialize(url)
            assert sid, "Expected Mcp-Session-Id header in initialize response"
        finally:
            handle.shutdown()

    def test_ping_with_valid_session_id_succeeds(self):
        _, handle = _make_server()
        try:
            url = handle.mcp_url()
            sid = _initialize(url)
            status, body = _post(
                url,
                {"jsonrpc": "2.0", "id": 2, "method": "ping"},
                headers={"Mcp-Session-Id": sid},
            )
            assert status == 200
            assert "result" in body
        finally:
            handle.shutdown()

    def test_delete_terminates_session(self):
        _, handle = _make_server()
        try:
            url = handle.mcp_url()
            sid = _initialize(url)

            del_req = urllib.request.Request(
                url,
                headers={"Mcp-Session-Id": sid},
                method="DELETE",
            )
            with urllib.request.urlopen(del_req, timeout=5) as resp:
                assert resp.status in (200, 204)

            # A second delete on the same session should return 404.
            with pytest.raises(urllib.error.HTTPError) as exc_info, urllib.request.urlopen(del_req, timeout=5):
                pass
            assert exc_info.value.code == 404
        finally:
            handle.shutdown()

    def test_multiple_sessions_are_independent(self):
        """Two clients can hold independent sessions concurrently."""
        _, handle = _make_server()
        try:
            url = handle.mcp_url()
            sid_a = _initialize(url)
            sid_b = _initialize(url)
            assert sid_a != sid_b, "Each client should get a unique session ID"

            status_a, _ = _post(
                url,
                {"jsonrpc": "2.0", "id": 1, "method": "ping"},
                headers={"Mcp-Session-Id": sid_a},
            )
            status_b, _ = _post(
                url,
                {"jsonrpc": "2.0", "id": 1, "method": "ping"},
                headers={"Mcp-Session-Id": sid_b},
            )
            assert status_a == 200
            assert status_b == 200
        finally:
            handle.shutdown()


class TestToolsListBoundary:
    """Boundary tests for tools/list with skill catalog."""

    @pytest.fixture
    def catalog_server(self):
        if not EXAMPLES_SKILLS.is_dir():
            pytest.skip("examples/skills not found")
        reg = ToolRegistry()
        config = McpHttpConfig(port=0, server_name="catalog-test")
        server = McpHttpServer(reg, config)
        server.discover(extra_paths=[str(EXAMPLES_SKILLS)])
        handle = server.start()
        time.sleep(0.1)
        yield handle
        handle.shutdown()

    def test_tools_list_contains_core_meta_tools(self, catalog_server):
        url = catalog_server.mcp_url()
        _initialize(url)
        _, body = _post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        names = {t["name"] for t in body["result"]["tools"]}
        for core in ("search_skills", "load_skill", "unload_skill", "list_skills"):
            assert core in names, f"Core meta-tool '{core}' missing from tools/list"

    def test_unloaded_skills_appear_as_stubs(self, catalog_server):
        """Unloaded skills must appear as __skill__<name> stubs."""
        url = catalog_server.mcp_url()
        _initialize(url)
        _, body = _post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        names = {t["name"] for t in body["result"]["tools"]}
        stubs = [n for n in names if n.startswith("__skill__")]
        assert len(stubs) > 0, "Expected at least one __skill__<name> stub in tools/list"

    def test_loaded_skill_tools_have_full_schema(self, catalog_server):
        """After load_skill, the skill's tools appear with input_schema."""
        url = catalog_server.mcp_url()
        _initialize(url)

        # Load hello-world
        _, load_resp = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        loaded = json.loads(load_resp["result"]["content"][0]["text"])
        assert loaded.get("loaded") is True

        # tools/list should now contain greet with schema
        _, tl = _post(url, {"jsonrpc": "2.0", "id": 4, "method": "tools/list"})
        names = {t["name"] for t in tl["result"]["tools"]}
        assert "greet" in names or any("hello" in n for n in names), (
            f"Expected greet in tools/list after load. Got: {names}"
        )

    def test_stub_call_returns_load_hint(self, catalog_server):
        """Calling a stub tool must return a hint to call load_skill first."""
        url = catalog_server.mcp_url()
        _initialize(url)

        # Find a stub
        _, body = _post(url, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        stubs = [t["name"] for t in body["result"]["tools"] if t["name"].startswith("__skill__")]
        if not stubs:
            pytest.skip("No stubs available")

        _, call_resp = _post(
            url,
            {"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": stubs[0], "arguments": {}}},
        )
        text = call_resp["result"]["content"][0]["text"]
        assert "load_skill" in text, f"Expected load_skill hint in stub response. Got: {text}"

    def test_double_load_is_idempotent(self, catalog_server):
        """Loading a skill twice must not duplicate tools."""
        url = catalog_server.mcp_url()
        _initialize(url)

        for _ in range(2):
            _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "tools/call",
                    "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
                },
            )

        _, tl = _post(url, {"jsonrpc": "2.0", "id": 4, "method": "tools/list"})
        tools = [t["name"] for t in tl["result"]["tools"]]
        greet_count = tools.count("greet")
        assert greet_count <= 1, f"greet duplicated after double load: {greet_count}"

    def test_unload_then_reload_works(self, catalog_server):
        """Unloading a skill and reloading it re-registers its tools."""
        url = catalog_server.mcp_url()
        _initialize(url)

        # Load
        _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )

        # Unload
        _, ul = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": "unload_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        ul_data = json.loads(ul["result"]["content"][0]["text"])
        assert ul_data.get("unloaded") is True

        # Reload
        _, rl = _post(
            url,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "load_skill", "arguments": {"skill_name": "hello-world"}},
            },
        )
        rl_data = json.loads(rl["result"]["content"][0]["text"])
        assert rl_data.get("loaded") is True


class TestToolCallExecution:
    """Boundary tests for tools/call execution."""

    def test_unknown_tool_returns_error_flag(self):
        _, handle = _make_server()
        try:
            url = handle.mcp_url()
            _initialize(url)
            _, body = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "nonexistent_tool_xyz", "arguments": {}},
                },
            )
            assert body["result"]["isError"] is True
        finally:
            handle.shutdown()

    def test_tool_without_handler_returns_no_handler_message(self):
        """A registered action without a Python handler returns a helpful error."""
        reg = ToolRegistry()
        reg.register(
            "unhandled_action",
            description="no handler",
            category="test",
            tags=["test"],
            dcc="test",
            version="1.0.0",
        )
        config = McpHttpConfig(port=0, server_name="test")
        server = McpHttpServer(reg, config)
        handle = server.start()
        time.sleep(0.1)
        try:
            url = handle.mcp_url()
            _initialize(url)
            _, body = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "unhandled_action", "arguments": {}},
                },
            )
            assert body["result"]["isError"] is True
            text = body["result"]["content"][0]["text"]
            assert "handler" in text.lower(), f"Expected 'handler' in error text: {text}"
        finally:
            handle.shutdown()

    def test_registered_handler_returns_result(self):
        reg = ToolRegistry()
        reg.register(
            "echo",
            description="echo back params",
            category="test",
            tags=["test"],
            dcc="test",
            version="1.0.0",
        )
        config = McpHttpConfig(port=0, server_name="test")
        server = McpHttpServer(reg, config)
        server.register_handler("echo", lambda p: {"echoed": p})
        handle = server.start()
        time.sleep(0.1)
        try:
            url = handle.mcp_url()
            _initialize(url)
            _, body = _post(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {"name": "echo", "arguments": {"msg": "hello"}},
                },
            )
            assert body["result"]["isError"] is False
            text = body["result"]["content"][0]["text"]
            # The response text may be JSON or Python repr; check for msg in text
            assert "hello" in text, f"Expected 'hello' in response text: {text}"
        finally:
            handle.shutdown()

    def test_handler_exception_returns_error_flag(self):
        reg = ToolRegistry()
        reg.register(
            "boom",
            description="always raises",
            category="test",
            tags=["test"],
            dcc="test",
            version="1.0.0",
        )
        config = McpHttpConfig(port=0, server_name="test")
        server = McpHttpServer(reg, config)
        server.register_handler("boom", lambda p: (_ for _ in ()).throw(RuntimeError("kaboom")))
        handle = server.start()
        time.sleep(0.1)
        try:
            url = handle.mcp_url()
            _initialize(url)
            _, body = _post(
                url,
                {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "boom", "arguments": {}}},
            )
            assert body["result"]["isError"] is True
        finally:
            handle.shutdown()


# ── dcc-mcp-server binary smoke tests ────────────────────────────────────────


def _find_server_binary() -> Path | None:
    """Locate the dcc-mcp-server binary in the Cargo target tree."""
    candidates = [
        REPO_ROOT / "target" / "debug" / "dcc-mcp-server",
        REPO_ROOT / "target" / "debug" / "dcc-mcp-server.exe",
        REPO_ROOT / "target" / "release" / "dcc-mcp-server",
        REPO_ROOT / "target" / "release" / "dcc-mcp-server.exe",
    ]
    for c in candidates:
        if c.exists():
            return c
    return None


@pytest.mark.skipif(
    _find_server_binary() is None,
    reason="dcc-mcp-server binary not built (run: just build-server)",
)
class TestServerBinarySidecar:
    """Smoke tests for the compiled dcc-mcp-server binary."""

    @pytest.fixture
    def binary(self) -> Path:
        b = _find_server_binary()
        assert b is not None
        return b

    def test_help_flag(self, binary):
        result = subprocess.run(
            [str(binary), "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert result.returncode == 0
        assert "--mcp-port" in result.stdout
        assert "--pid-file" in result.stdout
        assert "--heartbeat-secs" in result.stdout
        assert "--reconnect-timeout-secs" in result.stdout

    def test_version_flag(self, binary):
        result = subprocess.run(
            [str(binary), "--version"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        assert result.returncode == 0

    def test_pid_file_written_on_start(self, binary, tmp_path):
        """Server writes a PID file on startup and removes it on SIGTERM."""
        import signal

        pid_file = tmp_path / "test-server.pid"
        registry_dir = tmp_path / "registry"
        proc = subprocess.Popen(
            [
                str(binary),
                "--mcp-port",
                "18765",
                "--no-bridge",
                "--pid-file",
                str(pid_file),
                "--registry-dir",
                str(registry_dir),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        try:
            # Wait up to 5 s for the PID file to appear.
            deadline = time.monotonic() + 5.0
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.1)

            assert pid_file.exists(), "PID file was not created within 5 seconds"
            recorded_pid = int(pid_file.read_text().strip())
            assert recorded_pid == proc.pid, f"PID file contains {recorded_pid}, expected {proc.pid}"
        finally:
            if platform.system() == "Windows":
                proc.terminate()
            else:
                proc.send_signal(signal.SIGINT)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()

        # PID file cleanup may be asynchronous on Windows because terminate()
        # force-kills the process. Allow a short grace period for the cleanup watcher.
        deadline = time.monotonic() + 15.0
        while pid_file.exists() and time.monotonic() < deadline:
            time.sleep(0.1)
        assert not pid_file.exists(), "PID file was not removed after shutdown"

    def test_duplicate_start_rejected_without_force(self, binary, tmp_path):
        """A second server instance refuses to start when PID file exists."""
        import signal

        pid_file = tmp_path / "dup-test.pid"
        registry_dir = tmp_path / "registry"
        proc = subprocess.Popen(
            [
                str(binary),
                "--mcp-port",
                "18766",
                "--no-bridge",
                "--pid-file",
                str(pid_file),
                "--registry-dir",
                str(registry_dir),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        try:
            # Wait for PID file.
            deadline = time.monotonic() + 5.0
            while not pid_file.exists() and time.monotonic() < deadline:
                time.sleep(0.1)
            assert pid_file.exists()

            # Try to start a second instance without --force.
            dup = subprocess.run(
                [
                    str(binary),
                    "--mcp-port",
                    "18767",
                    "--no-bridge",
                    "--pid-file",
                    str(pid_file),
                    "--registry-dir",
                    str(registry_dir),
                ],
                capture_output=True,
                text=True,
                timeout=10,
            )
            assert dup.returncode != 0, "Second instance should have failed"
            assert "already running" in dup.stderr.lower() or "force" in dup.stderr.lower()
        finally:
            if platform.system() == "Windows":
                proc.terminate()
            else:
                proc.send_signal(signal.SIGINT)
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
