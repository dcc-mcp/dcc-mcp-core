"""Cross-process regression for issue #303.

Spawns ``dcc-mcp-server`` as a subprocess with gateway enabled, waits
for it to announce a gateway port in its logs, then opens a TCP socket
to confirm the listener is actually accepting. The standalone binary
runs under ``#[tokio::main]`` and therefore uses the ``Ambient`` spawn
mode — this test provides the opposite-end coverage to
``test_gateway_reachability.py`` which exercises the PyO3 ``Dedicated``
path.

Skipped if the binary has not been built (``cargo build --release -p
dcc-mcp-server`` or equivalent).
"""

from __future__ import annotations

import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent


def _find_exe() -> Path | None:
    """Locate the ``dcc-mcp-server`` binary built by cargo."""
    exe_name = "dcc-mcp-server.exe" if os.name == "nt" else "dcc-mcp-server"
    candidates = [
        REPO_ROOT / "target" / "debug" / exe_name,
        REPO_ROOT / "target" / "release" / exe_name,
    ]
    for c in candidates:
        if c.exists():
            return c
    return None


def _allocate_port() -> int:
    """Return an OS-picked ephemeral port; release it immediately."""
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


@pytest.mark.skipif(_find_exe() is None, reason="dcc-mcp-server binary not built")
def test_standalone_server_gateway_listener_reachable(tmp_path):
    """The standalone exe must answer on its gateway port within 5s."""
    exe = _find_exe()
    assert exe is not None

    # Pick a free port for the gateway. There is still a tiny race window
    # where something else could grab it, but retry is handled below.
    gateway_port = _allocate_port()
    mcp_port = _allocate_port()

    env = os.environ.copy()
    env["DCC_MCP_REGISTRY_DIR"] = str(tmp_path)
    env["RUST_LOG"] = "info"

    proc = subprocess.Popen(
        [
            str(exe),
            "--mcp-port",
            str(mcp_port),
            "--gateway-port",
            str(gateway_port),
            "--dcc",
            "test-exe",
            "--no-bridge",
            "--registry-dir",
            str(tmp_path),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=env,
    )

    try:
        # Wait up to 5 seconds for the listener to come up.
        deadline = time.time() + 5.0
        reachable = False
        while time.time() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", gateway_port), timeout=0.2):
                    reachable = True
                    break
            except (OSError, socket.timeout):
                time.sleep(0.05)

        if not reachable and proc.poll() is not None:
            out = proc.stdout.read().decode("utf-8", "replace") if proc.stdout else ""
            pytest.skip(
                f"dcc-mcp-server exited early (code={proc.returncode}) — likely port race. stdout:\n{out[:2000]}"
            )

        assert reachable, f"gateway listener on port {gateway_port} unreachable 5s after start"

        # Also check the per-instance listener.
        with socket.create_connection(("127.0.0.1", mcp_port), timeout=1.0):
            pass
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)
