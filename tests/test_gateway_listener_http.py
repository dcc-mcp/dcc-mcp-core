"""Public adapter listener policy over test-owned loopback HTTP (#2699)."""

from __future__ import annotations

import json
import os
from pathlib import Path
import socket
import time
from urllib.request import Request
from urllib.request import urlopen

import pytest

from conftest import McpClient
from conftest import allocate_gateway_port
from dcc_mcp_core import DccServerBase
from dcc_mcp_core._server import runtime as rt
from dcc_mcp_core.server import DccServerOptions


def _post(base, path, body):
    req = Request(
        base + path,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
    )
    with urlopen(req, timeout=5) as response:
        assert response.status == 200
        return json.load(response)


@pytest.mark.parametrize("dcc_name", ["blender", "photoshop"])
@pytest.mark.parametrize("secondary", ["default", "disabled", "loopback"])
def test_public_adapter_keeps_gateway_routes_with_secondary_policy(tmp_path, monkeypatch, dcc_name, secondary):
    monkeypatch.delenv("DCC_MCP_GATEWAY_REMOTE_HOST", raising=False)
    monkeypatch.delenv("DCC_MCP_GATEWAY_REMOTE_PORT", raising=False)
    monkeypatch.setattr(rt, "ensure_gateway_daemon", lambda **_kw: {"ok": False, "reason": "test_owned_fallback"})
    monkeypatch.setattr(rt, "_RETRY_INTERVAL_SECS", 0)
    gateway_port = allocate_gateway_port()
    remote_port = allocate_gateway_port() if secondary == "loopback" else 0
    kwargs = {} if secondary == "default" else {"gateway_remote_port": remote_port}
    options = DccServerOptions.from_env(
        dcc_name,
        tmp_path,
        gateway_port=gateway_port,
        registry_dir=str(tmp_path / "registry"),
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
        enable_checkpoint_persistence=False,
        enable_checkpoint_tools=False,
        **kwargs,
    )
    server = DccServerBase(options)
    # This guard deliberately fails before start on the pre-fix implementation:
    # the regression must never reproduce a wildcard listener on a developer host.
    assert server._config.gateway_remote_host == "127.0.0.1"
    assert server._config.gateway_remote_port == remote_port
    assert options.gateway.enable_failover is True
    unrelated = socket.socket()
    unrelated.bind(("127.0.0.1", 0))
    unrelated.listen()
    unrelated_port = unrelated.getsockname()[1]
    before = _process_listeners()
    try:
        handle = server.start(install_atexit_hook=False)
        assert handle.is_gateway is True
        after = _process_listeners()
        if before is not None and after is not None:
            new_listeners = after - before
            assert {port for _host, port in new_listeners} == {handle.port, gateway_port} | (
                {remote_port} if remote_port else set()
            )
            assert all(host == "0100007F" for host, _port in new_listeners)
        base = f"http://127.0.0.1:{gateway_port}"
        if remote_port:
            with urlopen(f"http://127.0.0.1:{remote_port}/health", timeout=5) as response:
                assert response.status == 200
        client = McpClient(base + "/mcp")
        _, listed = client.post({"jsonrpc": "2.0", "id": "list", "method": "tools/list"})
        assert {"search", "describe"} <= {tool["name"] for tool in listed["result"]["tools"]}
        deadline = time.monotonic() + 10
        while True:
            search = _post(base, "/v1/search", {"query": "gateway_failover", "dcc_type": dcc_name, "limit": 10})
            hits = [hit for hit in search.get("hits", []) if "gateway_failover" in hit.get("tool_slug", "")]
            if hits or time.monotonic() >= deadline:
                break
            time.sleep(0.1)
        assert hits, search
        slug = hits[0]["tool_slug"]
        description = _post(base, "/v1/describe", {"tool_slug": slug})
        assert "error" not in description, description
        rest = _post(base, "/v1/call", {"tool_slug": slug, "arguments": {}})
        _, mcp = client.post(
            {
                "jsonrpc": "2.0",
                "id": "call",
                "method": "tools/call",
                "params": {
                    "name": "call",
                    "arguments": {"tool_slug": slug, "arguments": {}},
                },
            }
        )
        assert rest["output"]["success"] is True
        assert mcp["result"].get("isError", False) is False
        mcp_output = json.loads(mcp["result"]["content"][0]["text"])["output"]
        for result in [rest["output"], mcp_output]:
            assert result["remote_listener"]["configured_port"] == remote_port
            assert result["remote_listener"]["effective_state"] == "unverified"
        status = server.get_gateway_election_status()
        assert status["remote_listener"]["configured_port"] == remote_port
        assert status["remote_listener"]["effective_state"] == "unverified"
        assert status["gateway_runtime_mode"] == "embedded-fallback"
    finally:
        server.stop()
        try:
            # An unrelated test-owned listener survives adapter shutdown.
            with socket.create_connection(("127.0.0.1", unrelated_port), timeout=1):
                pass
        finally:
            unrelated.close()
    # Both the adapter and its embedded gateway rows are removed.
    assert json.loads((tmp_path / "registry" / "services.json").read_text()) == []
    for port in {handle.port, gateway_port, remote_port} - {0}:
        deadline = time.monotonic() + 5
        while _is_listening(port) and time.monotonic() < deadline:
            time.sleep(0.01)
        assert not _is_listening(port), f"owned listener {port} survived shutdown"


def _is_listening(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.1):
            return True
    except OSError:
        return False


def _process_listeners():
    """On Linux, inspect only this test process's socket inodes; never probe LAN."""
    fd_dir = Path("/proc/self/fd")
    if not fd_dir.is_dir():
        return None
    inodes = set()
    for fd in fd_dir.iterdir():
        try:
            target = os.readlink(str(fd))
        except OSError:
            continue
        if target.startswith("socket:["):
            inodes.add(target[8:-1])
    listeners = set()
    for name in ["tcp", "tcp6"]:
        for row in Path(f"/proc/self/net/{name}").read_text().splitlines()[1:]:
            fields = row.split()
            if fields[3] == "0A" and fields[9] in inodes:
                host, port = fields[1].split(":")
                listeners.add((host, int(port, 16)))
    return listeners
